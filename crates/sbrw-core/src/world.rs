//! The world: all game state, owned by one thread.
//!
//! Upstream's container gave every EJB call from a REST resource its own
//! transaction. Here every [`WorldHandle::call`] is one SQLite transaction on
//! the world thread: it commits on success and on an `EngineException` (an
//! application exception upstream), and rolls back on anything else, including
//! a panic.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use anyhow::anyhow;
use rusqlite::Connection;
use sbrw_common::{EffectLog, Env};

use crate::bo::online_users::{self, OnlineUsers};
use crate::bo::token_session::TokenSessions;
use crate::openfire::Openfire;
use crate::params::Parameters;
use crate::{Fault, store};

pub struct World {
    db: Connection,
    pub env: Env,
    pub effects: EffectLog,
    pub params: Parameters,
    pub tokens: TokenSessions,
    pub online_users: OnlineUsers,
    pub openfire: Openfire,
}

pub struct WorldOptions {
    /// The database file; in memory when `None`.
    pub db_path: Option<PathBuf>,
    /// Overrides for upstream's `parameter` rows.
    pub parameters: BTreeMap<String, String>,
    pub env: Env,
    pub effects: EffectLog,
}

impl World {
    /// Open the database and run what upstream ran at startup.
    pub fn boot(options: WorldOptions) -> anyhow::Result<Self> {
        let db = store::open(options.db_path.as_deref(), &options.parameters)?;
        let params = Parameters::load(&db)?;
        let mut world = Self {
            db,
            env: options.env,
            effects: options.effects,
            params,
            tokens: TokenSessions::default(),
            online_users: OnlineUsers::default(),
            openfire: Openfire::default(),
        };
        // Upstream's @Startup beans, in the order the reference recordings show.
        world.transaction(|w| {
            w.openfire.init(&w.effects);
            Ok(())
        })?;
        world.transaction(online_users::init)?;
        Ok(world)
    }

    // port: dao/util/BaseDAO#setEntityManager(EntityManager)
    /// The database every DAO works on (upstream injected an `EntityManager`).
    pub fn db(&self) -> &Connection {
        &self.db
    }

    /// Run `f` in one transaction, with upstream's commit and rollback rules.
    pub fn transaction<R>(&mut self, f: impl FnOnce(&mut Self) -> Result<R, Fault>) -> Result<R, Fault> {
        self.db.execute_batch("BEGIN")?;
        let result = catch_unwind(AssertUnwindSafe(|| f(self)))
            .unwrap_or_else(|panic| Err(Fault::Internal(anyhow!("panic in the world: {}", panic_message(&panic)))));
        let commit = match &result {
            Ok(_) => true,
            Err(fault) => fault.commits(),
        };
        self.db.execute_batch(if commit { "COMMIT" } else { "ROLLBACK" })?;
        result
    }
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

type Job = Box<dyn FnOnce(&mut World) + Send>;

/// Sends work to the world thread. Cheap to clone; the thread stops when the
/// last handle is dropped.
#[derive(Clone)]
pub struct WorldHandle {
    jobs: mpsc::Sender<Job>,
}

impl WorldHandle {
    pub fn spawn(world: World) -> Self {
        let (jobs, inbox) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("world".into())
            .spawn(move || {
                let mut world = world;
                for job in inbox {
                    job(&mut world);
                }
            })
            .expect("spawning the world thread");
        Self { jobs }
    }

    /// Run `f` on the world thread in its own transaction.
    pub async fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut World) -> Result<R, Fault> + Send + 'static,
    ) -> Result<R, Fault> {
        let (reply, result) = tokio::sync::oneshot::channel();
        self.jobs
            .send(Box::new(move |world| {
                let _ = reply.send(world.transaction(f));
            }))
            .map_err(|_| Fault::Internal(anyhow!("the world thread has stopped")))?;
        result.await.map_err(|_| Fault::Internal(anyhow!("the world thread dropped a call")))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineExceptionCode;
    use crate::dao::user;
    use time::macros::datetime;

    fn world() -> World {
        World::boot(WorldOptions {
            db_path: None,
            parameters: BTreeMap::new(),
            env: Env::deterministic(1, datetime!(2026-01-01 12:00 UTC)),
            effects: EffectLog::recording(),
        })
        .unwrap()
    }

    fn users(w: &World) -> i64 {
        user::count_users(w.db()).unwrap()
    }

    fn insert_user(w: &mut World) -> Result<(), Fault> {
        let mut u = user::UserEntity::new();
        user::insert(w.db(), &mut u)?;
        Ok(())
    }

    #[test]
    fn boot_matches_upstreams_startup() {
        let w = world();
        let boot = w.effects.drain();
        let calls: Vec<&str> = boot.openfire.iter().map(|c| c.call.as_str()).collect();
        assert_eq!(calls, ["init", "getTotalOnlineUsers"]);
        let row: (i64, i64, i64) = w
            .db()
            .query_row("SELECT ID, numberOfOnline, numberOfRegistered FROM online_users", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(row, (1_767_268_800, 0, 0));
    }

    #[test]
    fn engine_exceptions_commit_and_other_failures_roll_back() {
        let mut w = world();
        let engine = w.transaction(|w| {
            insert_user(w)?;
            Err::<(), _>(Fault::engine(EngineExceptionCode::UnspecifiedError, false))
        });
        assert!(matches!(engine, Err(Fault::Engine(_))));
        assert_eq!(users(&w), 1, "an EngineException still commits");

        let internal = w.transaction(|w| {
            insert_user(w)?;
            Err::<(), _>(Fault::Internal(anyhow!("boom")))
        });
        assert!(matches!(internal, Err(Fault::Internal(_))));
        assert_eq!(users(&w), 1, "a system exception rolls back");

        let panicked = w.transaction(|w| -> Result<(), Fault> {
            insert_user(w)?;
            panic!("bug");
        });
        assert!(matches!(panicked, Err(Fault::Internal(_))));
        assert_eq!(users(&w), 1, "a panic rolls back");
    }

    #[tokio::test]
    async fn handle_runs_calls_on_the_world_thread() {
        let handle = WorldHandle::spawn(world());
        handle.call(insert_user).await.unwrap();
        let n = handle.call(|w| Ok(users(w))).await.unwrap();
        assert_eq!(n, 1);
    }
}
