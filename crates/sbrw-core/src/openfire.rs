//! What upstream asked of Openfire, now in-process.
//!
//! Upstream provisions an XMPP account per persona, the engine's own account
//! and the public chat rooms through `OpenFireRestApiCli`, and pushes messages
//! to game clients through `OpenFireSoapBoxCli`. sbrw-rs runs its own XMPP
//! server, so this keeps the same operations as in-process state for it to
//! use, and reports each one to the effect log exactly as the reference
//! recorder captured the calls.

use std::collections::HashMap;

use sbrw_common::EffectLog;
use sbrw_xml::{ToXml, XmlRoot};
use serde_json::json;

#[derive(Debug, Default)]
pub struct Openfire {
    /// XMPP username -> password.
    accounts: HashMap<String, String>,
    /// Players connected to XMPP: a test scenario's simulated count until
    /// sbrw-xmpp serves clients and reports its own.
    pub online_users: i32,
}

impl Openfire {
    // port: xmpp/OpenFireRestApiCli#init()
    /// Upstream creates the engine's account and the public chat rooms here;
    /// sbrw-xmpp creates both itself when it starts.
    pub fn init(&mut self, effects: &EffectLog) {
        effects.openfire("init", Vec::new());
    }

    // port: xmpp/OpenFireRestApiCli#getTotalOnlineUsers()
    pub fn get_total_online_users(&self, effects: &EffectLog) -> i32 {
        effects.openfire("getTotalOnlineUsers", Vec::new());
        self.online_users
    }

    // port: xmpp/OpenFireRestApiCli#createUpdatePersona(Long,String)
    /// The account `sbrw.<personaId>` with this password, created or updated.
    pub fn create_update_persona(&mut self, persona_id: i64, password: &str, effects: &EffectLog) {
        effects.openfire("createUpdatePersona", vec![json!(persona_id), json!(password)]);
        self.accounts.insert(format!("sbrw.{persona_id}"), password.to_owned());
    }

    // port: xmpp/OpenFireSoapBoxCli#send(Object,Long)
    /// Push `object`, as XML, to a persona's game client.
    pub fn send<T: XmlRoot>(&self, object: &T, to: i64, effects: &EffectLog) {
        self.send_string(&object.to_xml(), to, effects);
    }

    // port: xmpp/OpenFireSoapBoxCli#send(String,Long)
    pub fn send_string(&self, msg: &str, to: i64, effects: &EffectLog) {
        // Upstream's OpenFireConnector sends this as an XMPP message from the
        // engine's account. sbrw-xmpp will deliver it; for now the effect log
        // is its only record.
        effects.xmpp(to, msg);
    }

    /// The password of an XMPP account, for sbrw-xmpp's logins.
    pub fn password(&self, username: &str) -> Option<&str> {
        self.accounts.get(username).map(String::as_str)
    }
}
