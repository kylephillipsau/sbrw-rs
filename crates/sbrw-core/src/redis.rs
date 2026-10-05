//! What upstream kept in Redis, in memory.
//!
//! Upstream used Redis (when `ENABLE_REDIS` is true) for player presence, the
//! "race now" matchmaking queue and each persona's ignored events. One process
//! needs no server: this keeps the same keys and commands, and reports writes
//! to the effect log exactly as the reference recorder captured them. Hashes
//! and sets iterate in insertion order, as the recorder's store did.

use std::collections::BTreeMap;

use indexmap::{IndexMap, IndexSet};
use sbrw_common::EffectLog;
use serde_json::json;

pub struct Redis {
    strings: BTreeMap<String, String>,
    hashes: BTreeMap<String, IndexMap<String, String>>,
    sets: BTreeMap<String, IndexSet<String>>,
    effects: EffectLog,
}

impl Redis {
    pub fn new(effects: EffectLog) -> Self {
        Self { strings: BTreeMap::new(), hashes: BTreeMap::new(), sets: BTreeMap::new(), effects }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.strings.get(key).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.effects.redis("SET", vec![json!(key), json!(value)]);
        self.strings.insert(key.to_owned(), value.to_owned());
    }

    /// Removes keys of any type; returns how many existed.
    pub fn del(&mut self, keys: &[&str]) -> i64 {
        self.effects.redis("DEL", keys.iter().map(|k| json!(k)).collect());
        keys.iter()
            .filter(|k| {
                let s = self.strings.remove(**k).is_some();
                let h = self.hashes.remove(**k).is_some();
                let t = self.sets.remove(**k).is_some();
                s | h | t
            })
            .count() as i64
    }

    /// `KEYS pattern`, where `*` matches anything.
    pub fn keys(&self, pattern: &str) -> Vec<String> {
        let mut keys: Vec<&String> = self.strings.keys().chain(self.hashes.keys()).chain(self.sets.keys()).collect();
        keys.sort();
        keys.dedup();
        keys.into_iter().filter(|k| glob(pattern, k)).cloned().collect()
    }

    /// Returns whether the field is new.
    pub fn hset(&mut self, key: &str, field: &str, value: &str) -> bool {
        self.effects.redis("HSET", vec![json!(key), json!(field), json!(value)]);
        self.hashes.entry(key.to_owned()).or_default().insert(field.to_owned(), value.to_owned()).is_none()
    }

    pub fn hdel(&mut self, key: &str, fields: &[&str]) -> i64 {
        self.effects.redis("HDEL", vec![json!(key), json!(fields)]);
        match self.hashes.get_mut(key) {
            Some(hash) => fields.iter().filter(|f| hash.shift_remove(**f).is_some()).count() as i64,
            None => 0,
        }
    }

    /// `HSCAN key`: every field and value.
    pub fn hscan(&self, key: &str) -> Vec<(String, String)> {
        self.hashes.get(key).map(|h| h.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
    }

    pub fn sadd(&mut self, key: &str, members: &[&str]) -> i64 {
        self.effects.redis("SADD", vec![json!(key), json!(members)]);
        let set = self.sets.entry(key.to_owned()).or_default();
        members.iter().filter(|m| set.insert((**m).to_owned())).count() as i64
    }

    pub fn sismember(&self, key: &str, member: &str) -> bool {
        self.sets.get(key).is_some_and(|s| s.contains(member))
    }

    /// Upstream published presence changes; nothing subscribes, so this only records.
    pub fn publish(&mut self, channel: &str, message: &str) {
        self.effects.redis("PUBLISH", vec![json!(channel), json!(message)]);
    }
}

fn glob(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((prefix, rest)) => {
            let Some(tail) = text.strip_prefix(prefix) else { return false };
            (0..=tail.len()).filter(|i| tail.is_char_boundary(*i)).any(|i| glob(rest, &tail[i..]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_writes_like_the_reference() {
        let log = EffectLog::recording();
        let mut r = Redis::new(log.clone());
        r.hset("matchmaking_queue", "101", "-405837480");
        r.hset("matchmaking_queue", "102", "1");
        assert_eq!(r.hscan("matchmaking_queue")[0].0, "101");
        assert_eq!(r.hdel("matchmaking_queue", &["101"]), 1);
        r.set("game_presence.101", "1");
        assert_eq!(r.keys("game_presence.*"), ["game_presence.101"]);
        assert_eq!(r.del(&["game_presence.101", "missing"]), 1);
        assert_eq!(r.sadd("ignored_events.101", &["7", "7"]), 1);
        assert!(r.sismember("ignored_events.101", "7"));
        let commands: Vec<(String, serde_json::Value)> =
            log.drain().redis.into_iter().map(|c| (c.command, json!(c.args))).collect();
        assert_eq!(commands[2], ("HDEL".to_owned(), json!(["matchmaking_queue", ["101"]])));
        assert_eq!(commands[4], ("DEL".to_owned(), json!(["game_presence.101", "missing"])));
        assert_eq!(commands[5], ("SADD".to_owned(), json!(["ignored_events.101", ["7", "7"]])));
    }
}
