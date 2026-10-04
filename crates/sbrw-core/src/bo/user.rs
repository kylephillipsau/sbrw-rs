//! `UserBO`: accounts. Registration so far; the rest lands with `GetPermanentSession`.

use sbrw_java::validator::is_valid_email;

use crate::dao::invite_ticket::{self, InviteTicketEntity};
use crate::dao::user::{self, UserEntity};
use crate::vo::LoginStatus;
use crate::{Fault, World, datetime, password};

// port: bo/UserBO#createUser(String,String,String)
pub fn create_user(w: &mut World, email: Option<&str>, passwd: Option<&str>, ip: &str) -> Result<UserEntity, Fault> {
    let now = datetime::now(&w.env);
    let mut user = UserEntity::new();
    user.email = email.map(str::to_owned);
    user.password = passwd.map(password::hash);
    user.ip_address = Some(ip.to_owned());
    user.created = Some(now);
    user.last_login = Some(now);
    user::insert(w.db(), &mut user)?;
    Ok(user)
}

// port: bo/UserBO#createUserWithTicket(String,String,String,String)
pub fn create_user_with_ticket(
    w: &mut World,
    email: Option<&str>,
    passwd: Option<&str>,
    ip: &str,
    ticket: Option<&str>,
) -> Result<LoginStatus, Fault> {
    let mut status = LoginStatus::new(0, "", false);
    if !email.is_some_and(is_valid_email) {
        status.vo.description = Some("Registration Error: Invalid Email Format!".into());
        return Ok(status);
    }
    let email = email.expect("validated above");

    let mut invite = InviteTicketEntity { ticket: Some("empty-ticket".into()), ..Default::default() };
    let ticket_token = w.params.get_str_param("TICKET_TOKEN");
    if ticket_token.is_some_and(|t| t != "null") {
        match invite_ticket::find_by_ticket(w.db(), ticket)? {
            Some(found) if found.ticket.as_deref().is_some_and(|t| !t.is_empty()) => invite = found,
            _ => {
                status.vo.description = Some("Registration Error: Invalid Ticket!".into());
                return Ok(status);
            }
        }
        if invite.user(w.db())?.is_some() {
            status.vo.description = Some("Registration Error: Ticket already in use!".into());
            return Ok(status);
        }
    }

    if user::find_by_email(w.db(), email)?.is_some() {
        status.vo.description = Some("Registration Error: Registration limit reached for email!".into());
        return Ok(status);
    }

    let user = create_user(w, Some(email), passwd, ip)?;
    invite.user_id = Some(user.id);
    if invite.id == 0 {
        invite_ticket::insert(w.db(), &mut invite)?;
    } else {
        // Upstream persists the ticket it loaded in this transaction: a no-op
        // on a managed entity, whose new user is flushed at commit.
        invite_ticket::update(w.db(), &invite)?;
    }
    Ok(LoginStatus::new(user.id, "", true))
}
