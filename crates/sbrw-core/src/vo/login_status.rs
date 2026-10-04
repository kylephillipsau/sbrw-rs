//! `LoginStatusVO`: the launcher's view of a login or registration.

use sbrw_xml::{ToXml, XmlWriter};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoginStatusVO {
    pub user_id: Option<i64>,
    pub login_token: Option<String>,
    /// Any non-empty description is shown to the player as an error.
    pub description: Option<String>,
    pub ban: Option<Ban>,
    /// `@XmlTransient`: decides the HTTP status, never sent.
    pub login_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ban {
    pub reason: Option<String>,
    pub expires: Option<String>,
}

impl LoginStatusVO {
    /// `new LoginStatusVO(userId, loginToken, loginOk)`.
    pub fn new(user_id: i64, login_token: &str, login_ok: bool) -> Self {
        Self { user_id: Some(user_id), login_token: Some(login_token.to_owned()), login_ok, ..Self::default() }
    }
}

impl ToXml for LoginStatusVO {
    fn to_xml(&self) -> String {
        let mut w = XmlWriter::new();
        w.open("LoginStatusVO");
        w.opt_value("UserId", self.user_id);
        w.opt_text("LoginToken", self.login_token.as_deref());
        w.opt_text("Description", self.description.as_deref());
        if let Some(ban) = &self.ban {
            w.open("Ban");
            w.opt_text("Reason", ban.reason.as_deref());
            w.opt_text("Expires", ban.expires.as_deref());
            w.close();
        }
        w.close();
        w.finish()
    }
}
