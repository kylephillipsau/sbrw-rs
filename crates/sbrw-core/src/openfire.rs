//! What upstream asked of Openfire, now in-process.
//!
//! Upstream provisions an XMPP account per persona, the engine's own account
//! and the public chat rooms through `OpenFireRestApiCli`, and pushes messages
//! to game clients through `OpenFireSoapBoxCli`. sbrw-rs runs its own XMPP
//! server, so this keeps the same operations as in-process state for it to
//! use, and reports each one to the effect log exactly as the reference
//! recorder captured the calls.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use sbrw_common::EffectLog;
use sbrw_xml::{ToXml, XmlRoot};
use serde_json::json;

/// XMPP username -> password, shared with sbrw-xmpp's logins.
pub type Accounts = Arc<RwLock<HashMap<String, String>>>;

/// Persona id -> the personas in the group chat room (`group.channel.*`) it's
/// in, as sbrw-xmpp's hub keeps it (a test scenario sets it instead).
pub type GroupMembers = Arc<RwLock<HashMap<i64, Vec<i64>>>>;

#[derive(Debug, Default)]
pub struct Openfire {
    accounts: Accounts,
    /// Where pushes go to be delivered: sbrw-xmpp's hub, when it runs.
    pub outbound: Option<tokio::sync::mpsc::UnboundedSender<(i64, String)>>,
    /// Players connected to XMPP: a test scenario's simulated count until
    /// sbrw-xmpp serves clients and reports its own.
    pub online_users: i32,
    pub group_members: GroupMembers,
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
        self.accounts.write().expect("accounts lock").insert(format!("sbrw.{persona_id}"), password.to_owned());
    }

    // port: xmpp/OpenFireRestApiCli#getAllPersonaByGroup(Long)
    // port: xmpp/OpenFireRestApiCli#getAllOccupantsInRoom(String)
    /// The personas in the group chat room the persona is in; none when it
    /// isn't in one. Upstream asked Openfire's REST API for the persona's
    /// rooms, then the first group room's occupants.
    pub fn get_all_persona_by_group(&self, persona_id: i64, effects: &EffectLog) -> Vec<i64> {
        effects.openfire("getAllPersonaByGroup", vec![json!(persona_id)]);
        self.group_members.read().expect("groups lock").get(&persona_id).cloned().unwrap_or_default()
    }

    // port: xmpp/OpenFireSoapBoxCli#send(Object,Long)
    /// Push `object`, as XML, to a persona's game client.
    pub fn send<T: XmlRoot>(&self, object: &T, to: i64, effects: &EffectLog) {
        self.send_string(&object.to_xml(), to, effects);
    }

    // port: xmpp/OpenFireSoapBoxCli#send(String,Long)
    pub fn send_string(&self, msg: &str, to: i64, effects: &EffectLog) {
        // Upstream's OpenFireConnector sends this at once as an XMPP message
        // from the engine's account, whether or not the transaction commits.
        effects.xmpp(to, msg);
        if let Some(outbound) = &self.outbound {
            let _ = outbound.send((to, msg.to_owned()));
        }
    }

    /// The password of an XMPP account, for sbrw-xmpp's logins.
    pub fn password(&self, username: &str) -> Option<String> {
        self.accounts.read().expect("accounts lock").get(username).cloned()
    }

    /// The accounts, for sbrw-xmpp to check logins against while the world runs.
    pub fn accounts(&self) -> Accounts {
        self.accounts.clone()
    }
}
