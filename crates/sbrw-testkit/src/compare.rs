//! Compares what sbrw-rs did with what the original core recorded.
//!
//! Exact, except for deliberate and documented normalisations:
//! - the text of `<StackTrace>` elements in error bodies (Java stack traces);
//! - JSON numbers compare by value (`1` equals `1.0`), and JSON bodies compare
//!   as values, since property order is a serializer detail;
//! - only the `Connection` and `Content-Type` response headers are compared;
//! - `user.PASSWORD` holds an argon2id hash in sbrw-rs, where upstream stores
//!   the SHA-1 the launcher sent; the hash must verify against it;
//! - a car's paints, performance parts, skill mods, vinyls and visual parts
//!   compare as unordered, in bodies and in their tables (whose row ids are
//!   then compared as a set). Upstream holds them in `HashSet`s of entities
//!   with identity hash codes, so their order, and the order they are
//!   inserted in, depends on the JVM's hash codes; no response shows a
//!   part's id.

use std::fmt;

use sbrw_common::Effects;
use serde_json::Value;

use crate::scenario::{Db, Fixture, HttpResponse, StepOutcome};

/// One difference between the recording and the replay.
#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    /// Where, e.g. `steps[2] (GET /Engine.svc/User/authenticateUser) response.body`.
    pub path: String,
    pub detail: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.path)?;
        for line in self.detail.lines() {
            writeln!(f, "    {line}")?;
        }
        Ok(())
    }
}

pub fn compare(expected: &Fixture, actual: &Fixture) -> Vec<Mismatch> {
    let mut out = Vec::new();
    effects("boot", &expected.boot, &actual.boot, &mut out);

    if expected.steps.len() != actual.steps.len() {
        out.push(Mismatch {
            path: "steps".into(),
            detail: format!("expected {} steps, replayed {}", expected.steps.len(), actual.steps.len()),
        });
    }
    for (i, (e, a)) in expected.steps.iter().zip(&actual.steps).enumerate() {
        step(&step_label(i, e), e, a, &mut out);
    }
    db(&expected.db, &actual.db, &mut out);
    out
}

fn step_label(i: usize, step: &StepOutcome) -> String {
    if let Some(request) = &step.request {
        format!("steps[{i}] ({})", request.label())
    } else if let Some(ms) = step.advance_ms {
        format!("steps[{i}] (advance {ms} ms)")
    } else if step.timers_fired.is_some() {
        format!("steps[{i}] (fire timers)")
    } else if let Some(job) = &step.job {
        format!("steps[{i}] (job {job})")
    } else {
        format!("steps[{i}]")
    }
}

fn step(label: &str, e: &StepOutcome, a: &StepOutcome, out: &mut Vec<Mismatch>) {
    if let Some(error) = &a.error {
        out.push(Mismatch { path: label.to_owned(), detail: format!("sbrw-rs could not perform this step: {error}") });
    }
    match (&e.response, &a.response) {
        (Some(e), Some(a)) => response(label, e, a, out),
        (Some(_), None) => {
            if a.error.is_none() {
                out.push(Mismatch { path: format!("{label} response"), detail: "no response".into() });
            }
        }
        (None, Some(_)) => {
            out.push(Mismatch { path: format!("{label} response"), detail: "unexpected response".into() });
        }
        (None, None) => {}
    }
    if e.timers_fired != a.timers_fired && a.error.is_none() {
        out.push(Mismatch {
            path: format!("{label} timers_fired"),
            detail: format!("expected {:?}, got {:?}", e.timers_fired, a.timers_fired),
        });
    }
    effects(&format!("{label} effects"), &e.effects, &a.effects, out);
}

fn response(label: &str, e: &HttpResponse, a: &HttpResponse, out: &mut Vec<Mismatch>) {
    if e.status != a.status {
        out.push(Mismatch {
            path: format!("{label} response.status"),
            detail: format!("expected {}, got {}", e.status, a.status),
        });
    }
    for name in ["Connection", "Content-Type"] {
        let (eh, ah) = (e.header(name), a.header(name));
        if eh != ah {
            out.push(Mismatch {
                path: format!("{label} response.headers.{name}"),
                detail: format!("expected {}, got {}", show_opt(&eh), show_opt(&ah)),
            });
        }
    }
    let is_json = e.header("Content-Type").is_some_and(|ct| ct.contains("json"));
    let (eb, ab) = (normalize_body(&e.body), normalize_body(&a.body));
    let same = if is_json {
        match (serde_json::from_str::<Value>(&eb), serde_json::from_str::<Value>(&ab)) {
            (Ok(ev), Ok(av)) => values_equal(&ev, &av),
            _ => eb == ab,
        }
    } else {
        eb == ab
    };
    if !same {
        out.push(Mismatch { path: format!("{label} response.body"), detail: describe_strings(&eb, &ab) });
    }
}

fn effects(path: &str, e: &Effects, a: &Effects, out: &mut Vec<Mismatch>) {
    let (ev, av) =
        (serde_json::to_value(e).expect("effects serialize"), serde_json::to_value(a).expect("effects serialize"));
    for kind in ["xmpp", "openfire", "timers", "redis"] {
        let empty = Vec::new();
        let el = ev.get(kind).and_then(Value::as_array).unwrap_or(&empty);
        let al = av.get(kind).and_then(Value::as_array).unwrap_or(&empty);
        for i in 0..el.len().max(al.len()) {
            match (el.get(i), al.get(i)) {
                (Some(x), Some(y)) if values_equal(x, y) => {}
                (x, y) => out.push(Mismatch {
                    path: format!("{path}.{kind}[{i}]"),
                    detail: format!("expected {}\ngot      {}", show_value(x), show_value(y)),
                }),
            }
        }
    }
}

/// The tables behind a car's `Set`s of parts, and their id column.
const UNORDERED_TABLES: [&str; 5] = ["paint", "performancepart", "skillmodpart", "vinyl", "visualpart"];

/// An unordered table's rows without their ids, sorted, and its ids, sorted.
fn unordered(rows: &[crate::scenario::Row]) -> (Vec<crate::scenario::Row>, Vec<String>) {
    let mut ids: Vec<String> = rows.iter().map(|r| r.get("id").map_or_else(String::new, Value::to_string)).collect();
    ids.sort();
    let mut rows: Vec<crate::scenario::Row> = rows
        .iter()
        .map(|r| r.iter().filter(|(k, _)| *k != "id").map(|(k, v)| (k.clone(), v.clone())).collect())
        .collect();
    rows.sort_by_cached_key(|r| serde_json::to_string(r).expect("rows serialize"));
    (rows, ids)
}

fn db(e: &Db, a: &Db, out: &mut Vec<Mismatch>) {
    let tables: std::collections::BTreeSet<&String> = e.keys().chain(a.keys()).collect();
    for table in tables {
        let (er, ar) = (e.get(table).map_or(&[][..], Vec::as_slice), a.get(table).map_or(&[][..], Vec::as_slice));
        let sorted;
        let (er, ar) = if UNORDERED_TABLES.contains(&table.as_str()) {
            let ((er, eids), (ar, aids)) = (unordered(er), unordered(ar));
            if eids != aids {
                out.push(Mismatch {
                    path: format!("db.{table}.id"),
                    detail: format!("expected ids {eids:?}, got {aids:?}"),
                });
            }
            sorted = (er, ar);
            (sorted.0.as_slice(), sorted.1.as_slice())
        } else {
            (er, ar)
        };
        if er.len() != ar.len() {
            out.push(Mismatch {
                path: format!("db.{table}"),
                detail: format!("expected {} rows, got {}", er.len(), ar.len()),
            });
        }
        for (i, (erow, arow)) in er.iter().zip(ar).enumerate() {
            let mut columns: Vec<&String> = erow.keys().collect();
            columns.extend(arow.keys().filter(|k| !erow.contains_key(*k)));
            for column in columns {
                let (x, y) = (erow.get(column), arow.get(column));
                let same = match (x, y) {
                    (Some(Value::String(sha1)), Some(Value::String(hash)))
                        if table == "user" && column == "PASSWORD" && hash.starts_with("$argon2") =>
                    {
                        sbrw_core::password::matches(sha1, hash)
                    }
                    (Some(x), Some(y)) => values_equal(x, y),
                    _ => false,
                };
                if !same {
                    out.push(Mismatch {
                        path: format!("db.{table}[{i}].{column}"),
                        detail: format!("expected {}, got {}", show_value(x), show_value(y)),
                    });
                }
            }
        }
    }
}

/// Replace the text of every `<StackTrace>` element (Java stack traces can't
/// be reproduced), and sort the children of a car's unordered part lists.
pub fn normalize_body(body: &str) -> String {
    const OPEN: &str = "<StackTrace>";
    const CLOSE: &str = "</StackTrace>";
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find(OPEN) {
        let after = start + OPEN.len();
        let Some(len) = rest[after..].find(CLOSE) else { break };
        out.push_str(&rest[..after]);
        out.push('…');
        rest = &rest[after + len..];
    }
    out.push_str(rest);
    ["Paints", "PerformanceParts", "SkillModParts", "Vinyls", "VisualParts"]
        .into_iter()
        .fold(out, |body, list| sort_children(&body, list))
}

/// Sort the child elements of every `<list>` element. The children are flat
/// records, so each ends at its first closing tag. This applies wherever the
/// element appears in a body; today only `CustomCarTrans` has these names.
/// A response that uses one for an ordered list would need this narrowed.
fn sort_children(body: &str, list: &str) -> String {
    let (open, close) = (format!("<{list}>"), format!("</{list}>"));
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find(&open) {
        let inner_start = start + open.len();
        let Some(len) = rest[inner_start..].find(&close) else { break };
        let mut inner = &rest[inner_start..inner_start + len];
        let mut children = Vec::new();
        while let Some(name) = inner.strip_prefix('<').and_then(|s| s.split(['>', '/', ' ']).next()) {
            let end_tag = format!("</{name}>");
            let end = match inner.find('>') {
                Some(gt) if inner[..gt].ends_with('/') => gt + 1,
                _ => match inner.find(&end_tag) {
                    Some(at) => at + end_tag.len(),
                    None => break,
                },
            };
            children.push(&inner[..end]);
            inner = &inner[end..];
        }
        children.sort_unstable();
        out.push_str(&rest[..inner_start]);
        children.into_iter().for_each(|c| out.push_str(c));
        out.push_str(inner);
        rest = &rest[inner_start + len..];
    }
    out.push_str(rest);
    out
}

/// JSON equality where numbers compare by value.
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_i64(), y.as_i64()) {
            (Some(x), Some(y)) => x == y,
            _ => x.as_f64() == y.as_f64(),
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| values_equal(x, y)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| values_equal(v, w)))
        }
        _ => a == b,
    }
}

fn show_value(v: Option<&Value>) -> String {
    v.map_or_else(|| "(nothing)".to_owned(), Value::to_string)
}

fn show_opt(v: &Option<String>) -> String {
    v.as_deref().map_or_else(|| "(none)".to_owned(), |s| format!("{s:?}"))
}

/// Short strings in full; long ones around the first difference.
fn describe_strings(expected: &str, actual: &str) -> String {
    let show = |s: &str| if s.is_empty() { "(empty)".to_owned() } else { s.to_owned() };
    if expected.len() + actual.len() <= 240 {
        return format!("expected {}\ngot      {}", show(expected), show(actual));
    }
    let at = expected
        .char_indices()
        .zip(actual.chars())
        .find(|((_, x), y)| x != y)
        .map_or_else(|| expected.len().min(actual.len()), |((i, _), _)| i);
    let window = |s: &str| {
        let start = floor_char(s, at.saturating_sub(60));
        let end = floor_char(s, (at + 100).min(s.len()));
        let (pre, post) = (if start > 0 { "…" } else { "" }, if end < s.len() { "…" } else { "" });
        format!("{pre}{}{post}", &s[start..end])
    };
    format!(
        "first difference at byte {at} (expected {} bytes, got {})\nexpected {}\ngot      {}",
        expected.len(),
        actual.len(),
        window(expected),
        if actual.is_empty() { "(empty)".to_owned() } else { window(actual) }
    )
}

fn floor_char(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{HttpRequest, StepOutcome};
    use indexmap::IndexMap;
    use serde_json::json;

    fn response(status: u16, content_type: &str, body: &str) -> HttpResponse {
        let mut headers = IndexMap::new();
        headers.insert("Connection".to_owned(), json!("close"));
        headers.insert("Content-Type".to_owned(), json!(content_type));
        HttpResponse { status, headers, body: body.to_owned() }
    }

    fn fixture(response: HttpResponse) -> Fixture {
        let request = HttpRequest {
            method: "GET".into(),
            path: "/Engine.svc/x".into(),
            query: None,
            headers: None,
            form: None,
            body: None,
            content_type: None,
            remote_addr: None,
        };
        Fixture {
            scenario: "t".into(),
            description: None,
            upstream: "test".into(),
            boot: Effects::default(),
            steps: vec![StepOutcome { request: Some(request), response: Some(response), ..Default::default() }],
            db: Db::new(),
        }
    }

    #[test]
    fn identical_fixtures_match() {
        let f = fixture(response(200, "application/xml;charset=UTF-8", "<a/>"));
        assert!(compare(&f, &f.clone()).is_empty());
    }

    #[test]
    fn reports_status_header_and_body_differences() {
        let e = fixture(response(500, "application/xml;charset=UTF-8", "<a>1</a>"));
        let a = fixture(response(200, "application/xml", "<a>2</a>"));
        let paths: Vec<String> = compare(&e, &a).into_iter().map(|m| m.path).collect();
        assert_eq!(
            paths,
            [
                "steps[0] (GET /Engine.svc/x) response.status",
                "steps[0] (GET /Engine.svc/x) response.headers.Content-Type",
                "steps[0] (GET /Engine.svc/x) response.body",
            ]
        );
    }

    #[test]
    fn stack_traces_are_normalised() {
        let body = |trace: &str| {
            format!(
                "<EngineExceptionTrans><ErrorCode>-1</ErrorCode><StackTrace>{trace}</StackTrace></EngineExceptionTrans>"
            )
        };
        let e = fixture(response(503, "application/xml", &body("java.lang.Exception\n\tat Foo.bar(Foo.java:1)")));
        let a = fixture(response(503, "application/xml", &body("sbrw-rs")));
        assert!(compare(&e, &a).is_empty());
        assert_eq!(
            normalize_body("<StackTrace>x</StackTrace><StackTrace></StackTrace>"),
            "<StackTrace>…</StackTrace><StackTrace>…</StackTrace>"
        );
    }

    #[test]
    fn json_bodies_compare_as_values() {
        let e = fixture(response(200, "application/json", r#"{"a":1.0,"b":[true]}"#));
        let a = fixture(response(200, "application/json", r#"{"b":[true],"a":1}"#));
        assert!(compare(&e, &a).is_empty());
    }

    #[test]
    fn effects_and_db_differences_are_located() {
        let mut e = fixture(response(200, "application/xml", ""));
        let mut a = e.clone();
        e.steps[0].effects.xmpp.push(sbrw_common::effects::XmppPush { to: 101, body: "<x/>".into() });
        let row = |email: &str| -> crate::scenario::Row {
            [("ID".to_owned(), json!(1)), ("EMAIL".to_owned(), json!(email))].into()
        };
        e.db.insert("user".into(), vec![row("a@example.com")]);
        a.db.insert("user".into(), vec![row("b@example.com")]);
        let paths: Vec<String> = compare(&e, &a).into_iter().map(|m| m.path).collect();
        assert_eq!(paths, ["steps[0] (GET /Engine.svc/x) effects.xmpp[0]", "db.user[0].EMAIL"]);
    }

    #[test]
    fn stored_passwords_are_verified_not_compared() {
        let sha1 = "5baa61e4c9b93f3f0682250b6cf8331b7ee68fd8";
        let mut e = fixture(response(200, "application/xml", ""));
        let mut a = e.clone();
        let row = |pw: &str| -> crate::scenario::Row { [("PASSWORD".to_owned(), json!(pw))].into() };
        e.db.insert("user".into(), vec![row(sha1)]);
        a.db.insert("user".into(), vec![row(&sbrw_core::password::hash(sha1))]);
        assert!(compare(&e, &a).is_empty());
        a.db.insert("user".into(), vec![row(&sbrw_core::password::hash("something else"))]);
        assert_eq!(compare(&e, &a).len(), 1);
    }

    #[test]
    fn car_part_lists_compare_unordered() {
        let car = |paints: &str| {
            format!(
                "<OwnedCarTrans><CustomCar><Paints>{paints}</Paints><Vinyls/></CustomCar><Id>1</Id></OwnedCarTrans>"
            )
        };
        let (p1, p2) = (
            "<CustomPaintTrans><Slot>1</Slot></CustomPaintTrans>",
            "<CustomPaintTrans><Slot>2</Slot></CustomPaintTrans>",
        );
        let e = fixture(response(200, "application/xml", &car(&format!("{p1}{p2}"))));
        assert!(compare(&e, &fixture(response(200, "application/xml", &car(&format!("{p2}{p1}"))))).is_empty());
        assert_eq!(compare(&e, &fixture(response(200, "application/xml", &car(&format!("{p1}{p1}"))))).len(), 1);

        let (mut e, mut a) = (e.clone(), e);
        let row = |id: i64, slot: i64| -> crate::scenario::Row {
            [("id".to_owned(), json!(id)), ("slot".to_owned(), json!(slot)), ("carId".to_owned(), json!(1))].into()
        };
        e.db.insert("paint".into(), vec![row(1, 5), row(2, 0)]);
        a.db.insert("paint".into(), vec![row(1, 0), row(2, 5)]);
        assert!(compare(&e, &a).is_empty(), "ids are a set, rows a multiset");
        a.db.insert("paint".into(), vec![row(1, 0), row(3, 5)]);
        let paths: Vec<String> = compare(&e, &a).into_iter().map(|m| m.path).collect();
        assert_eq!(paths, ["db.paint.id"]);
        a.db.insert("car".into(), vec![row(1, 0), row(2, 5)]);
        e.db.insert("car".into(), vec![row(1, 5), row(2, 0)]);
        assert_eq!(compare(&e, &a).len(), 3, "other tables stay ordered");
    }

    #[test]
    fn numbers_compare_by_value() {
        assert!(values_equal(&json!(1), &json!(1.0)));
        assert!(!values_equal(&json!(1), &json!(2)));
        assert!(!values_equal(&json!(1), &json!("1")));
    }

    #[test]
    fn long_strings_show_the_first_difference() {
        let e = format!("{}X{}", "a".repeat(200), "b".repeat(200));
        let a = format!("{}Y{}", "a".repeat(200), "b".repeat(200));
        let d = describe_strings(&e, &a);
        assert!(d.starts_with("first difference at byte 200"), "{d}");
        assert!(d.contains("aX") && d.contains("aY"), "{d}");
    }
}
