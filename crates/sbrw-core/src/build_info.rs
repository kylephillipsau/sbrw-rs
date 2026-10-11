//! `BuildInfo`: which build of the server is running.
//!
//! Upstream reads its `git.properties`; sbrw-rs takes the same five values
//! from `SBRW_GIT_*` variables set when it is compiled. Without them, every
//! value is `unknown`, as upstream reports when the file is missing, which is
//! also how the reference recordings were made.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    branch: String,
    commit_id: String,
    long_commit_id: String,
    time: String,
    version: String,
}

impl BuildInfo {
    /// What upstream reports without `git.properties`.
    pub fn unknown() -> Self {
        let unknown = || "unknown".to_owned();
        Self { branch: unknown(), commit_id: unknown(), long_commit_id: unknown(), time: unknown(), version: unknown() }
    }

    /// This binary's build, from `SBRW_GIT_BRANCH`, `SBRW_GIT_COMMIT`,
    /// `SBRW_GIT_COMMIT_LONG`, `SBRW_GIT_TIME` and `SBRW_VERSION` at compile time.
    pub fn of_this_build() -> Self {
        let or_unknown = |v: Option<&str>| v.unwrap_or("unknown").to_owned();
        Self {
            branch: or_unknown(option_env!("SBRW_GIT_BRANCH")),
            commit_id: or_unknown(option_env!("SBRW_GIT_COMMIT")),
            long_commit_id: or_unknown(option_env!("SBRW_GIT_COMMIT_LONG")),
            time: or_unknown(option_env!("SBRW_GIT_TIME")),
            version: option_env!("SBRW_VERSION").unwrap_or(concat!("sbrw-rs ", env!("CARGO_PKG_VERSION"))).to_owned(),
        }
    }

    // port: api/util/BuildInfo#getBranch()
    pub fn branch(&self) -> &str {
        self.branch.trim()
    }

    // port: api/util/BuildInfo#getCommitID()
    pub fn commit_id(&self) -> &str {
        self.commit_id.trim()
    }

    // port: api/util/BuildInfo#getLongCommitID()
    pub fn long_commit_id(&self) -> &str {
        self.long_commit_id.trim()
    }

    // port: api/util/BuildInfo#getTime()
    pub fn time(&self) -> &str {
        self.time.trim()
    }

    // port: api/util/BuildInfo#getVersion()
    pub fn version(&self) -> &str {
        self.version.trim()
    }
}
