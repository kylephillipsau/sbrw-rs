//! `RecoveryPasswordBO`: password reset keys, sent by email as a link to the
//! reset page.
//!
//! Upstream sends the mail inside the request's transaction. Sending can take
//! seconds, which would hold up the whole world here, so the steps around it
//! are two calls: [`forgot_password`] up to the mail, and
//! [`recovery_mail_sent`] once the server has sent it.

use anyhow::anyhow;
use sha1::{Digest, Sha1};
use time::{Duration, PrimitiveDateTime};

use crate::dao::recovery_password::{self, RecoveryPasswordEntity};
use crate::dao::user::{self, UserEntity};
use crate::{Fault, World, datetime, password};

/// A recovery link to mail: upstream's `MimeMessage`, and the key to store
/// once it is sent.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryMail {
    pub user_id: i64,
    pub random_key: String,
    /// The `EMAIL_FROM` parameter.
    pub from: Option<String>,
    pub to: Option<String>,
    pub subject: &'static str,
    pub text: String,
}

/// Where `forgotPassword` got to: its answer, or a mail to send first.
#[derive(Debug, Clone, PartialEq)]
pub enum Recovery {
    Done(String),
    Send(RecoveryMail),
}

// port: bo/RecoveryPasswordBO#resetPassword(String,String)
pub fn reset_password(w: &mut World, password: &str, random_key: &str) -> Result<&'static str, Fault> {
    let Some(mut recovery) = recovery_password::find_by_random_key(w.db(), random_key)? else {
        return Ok("ERROR: invalid randomKey!");
    };
    let current_time = datetime::now(&w.env);
    let recovery_time =
        recovery.expiration_date.ok_or_else(|| Fault::null_pointer("RecoveryPasswordEntity.expirationDate"))?;
    let is_close = recovery.is_close.ok_or_else(|| Fault::null_pointer("RecoveryPasswordEntity.isClose"))?;
    if is_close || current_time > recovery_time {
        return Ok("ERROR: randomKey expired");
    }
    let user_id = recovery.user_id.ok_or_else(|| anyhow!("id to load is required for loading"))?;
    let mut user = user::find(w.db(), user_id)?.ok_or_else(|| Fault::null_pointer("UserEntity"))?;
    // Upstream stores the SHA-1 hex the launcher will send; sbrw-rs stores
    // its hash, as for registration.
    user.password = Some(password::hash(&hex::encode(Sha1::digest(password.as_bytes()))));
    user::update(w.db(), &user)?;
    recovery.is_close = Some(true);
    recovery_password::update(w.db(), &recovery)?;
    Ok("Password changed!")
}

// port: bo/RecoveryPasswordBO#forgotPassword(String)
pub fn forgot_password(w: &mut World, email: Option<&str>) -> Result<Recovery, Fault> {
    // JPQL `= :email` with a null parameter matches nothing.
    let user = match email {
        Some(email) => user::find_by_email(w.db(), email)?,
        None => None,
    };
    let Some(user) = user else {
        return Ok(Recovery::Done("ERROR: Invalid email!".into()));
    };
    create_recovery_password(w, &user)
}

// port: bo/RecoveryPasswordBO#createRecoveryPassword(UserEntity)
/// Up to the mail; [`recovery_mail_sent`] is the rest.
fn create_recovery_password(w: &mut World, user: &UserEntity) -> Result<Recovery, Fault> {
    let open = recovery_password::find_all_open_by_user_id(w.db(), user.id, datetime::now(&w.env))?;
    if !open.is_empty() {
        return Ok(Recovery::Done(
            "ERROR: Recovery password link already sent, please check your spam mail box or try again in 1 hour."
                .into(),
        ));
    }
    let random_key = create_secret_key(w, user.id);
    Ok(Recovery::Send(send_email(w, random_key, user)))
}

/// The rest of `createRecoveryPassword`, once the mail is sent: the key is
/// stored, open for an hour.
pub fn recovery_mail_sent(w: &mut World, mail: &RecoveryMail) -> Result<String, Fault> {
    let mut recovery = RecoveryPasswordEntity {
        random_key: Some(mail.random_key.clone()),
        expiration_date: Some(get_hours(w, 1)),
        user_id: Some(mail.user_id),
        is_close: Some(false),
        ..Default::default()
    };
    recovery_password::insert(w.db(), &mut recovery)?;
    Ok(format!("Link to reset password sent to: [{}]", mail.to.as_deref().unwrap_or("null")))
}

// port: bo/RecoveryPasswordBO#createSecretKey(Long)
/// `Long.toHexString(Double.doubleToLongBits(Math.random()))` and the user id.
fn create_secret_key(w: &World, user_id: i64) -> String {
    let bits = w.env.random(|r| r.next_double()).to_bits();
    format!("{bits:x}{user_id}")
}

// port: bo/RecoveryPasswordBO#sendEmail(String,UserEntity)
/// The message; the HTTP layer sends it, through the configured SMTP server.
fn send_email(w: &World, random_key: String, user: &UserEntity) -> RecoveryMail {
    // String concatenation prints a missing value as "null".
    let server_address = w.params.get_str_param("SERVER_ADDRESS").unwrap_or_else(|| "null".into());
    let reset_path = w
        .params
        .get_str_param_or("SERVER_PASSWORD_RESET_PATH", Some("/soapbox-race-core/password.jsp"))
        .unwrap_or_else(|| "null".into());
    let text = format!(
        "Dear user,\n\nSomeone requested to recover forgotten password in our soapbox race world server.\n\n\
         If wasn't you, just ignore this email.\n\nYou can click this link to reset your password:\n\n\
         {server_address}{reset_path}?randomKey={random_key}\n\nThanks for playing!\n\n\n\nSBRW Team.\n"
    );
    RecoveryMail {
        user_id: user.id,
        random_key,
        from: w.params.get_str_param("EMAIL_FROM"),
        to: user.email.clone(),
        subject: "Recovery Password Email",
        text,
    }
}

// port: bo/RecoveryPasswordBO#getHours(int)
/// Now plus the hours, in `int` milliseconds as upstream computes them.
fn get_hours(w: &World, hours: i32) -> PrimitiveDateTime {
    datetime::now(&w.env) + Duration::milliseconds(i64::from(hours.wrapping_mul(60000).wrapping_mul(60)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mail_links_to_the_reset_page_with_a_new_key() {
        let mut w = World::test(&[("SERVER_ADDRESS", "http://example.com:8680")]);
        let mut user = UserEntity::new();
        user.email = Some("player@example.com".into());
        user::insert(w.db(), &mut user).unwrap();
        let Recovery::Send(mail) = forgot_password(&mut w, Some("player@example.com")).unwrap() else {
            panic!("a mail to send");
        };
        // java.util.Random(1).nextDouble()
        assert_eq!(mail.random_key, format!("{:x}{}", 0.7308781907032909f64.to_bits(), user.id));
        assert!(mail.text.contains(&format!(
            "\n\nhttp://example.com:8680/soapbox-race-core/password.jsp?randomKey={}\n\n",
            mail.random_key
        )));
        assert_eq!(
            (mail.from.as_deref(), mail.to.as_deref()),
            (Some("sbrwserver@sbrw.com"), Some("player@example.com"))
        );

        assert_eq!(recovery_mail_sent(&mut w, &mail).unwrap(), "Link to reset password sent to: [player@example.com]");
        let stored = recovery_password::find_by_random_key(w.db(), &mail.random_key).unwrap().unwrap();
        assert_eq!(stored.expiration_date, Some(datetime::now(&w.env) + Duration::HOUR));
        let Recovery::Done(again) = forgot_password(&mut w, Some("player@example.com")).unwrap() else {
            panic!("no second mail while the key is open");
        };
        assert!(again.starts_with("ERROR: Recovery password link already sent"));
        assert_eq!(reset_password(&mut w, "hunter2", &mail.random_key).unwrap(), "Password changed!");
    }
}
