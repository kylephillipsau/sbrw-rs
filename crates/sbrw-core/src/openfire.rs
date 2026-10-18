//! What upstream asked of Openfire, now in-process.
//!
//! Upstream provisions an XMPP account per persona, the engine's own account
//! and the public chat rooms through `OpenFireRestApiCli`, and pushes messages
//! to game clients through `OpenFireSoapBoxCli`. sbrw-rs runs its own XMPP
//! server, so this keeps the same operations as in-process state for it to
//! use, and reports each one to the effect log exactly as the reference
//! recorder captured the calls.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, RwLock};

use sbrw_common::EffectLog;
use sbrw_xml::{ToXml, XmlRoot};
use serde_json::json;

/// XMPP username -> password, shared with sbrw-xmpp's logins.
pub type Accounts = Arc<RwLock<HashMap<String, String>>>;

/// Persona id -> the personas in the group chat room (`group.channel.*`) it's
/// in, as sbrw-xmpp's hub keeps it (a test scenario sets it instead).
pub type GroupMembers = Arc<RwLock<HashMap<i64, Vec<i64>>>>;

/// How many players are logged in to XMPP, which sbrw-xmpp's hub keeps up to
/// date (and a test scenario sets).
pub type OnlineCount = Arc<AtomicI32>;

/// What the core hands sbrw-xmpp's hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outbound {
    /// A push to a persona's game (`OpenFireSoapBoxCli.send`).
    Push(i64, String),
    /// A message for every player, already wrapped as a system chat message.
    Announcement(String),
}

// port: xmpp/XmppChat#createSystemMessage(String)
/// A chat line from "System", as the game shows admin replies and
/// announcements. The text goes in as it is.
pub fn create_system_message(message: &str) -> String {
    format!(
        "<response status='1' ticket='0'>\n<ChatBroadcast>\n<ChatBlob>\n<FromName>System</FromName>\n\
         <FromPersonaId>0</FromPersonaId>\n<FromUserId>0</FromUserId>\n<Message>{message}</Message>\n<ToId>0</ToId>\n\
         <Type>2</Type>\n</ChatBlob>\n</ChatBroadcast>\n</response>"
    )
}

#[derive(Debug, Default)]
pub struct Openfire {
    accounts: Accounts,
    /// Where pushes and announcements go to be delivered: sbrw-xmpp's hub,
    /// when it runs.
    pub outbound: Option<tokio::sync::mpsc::UnboundedSender<Outbound>>,
    pub online_users: OnlineCount,
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
    /// Upstream asked Openfire for its client sessions and left out the
    /// engine's own; the hub has only players'.
    pub fn get_total_online_users(&self, effects: &EffectLog) -> i32 {
        effects.openfire("getTotalOnlineUsers", Vec::new());
        self.online_users.load(Ordering::Relaxed)
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
            let _ = outbound.send(Outbound::Push(to, msg.to_owned()));
        }
    }

    // port: xmpp/OpenFireRestApiCli#sendChatAnnouncement(String)
    /// Upstream posted the text to SBRW's REST plugin (`messages/game`), which
    /// wraps it in the same system chat message and broadcasts it to every
    /// session; sbrw-xmpp's hub does that. Without a message sbrw-rs
    /// announces an empty one; what the plugin made of none isn't known.
    pub fn send_chat_announcement(&self, message: Option<&str>, effects: &EffectLog) {
        effects.openfire("sendChatAnnouncement", vec![json!(message)]);
        if let Some(outbound) = &self.outbound {
            let _ = outbound.send(Outbound::Announcement(create_system_message(message.unwrap_or_default())));
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
