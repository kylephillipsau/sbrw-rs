//! The ModNet file tree under `/mods`, backed by `<data_dir>/mods`.
//!
//! After `GetModInfo` the launcher fetches `{basePath}/index.json`, deletes any
//! cached package not listed there, and downloads every listed package whose
//! SHA-1 differs from its cached copy. The index is generated from the
//! directory contents, so adding a mod is just dropping the file in.
//!
//! `cars.json` and `events.json` (Discord presence names) are served only if
//! present: when they 404 the launcher falls back to its built-in lists, while
//! a present-but-empty list would blank those names out.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sha1::{Digest, Sha1};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio_util::io::ReaderStream;

use crate::AppState;

const INDEX: &str = "index.json";
/// Served when present but never listed as mod packages.
const RESERVED: [&str; 3] = [INDEX, "cars.json", "events.json"];

#[derive(Debug, Serialize)]
pub struct ModIndex {
    built_at: String,
    entries: Vec<ModEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ModEntry {
    name: String,
    /// Lowercase hex SHA-1, compared against the launcher's cached copy.
    checksum: String,
}

#[derive(Clone, PartialEq)]
struct Fingerprint {
    len: u64,
    modified: SystemTime,
}

pub struct ModsDir {
    dir: PathBuf,
    /// SHA-1 per file name, reused while size and mtime are unchanged.
    hashes: Mutex<HashMap<String, (Fingerprint, String)>>,
}

impl ModsDir {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, hashes: Mutex::default() }
    }

    pub async fn index(&self) -> io::Result<ModIndex> {
        let mut files = Vec::new();
        match tokio::fs::read_dir(&self.dir).await {
            Ok(mut dir) => {
                while let Some(entry) = dir.next_entry().await? {
                    let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
                    let meta = entry.metadata().await?;
                    if meta.is_file() && is_package_name(&name) {
                        files.push((name, Fingerprint { len: meta.len(), modified: meta.modified()? }));
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));

        let built_at = files.iter().map(|(_, fp)| fp.modified).max().unwrap_or(SystemTime::UNIX_EPOCH);
        let mut entries = Vec::with_capacity(files.len());
        for (name, fingerprint) in files {
            let checksum = self.checksum(&name, fingerprint).await?;
            entries.push(ModEntry { name, checksum });
        }
        let built_at = OffsetDateTime::from(built_at).format(&Rfc3339).map_err(io::Error::other)?;
        Ok(ModIndex { built_at, entries })
    }

    async fn checksum(&self, name: &str, fingerprint: Fingerprint) -> io::Result<String> {
        if let Some((cached, hash)) = self.hashes.lock().expect("poisoned").get(name)
            && *cached == fingerprint
        {
            return Ok(hash.clone());
        }
        let path = self.dir.join(name);
        let hash = tokio::task::spawn_blocking(move || sha1_file(&path)).await.map_err(io::Error::other)??;
        self.hashes.lock().expect("poisoned").insert(name.to_owned(), (fingerprint, hash.clone()));
        Ok(hash)
    }
}

/// A plain file name: no directories, no hidden files, nothing reserved.
fn is_package_name(name: &str) -> bool {
    is_safe_name(name) && !RESERVED.contains(&name)
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

fn sha1_file(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// `GET /mods/{name}`: the generated index, or a file from the mods directory.
pub async fn file(State(state): State<AppState>, UrlPath(name): UrlPath<String>) -> Response {
    if name == INDEX {
        return match state.mods.index().await {
            Ok(index) => Json(index).into_response(),
            Err(err) => {
                tracing::error!(%err, dir = %state.mods.dir.display(), "failed to build the mod index");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        };
    }
    if !is_safe_name(&name) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::File::open(state.mods.dir.join(&name)).await {
        Ok(file) => {
            let content_type = if name.ends_with(".json") { "application/json" } else { "application/octet-stream" };
            ([(CONTENT_TYPE, content_type)], Body::from_stream(ReaderStream::new(file))).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
