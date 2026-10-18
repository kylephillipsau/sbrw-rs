//! `serve` keeps its log in the data directory, where it outlives the container.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn logs_in(dir: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(dir) else { return String::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("sbrw."))
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap_or_default())
        .collect()
}

#[test]
fn serve_writes_its_log_into_the_data_directory() {
    let dir = std::env::temp_dir().join(format!("sbrw-log-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut server = Command::new(env!("CARGO_BIN_EXE_sbrw"))
        .arg("serve")
        .env("SBRW__SERVER__DATA_DIR", &dir)
        .env("SBRW__HTTP__BIND", "127.0.0.1:0")
        .env("SBRW__XMPP__BIND", "127.0.0.1:0")
        .env("SBRW__UDP__FREEROAM__BIND", "127.0.0.1:0")
        .env("SBRW__UDP__RACE__BIND", "127.0.0.1:0")
        .env_remove("RUST_LOG")
        .env_remove("SBRW_CONFIG")
        .env_remove("SBRW_CAPTURE")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let log = loop {
        let log = logs_in(&dir.join("logs"));
        if log.contains("HTTP listening") || Instant::now() > deadline {
            break log;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    server.kill().unwrap();
    server.wait().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(log.contains("starting") && log.contains("HTTP listening"), "{log}");
    assert!(!log.contains('\u{1b}'), "no colour codes in the file: {log}");
}
