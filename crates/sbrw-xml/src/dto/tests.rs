//! Every generated class against what upstream's JAXB produced
//! (`reference/run.sh dtos`).

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{Entry, REGISTRY};

fn fixture<T: for<'de> Deserialize<'de>>(name: &str) -> T {
    let path = format!("{}/../../fixtures/dto/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn class(java: &str) -> &'static Entry {
    REGISTRY.iter().find(|(j, ..)| *j == java).unwrap_or_else(|| panic!("{java} was not generated"))
}

#[derive(Deserialize)]
struct Golden {
    sample: String,
    empty: String,
}

/// A fully populated instance, parsed and written back, is byte-for-byte what
/// JAXB wrote; `Default` writes what JAXB writes for `new X()`.
#[test]
fn every_class_writes_and_reads_like_jaxb() {
    let golden: BTreeMap<String, Golden> = fixture("golden.json");
    assert!(golden.len() > 150);
    let mut failures = Vec::new();
    for (java, g) in &golden {
        let (_, roundtrip, default) = class(java);
        match roundtrip(&g.sample) {
            Ok(xml) if xml == g.sample => {}
            Ok(xml) => failures.push(format!("{java} sample:\n  jaxb {}\n  ours {xml}", g.sample)),
            Err(err) => failures.push(format!("{java} sample: {err}")),
        }
        let empty = default();
        if empty != g.empty {
            failures.push(format!("{java} default:\n  jaxb {}\n  ours {empty}", g.empty));
        }
    }
    assert!(failures.is_empty(), "{} of {} classes differ:\n{}", failures.len(), golden.len(), failures.join("\n"));
}

#[derive(Deserialize)]
struct Probe {
    class: String,
    input: String,
    result: Option<String>,
    error: Option<String>,
}

/// Odd input read the way `JAXBUtility.unMarshal` reads it.
#[test]
fn reads_odd_input_like_jaxb() {
    let probes: Vec<Probe> = fixture("unmarshal.json");
    let mut failures = Vec::new();
    for p in &probes {
        let (_, roundtrip, _) = class(&p.class);
        match (roundtrip(&p.input), &p.result, &p.error) {
            (Ok(xml), Some(expected), _) if xml == *expected => {}
            (Err(_), None, Some(_)) => {}
            (got, expected, error) => {
                failures.push(format!(
                    "{:?}\n  jaxb {:?} {:?}\n  ours {:?}",
                    p.input,
                    expected,
                    error,
                    got.map_err(|e| e.to_string())
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{} of {} probes differ:\n{}", failures.len(), probes.len(), failures.join("\n"));
}
