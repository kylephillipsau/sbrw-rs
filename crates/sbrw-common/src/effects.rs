//! Side effects that reference recordings capture besides HTTP responses.
//!
//! The shape is the fixture format written by the reference recorder
//! (`reference/harness/.../Recording.java`). Ported code reports each effect at
//! the point where upstream called out to Openfire, Redis or the EJB timer
//! service, through the real implementation; the log only observes. It is
//! disabled in production, where recording is a no-op.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Effects in the order they happened, grouped by kind as in the fixtures.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effects {
    /// Messages pushed to a persona over XMPP.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xmpp: Vec<XmppPush>,
    /// Calls upstream made to Openfire's REST API (accounts, rooms, stats).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openfire: Vec<OpenfireCall>,
    /// Single-action timers scheduled (lobby countdown, DNF).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timers: Vec<TimerScheduled>,
    /// Writes upstream made to Redis (presence, matchmaking queue).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redis: Vec<RedisCommand>,
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.xmpp.is_empty() && self.openfire.is_empty() && self.timers.is_empty() && self.redis.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XmppPush {
    /// Recipient persona id.
    pub to: i64,
    /// The message body exactly as upstream marshalled it.
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenfireCall {
    /// Upstream method name, e.g. `createUpdatePersona`.
    pub call: String,
    pub args: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimerScheduled {
    /// Upstream bean that owns the timer, e.g. `LobbyCountdownBO`.
    pub bean: String,
    pub delay_ms: i64,
    /// `String.valueOf(info)` of the timer's info object.
    pub info: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisCommand {
    pub command: String,
    pub args: Vec<Value>,
}

/// Collects effects while recording is on. Cheap to clone; clones share the log.
#[derive(Clone, Default)]
pub struct EffectLog(Option<Arc<Mutex<Effects>>>);

impl EffectLog {
    /// A log that discards everything (production).
    pub fn disabled() -> Self {
        Self(None)
    }

    /// A log that keeps effects until [`EffectLog::drain`] (replays).
    pub fn recording() -> Self {
        Self(Some(Arc::default()))
    }

    pub fn xmpp(&self, to: i64, body: impl Into<String>) {
        self.with(|e| e.xmpp.push(XmppPush { to, body: body.into() }));
    }

    pub fn openfire(&self, call: &str, args: Vec<Value>) {
        self.with(|e| e.openfire.push(OpenfireCall { call: call.to_owned(), args }));
    }

    pub fn timer(&self, bean: &str, delay_ms: i64, info: impl Into<String>) {
        self.with(|e| e.timers.push(TimerScheduled { bean: bean.to_owned(), delay_ms, info: info.into() }));
    }

    pub fn redis(&self, command: &str, args: Vec<Value>) {
        self.with(|e| e.redis.push(RedisCommand { command: command.to_owned(), args }));
    }

    /// Everything recorded since the last drain.
    pub fn drain(&self) -> Effects {
        self.0.as_ref().map(|log| std::mem::take(&mut *log.lock().expect("poisoned"))).unwrap_or_default()
    }

    fn with(&self, f: impl FnOnce(&mut Effects)) {
        if let Some(log) = &self.0 {
            f(&mut log.lock().expect("poisoned"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_reference_fixture_format() {
        let effects: Effects = serde_json::from_value(json!({
            "openfire": [{ "call": "createUpdatePersona", "args": [101, "abcdef"] }],
            "redis": [{ "command": "HDEL", "args": ["matchmaking_queue", ["101"]] }]
        }))
        .unwrap();
        assert_eq!(effects.openfire[0].call, "createUpdatePersona");
        assert_eq!(effects.redis[0].args[1], json!(["101"]));
        assert!(effects.xmpp.is_empty());
        // Empty kinds are left out, as the recorder does.
        assert_eq!(serde_json::to_value(Effects::default()).unwrap(), json!({}));
    }

    #[test]
    fn recording_log_drains_in_order() {
        let log = EffectLog::recording();
        let clone = log.clone();
        log.xmpp(101, "<a/>");
        clone.xmpp(102, "<b/>");
        log.timer("DNFTimerBO", 60_000, "7");
        let drained = log.drain();
        assert_eq!(drained.xmpp.iter().map(|p| p.to).collect::<Vec<_>>(), [101, 102]);
        assert_eq!(drained.timers[0].delay_ms, 60_000);
        assert!(log.drain().is_empty());
    }

    #[test]
    fn disabled_log_keeps_nothing() {
        let log = EffectLog::disabled();
        log.xmpp(101, "<a/>");
        assert!(log.drain().is_empty());
    }
}
