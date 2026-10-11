//! Finding scenarios and replaying one.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::compare::{self, Mismatch};
use crate::scenario::{Fixture, Scenario, StepOutcome};
use crate::server::SbrwServer;

/// A scenario and its recording.
#[derive(Debug, Clone)]
pub struct ScenarioFiles {
    /// Path under `scenarios/` without `.json`, e.g. `user/register_and_login`.
    pub name: String,
    pub scenario: PathBuf,
    pub fixture: PathBuf,
}

/// Every `scenarios/**/*.json` under the repository root, sorted by name.
pub fn discover(repo: &Path) -> anyhow::Result<Vec<ScenarioFiles>> {
    let root = repo.join("scenarios");
    let mut found = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                let rel = path.strip_prefix(&root).expect("under root").with_extension("");
                let name = rel.to_string_lossy().replace('\\', "/");
                let fixture = repo.join("fixtures/recorded").join(&rel).with_extension("json");
                found.push(ScenarioFiles { name, scenario: path, fixture });
            }
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// The scenario or its recording can't be used, independent of sbrw-rs.
#[derive(Debug)]
pub struct ReplayError(pub anyhow::Error);

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.0)
    }
}

impl std::error::Error for ReplayError {}

/// What sbrw-rs did, as a fixture, and how that differs from the recording.
#[derive(Debug)]
pub struct Replay {
    pub actual: Fixture,
    pub mismatches: Vec<Mismatch>,
}

/// Replay one scenario against a fresh sbrw-rs. Errors only when the files
/// themselves are unusable (missing, unparseable, or a stale recording);
/// anything sbrw-rs gets wrong or can't do is a mismatch.
pub async fn replay(files: &ScenarioFiles) -> Result<Replay, ReplayError> {
    let (scenario, expected) = load(files).map_err(ReplayError)?;

    let (mut server, boot) = match SbrwServer::boot(&scenario) {
        Ok(booted) => booted,
        Err(err) => {
            let actual = Fixture { steps: Vec::new(), db: Default::default(), ..sbrw_fixture(&expected) };
            let mismatches =
                vec![Mismatch { path: "boot".into(), detail: format!("sbrw-rs could not start: {err:#}") }];
            return Ok(Replay { actual, mismatches });
        }
    };

    let mut steps = Vec::with_capacity(scenario.steps.len());
    for step in &scenario.steps {
        let mut out = StepOutcome { note: step.note.clone(), ..Default::default() };
        let result: anyhow::Result<()> = async {
            if let Some(http) = &step.http {
                out.request = Some(http.clone());
                out.response = Some(server.http(http).await?);
            } else if let Some(ms) = step.advance_ms {
                out.advance_ms = Some(ms);
                server.advance(ms);
            } else if step.fire_timers == Some(true) {
                out.timers_fired = Some(server.fire_timers().await?);
            } else if let Some(job) = &step.job {
                out.job = Some(job.clone());
                server.job(job).await?;
            } else if let Some(sql) = &step.sql {
                out.sql = Some(sql.clone());
                server.sql(sql).await?;
            }
            Ok(())
        }
        .await;
        if let Err(err) = result {
            out.error = Some(format!("{err:#}"));
        }
        out.effects = server.effects();
        steps.push(out);
    }

    let mut actual = Fixture { boot, steps, ..sbrw_fixture(&expected) };
    let mut mismatches = Vec::new();
    match server.snapshot().await {
        Ok(db) => actual.db = db,
        Err(err) => mismatches.push(Mismatch { path: "db".into(), detail: format!("snapshot failed: {err:#}") }),
    }
    mismatches.splice(0..0, compare::compare(&expected, &actual));
    Ok(Replay { actual, mismatches })
}

fn sbrw_fixture(expected: &Fixture) -> Fixture {
    Fixture {
        scenario: expected.scenario.clone(),
        description: expected.description.clone(),
        upstream: format!("sbrw-rs {}", env!("CARGO_PKG_VERSION")),
        boot: Default::default(),
        steps: Vec::new(),
        db: Default::default(),
    }
}

fn load(files: &ScenarioFiles) -> anyhow::Result<(Scenario, Fixture)> {
    let scenario: Scenario = serde_json::from_str(
        &fs::read_to_string(&files.scenario).with_context(|| format!("reading {}", files.scenario.display()))?,
    )
    .with_context(|| format!("parsing {}", files.scenario.display()))?;
    let text = fs::read_to_string(&files.fixture).with_context(|| {
        format!(
            "{} has not been recorded; run reference/record.sh scenarios/{}.json",
            files.fixture.display(),
            files.name
        )
    })?;
    let fixture: Fixture =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", files.fixture.display()))?;
    check_fresh(&scenario, &fixture).with_context(|| {
        format!(
            "{} is stale; re-record it with reference/record.sh scenarios/{}.json",
            files.fixture.display(),
            files.name
        )
    })?;
    Ok((scenario, fixture))
}

/// The recording must be of this version of the scenario.
fn check_fresh(scenario: &Scenario, fixture: &Fixture) -> anyhow::Result<()> {
    anyhow::ensure!(
        scenario.steps.len() == fixture.steps.len(),
        "the scenario has {} steps, the recording {}",
        scenario.steps.len(),
        fixture.steps.len()
    );
    for (i, (s, f)) in scenario.steps.iter().zip(&fixture.steps).enumerate() {
        let same = s.note == f.note
            && s.http == f.request
            && s.advance_ms == f.advance_ms
            && s.job == f.job
            && s.sql == f.sql
            && (s.fire_timers == Some(true)) == f.timers_fired.is_some();
        anyhow::ensure!(same, "step {i} differs between the scenario and the recording");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register_and_login() -> ScenarioFiles {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        discover(&repo).unwrap().into_iter().find(|f| f.name == "user/register_and_login").unwrap()
    }

    #[test]
    fn finds_scenarios_and_their_recordings() {
        let files = register_and_login();
        assert!(files.fixture.ends_with("fixtures/recorded/user/register_and_login.json"));
        assert!(load(&files).is_ok());
    }

    #[test]
    fn rejects_a_recording_of_an_older_scenario() {
        let (mut scenario, fixture) = load(&register_and_login()).unwrap();
        scenario.steps[1].http.as_mut().unwrap().path.push('X');
        let err = check_fresh(&scenario, &fixture).unwrap_err();
        assert_eq!(err.to_string(), "step 1 differs between the scenario and the recording");
        scenario.steps.pop();
        assert!(check_fresh(&scenario, &fixture).is_err());
    }
}
