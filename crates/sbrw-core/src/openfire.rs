//! What upstream asked of Openfire's REST API, now in-process.
//!
//! Upstream provisions an XMPP account per persona, the engine's own account
//! and the public chat rooms through `OpenFireRestApiCli`. sbrw-rs runs its own
//! XMPP server, so this keeps the same operations as in-process state for it to
//! use, and reports each one to the effect log exactly as the reference
//! recorder captured the REST calls.

use std::collections::HashMap;

use sbrw_common::EffectLog;
use serde_json::json;

#[derive(Debug, Default)]
pub struct Openfire {
    /// XMPP username -> password.
    accounts: HashMap<String, String>,
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
        // Nobody can be connected until sbrw-xmpp serves clients.
        0
    }

    // port: xmpp/OpenFireRestApiCli#createUpdatePersona(Long,String)
    /// The account `sbrw.<personaId>` with this password, created or updated.
    pub fn create_update_persona(&mut self, persona_id: i64, password: &str, effects: &EffectLog) {
        effects.openfire("createUpdatePersona", vec![json!(persona_id), json!(password)]);
        self.accounts.insert(format!("sbrw.{persona_id}"), password.to_owned());
    }

    /// The password of an XMPP account, for sbrw-xmpp's logins.
    pub fn password(&self, username: &str) -> Option<&str> {
        self.accounts.get(username).map(String::as_str)
    }
}
