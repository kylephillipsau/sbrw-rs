//! Upstream's value objects: the generated XML classes (`sbrw_xml::dto`), plus
//! the few fields upstream keeps on them that JAXB never writes.

mod mod_info;

pub use mod_info::ModInfoVO;
use sbrw_xml::dto::login::LoginStatusVO;

/// `LoginStatusVO` together with its `@XmlTransient loginOk`, which decides
/// the HTTP status of a login or registration.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoginStatus {
    pub vo: LoginStatusVO,
    pub login_ok: bool,
}

impl LoginStatus {
    /// `new LoginStatusVO(userId, loginToken, loginOk)`.
    pub fn new(user_id: i64, login_token: &str, login_ok: bool) -> Self {
        let vo =
            LoginStatusVO { user_id: Some(user_id), login_token: Some(login_token.to_owned()), ..Default::default() };
        Self { vo, login_ok }
    }
}
