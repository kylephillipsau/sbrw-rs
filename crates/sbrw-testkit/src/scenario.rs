//! Scenario inputs and recorded fixtures, in the reference recorder's JSON
//! format (`reference/harness/.../Scenario.java` and `Recorder.java`).

use std::collections::BTreeMap;

use indexmap::IndexMap;
use sbrw_common::Effects;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

/// A scenario file: what to do, step by step.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub description: Option<String>,
    /// Seed for the shared `java.util.Random`.
    #[serde(default = "default_seed")]
    pub seed: i64,
    /// Starting time of the frozen clock.
    #[serde(default = "default_clock", with = "time::serde::rfc3339")]
    pub clock: OffsetDateTime,
    /// Overrides for upstream's `parameter` table.
    #[serde(default)]
    pub parameters: Option<BTreeMap<String, String>>,
    /// Simulated Openfire state the core may query.
    #[serde(default)]
    pub openfire: Option<OpenfireState>,
    pub steps: Vec<Step>,
}

fn default_seed() -> i64 {
    1
}

fn default_clock() -> OffsetDateTime {
    time::macros::datetime!(2026-01-01 0:00 UTC)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenfireState {
    #[serde(default)]
    pub online_users: i32,
    /// Persona id -> members of the group chat room that persona is in.
    #[serde(default)]
    pub groups: BTreeMap<String, Vec<i64>>,
}

/// Exactly one of the actions is set.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub note: Option<String>,
    pub http: Option<HttpRequest>,
    pub advance_ms: Option<i64>,
    pub fire_timers: Option<bool>,
    /// A scheduled job, named after upstream's `@Schedule` method: `BanBO#checkExpiredBans`.
    pub job: Option<String>,
    /// SQL statements run directly against the database, for rows no API
    /// creates. Write them so MySQL and SQLite read them alike: plain
    /// `INSERT`s, ISO datetimes.
    pub sql: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpRequest {
    #[serde(default = "default_method")]
    pub method: String,
    pub path: String,
    pub query: Option<IndexMap<String, String>>,
    pub headers: Option<IndexMap<String, String>>,
    /// Sent as an `application/x-www-form-urlencoded` body.
    pub form: Option<IndexMap<String, String>>,
    pub body: Option<String>,
    /// Content type of `body`; `application/xml` when absent.
    pub content_type: Option<String>,
    /// Client address the server sees; `127.0.0.1` when absent.
    pub remote_addr: Option<String>,
}

fn default_method() -> String {
    "GET".into()
}

impl HttpRequest {
    pub fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

/// A recording: the result of running a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fixture {
    pub scenario: String,
    pub description: Option<String>,
    /// Which implementation produced it, e.g. `SoapboxRaceWorld/soapbox-race-core@<sha>`.
    pub upstream: String,
    /// Effects of starting the server.
    pub boot: Effects,
    pub steps: Vec<StepOutcome>,
    /// Every runtime table with rows, in primary-key order.
    pub db: Db,
}

/// Table name -> rows; each row maps column name -> value.
pub type Db = BTreeMap<String, Vec<Row>>;
pub type Row = IndexMap<String, Value>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StepOutcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<HttpRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<HttpResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advance_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timers_fired: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Effects::is_empty")]
    pub effects: Effects,
    /// Set when sbrw-rs could not perform the step at all (never in recordings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    /// As the recorder writes them: canonical names, single values as strings.
    pub headers: IndexMap<String, Value>,
    pub body: String,
}

impl HttpResponse {
    /// Header value by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_recorded_fixture() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/recorded/user/register_and_login.json");
        let fixture: Fixture = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(fixture.scenario, "user/register_and_login");
        let first = &fixture.steps[0];
        assert_eq!(first.request.as_ref().unwrap().path, "/Engine.svc/User/createUser");
        let response = first.response.as_ref().unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.header("content-type").as_deref(), Some("application/xml;charset=UTF-8"));
        assert_eq!(fixture.db["user"][0]["EMAIL"], "player@example.com");
    }

    #[test]
    fn reads_the_scenario() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/user/register_and_login.json");
        let scenario: Scenario = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(scenario.seed, 1);
        assert_eq!(scenario.clock, time::macros::datetime!(2026-01-01 12:00 UTC));
        let query = scenario.steps[0].http.as_ref().unwrap().query.as_ref().unwrap();
        assert_eq!(query.keys().collect::<Vec<_>>(), ["email", "password"]);
    }
}
