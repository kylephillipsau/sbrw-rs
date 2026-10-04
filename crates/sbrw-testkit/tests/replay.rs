//! Replays every recorded scenario against sbrw-rs, one test per scenario.
//!
//!   cargo test -p sbrw-testkit --test replay
//!   cargo test -p sbrw-testkit --test replay -- user/register_and_login
//!
//! Scenarios in port/replay-pending.toml are expected to fail and are marked
//! `[pending]`; one that starts passing fails the run until it is removed from
//! the list, so the list only ever shrinks. Set SBRW_REPLAY_REPORT=1 to print
//! the differences for pending scenarios too. What sbrw-rs did is written to
//! `<target tmp>/replay/<scenario>.json` for diffing against the recording.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use libtest_mimic::{Arguments, Failed, Trial};
use sbrw_testkit::{Replay, ScenarioFiles};

fn main() -> ExitCode {
    let args = Arguments::from_args();
    let repo = match Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() {
        Ok(repo) => repo,
        Err(err) => {
            eprintln!("locating the repository: {err}");
            return ExitCode::FAILURE;
        }
    };
    let out_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("replay");
    let pending = match pending(&repo) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("port/replay-pending.toml: {err:#}");
            return ExitCode::FAILURE;
        }
    };
    let scenarios = match sbrw_testkit::discover(&repo) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("finding scenarios: {err:#}");
            return ExitCode::FAILURE;
        }
    };

    let names: BTreeSet<String> = scenarios.iter().map(|s| s.name.clone()).collect();
    let unknown: Vec<String> = pending.difference(&names).cloned().collect();
    let mut trials = vec![Trial::test("port/replay-pending.toml names real scenarios", move || {
        if unknown.is_empty() { Ok(()) } else { Err(format!("no such scenarios: {}", unknown.join(", ")).into()) }
    })];
    for files in scenarios {
        let is_pending = pending.contains(&files.name);
        let out = out_dir.join(format!("{}.json", files.name));
        let trial = Trial::test(files.name.clone(), move || run(&files, is_pending, &out));
        trials.push(if is_pending { trial.with_kind("pending") } else { trial });
    }
    libtest_mimic::run(&args, trials).exit_code()
}

fn pending(repo: &Path) -> anyhow::Result<BTreeSet<String>> {
    #[derive(serde::Deserialize)]
    struct File {
        pending: Vec<String>,
    }
    let text = std::fs::read_to_string(repo.join("port/replay-pending.toml"))?;
    Ok(toml::from_str::<File>(&text)?.pending.into_iter().collect())
}

fn run(files: &ScenarioFiles, is_pending: bool, out: &PathBuf) -> Result<(), Failed> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    // Unusable files fail even for pending scenarios: they need re-recording, not porting.
    let Replay { actual, mismatches } = runtime.block_on(sbrw_testkit::replay(files))?;

    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, serde_json::to_string_pretty(&actual)? + "\n")?;

    let report = || {
        let mut s = format!("{} difference(s) from the recording:\n\n", mismatches.len());
        for m in &mismatches {
            s.push_str(&m.to_string());
        }
        s.push_str(&format!("\nsbrw-rs output: {}\nrecording:      {}\n", out.display(), files.fixture.display()));
        s
    };
    match (is_pending, mismatches.is_empty()) {
        (false, true) => Ok(()),
        (false, false) => Err(report().into()),
        (true, true) => Err("this scenario now replays exactly; remove it from port/replay-pending.toml".into()),
        (true, false) => {
            if std::env::var_os("SBRW_REPLAY_REPORT").is_some() {
                println!("\n{}: {}", files.name, report());
            }
            Ok(())
        }
    }
}
