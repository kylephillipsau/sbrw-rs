//! The database: runtime tables in `main` (a file, or memory for tests), and
//! upstream's static game data in an attached in-memory `static` schema.
//!
//! The static data is upstream's `Data.sql`, converted by `tools/gen.py schema`
//! into `assets/static.sql`. It is parsed once per process into a template and
//! page-copied into each new connection. Queries use unqualified table names:
//! SQLite resolves them in `main` first, then `static`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use anyhow::Context;
use rusqlite::{Connection, MAIN_DB, OpenFlags, params};

const STATIC_SQL: &str = include_str!("../../../assets/static.sql");

/// Schema migrations for `main`, applied in order and tracked in `user_version`.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_upstream_schema.sql"),
    include_str!("../migrations/0002_achievement_rank_rarity.sql"),
];

static TEMPLATE: LazyLock<Mutex<Connection>> = LazyLock::new(|| {
    let conn = Connection::open_in_memory().expect("in-memory SQLite");
    conn.execute_batch(STATIC_SQL).expect("assets/static.sql loads");
    Mutex::new(conn)
});

/// Open the world database at `path` (in memory when `None`), with the static
/// data attached and `parameter` rows overridden by `parameters`.
pub fn open(path: Option<&Path>, parameters: &BTreeMap<String, String>) -> anyhow::Result<Connection> {
    let mut conn = match path {
        Some(path) => {
            let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
            let applied: i64 = conn.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
            if applied > 0 && (applied as usize) < MIGRATIONS.len() {
                let dir = backups_dir(path);
                let label = format!("before-migration-{}", applied + 1);
                let copy = backup(&conn, &dir, &label).context("backing up before migrating")?;
                tracing::info!(path = %copy.display(), "backed up the database before migrating it");
            }
            conn
        }
        None => Connection::open_in_memory()?,
    };
    conn.execute_batch("PRAGMA foreign_keys = ON; ATTACH DATABASE ':memory:' AS static;")?;
    {
        let template = TEMPLATE.lock().expect("poisoned");
        let backup = rusqlite::backup::Backup::new_with_names(&template, MAIN_DB, &mut conn, c"static")?;
        // -1 copies every page in one step.
        anyhow::ensure!(
            backup.step(-1)? == rusqlite::backup::StepResult::Done,
            "copying the static data did not finish"
        );
    }
    migrate(&mut conn)?;
    // The rarity job's last results, which MySQL would have kept in the
    // static table.
    conn.execute_batch(
        "UPDATE static.achievement_rank SET rarity = (SELECT r.rarity FROM main.achievement_rank_rarity r
         WHERE r.achievement_rank_id = static.achievement_rank.ID)
         WHERE ID IN (SELECT achievement_rank_id FROM main.achievement_rank_rarity)",
    )?;
    for (name, value) in parameters {
        conn.execute("INSERT OR REPLACE INTO static.parameter (name, value) VALUES (?1, ?2)", params![name, value])?;
    }
    Ok(conn)
}

/// Where backups of the database at `db` go: `backups/` beside it.
pub fn backups_dir(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join("backups")
}

/// Copy `main` into a new file in `dir`, named for the time and `label`
/// (`world-20261018T040000Z-nightly.db`): `VACUUM INTO`, which writes a
/// consistent, compacted database without the static data.
pub fn backup(conn: &Connection, dir: &Path, label: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let now = time::OffsetDateTime::now_utc();
    let stamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let mut path = dir.join(format!("world-{stamp}-{label}.db"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("world-{stamp}-{label}-{n}.db"));
    }
    let target = path.to_str().context("a backup path that isn't UTF-8")?;
    conn.execute("VACUUM main INTO ?1", [target]).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Back up the database file at `db` through a read-only connection of its
/// own, so the world carries on meanwhile (WAL gives the copy a snapshot).
pub fn backup_file(db: &Path, label: &str) -> anyhow::Result<PathBuf> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("opening {}", db.display()))?;
    backup(&conn, &backups_dir(db), label)
}

/// Delete all but the newest `keep` backups with this label; returns what
/// it deleted.
pub fn prune_backups(dir: &Path, label: &str, keep: usize) -> anyhow::Result<Vec<PathBuf>> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with("world-") && (n.ends_with(&format!("-{label}.db")) || n.contains(&format!("-{label}-")))
            })
        })
        .collect();
    // Oldest first: by the time in the name, then by the number a second
    // backup in the same second gets.
    found.sort_by_key(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        let stamp = name.trim_start_matches("world-").split('-').next().unwrap_or_default().to_owned();
        let n: u32 = name
            .trim_end_matches(".db")
            .strip_prefix(&format!("world-{stamp}-{label}-"))
            .and_then(|n| n.parse().ok())
            .unwrap_or(1);
        (stamp, n)
    });
    let excess = found.len().saturating_sub(keep);
    let old: Vec<PathBuf> = found.into_iter().take(excess).collect();
    for path in &old {
        std::fs::remove_file(path).with_context(|| format!("deleting {}", path.display()))?;
    }
    Ok(old)
}

fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    let applied: i64 = conn.query_row("PRAGMA main.user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql).with_context(|| format!("migration {}", i + 1))?;
        tx.execute_batch(&format!("PRAGMA main.user_version = {}", i + 1))?;
        tx.commit()?;
        tracing::info!(version = i + 1, "applied database migration");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_copy_the_world_without_the_static_data() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("world.db");
        let conn = open(Some(&db), &BTreeMap::new()).unwrap();
        conn.execute("INSERT INTO user (EMAIL) VALUES ('p@example.com')", []).unwrap();
        let copy = backup_file(&db, "nightly").unwrap();
        assert_eq!(copy.parent().unwrap(), dir.path().join("backups"));
        let restored = Connection::open(&copy).unwrap();
        let email: String = restored.query_row("SELECT EMAIL FROM user", [], |r| r.get(0)).unwrap();
        assert_eq!(email, "p@example.com");
        let statics: i64 =
            restored.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'product'", [], |r| r.get(0)).unwrap();
        assert_eq!(statics, 0);
        // Same second, same label: a second name, not a failure.
        assert_ne!(backup_file(&db, "nightly").unwrap(), copy);
    }

    #[test]
    fn a_database_is_backed_up_before_it_migrates() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("world.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.execute_batch("PRAGMA user_version = 1").unwrap();
        }
        open(Some(&db), &BTreeMap::new()).unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 1);
        assert!(names[0].ends_with("-before-migration-2.db"), "{names:?}");
        // Up to date now, so opening again backs nothing up.
        open(Some(&db), &BTreeMap::new()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path().join("backups")).unwrap().count(), 1);
    }

    #[test]
    fn pruning_keeps_the_newest_of_a_label() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "world-20261016T040000Z-nightly.db",
            "world-20261017T040000Z-nightly.db",
            "world-20261017T040000Z-nightly-2.db",
            "world-20261018T040000Z-nightly.db",
            "world-20261001T000000Z-before-migration-3.db",
        ] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        let deleted = prune_backups(dir.path(), "nightly", 2).unwrap();
        let names: Vec<_> = deleted.iter().map(|p| p.file_name().unwrap().to_str().unwrap().to_owned()).collect();
        assert_eq!(names, ["world-20261016T040000Z-nightly.db", "world-20261017T040000Z-nightly.db"]);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn static_data_is_upstreams() {
        let conn = open(None, &BTreeMap::new()).unwrap();
        for (table, rows) in [
            ("product", 3712),
            ("vinylproduct", 3395),
            ("basketdefinition", 382),
            ("reward_table", 403),
            ("reward_table_item", 1288),
            ("card_pack", 108),
            ("card_pack_item", 405),
            ("event", 165),
            ("event_reward", 165),
            ("achievement", 67),
            ("achievement_rank", 240),
            ("achievement_reward", 143),
            ("badge_definition", 98),
            ("car_classes", 169),
            ("category", 54),
            ("level_rep", 60),
            ("amplifiers", 5),
            ("treasure_hunt_config", 1),
            ("chat_room", 1),
            ("parameter", 72),
        ] {
            assert_eq!(count(&conn, &format!("static.{table}")), rows, "{table}");
        }
    }

    #[test]
    fn runtime_tables_start_empty_with_upstreams_ids() {
        let conn = open(None, &BTreeMap::new()).unwrap();
        assert_eq!(count(&conn, "main.user"), 0);
        // persona ids start at 100 (AUTO_INCREMENT = 100).
        conn.execute("INSERT INTO user (EMAIL, premium) VALUES ('a@example.com', 0)", []).unwrap();
        conn.execute(
            "INSERT INTO persona (boost, cash, curCarIndex, iconIndex, level, percentToLevel, rating, rep, repAtCurrentLevel, score, USERID) VALUES (0,0,0,0,1,0,0,0,0,0,1)",
            [],
        )
        .unwrap();
        let id: i64 = conn.query_row("SELECT ID FROM persona", [], |r| r.get(0)).unwrap();
        assert_eq!(id, 100);
        // Emails compare case-insensitively, like utf8mb4_unicode_ci.
        let found: i64 =
            conn.query_row("SELECT COUNT(*) FROM user WHERE EMAIL = 'A@EXAMPLE.COM'", [], |r| r.get(0)).unwrap();
        assert_eq!(found, 1);
    }

    #[test]
    fn parameter_overrides_apply() {
        let overrides = BTreeMap::from([("TICKET_TOKEN".to_owned(), "secret".to_owned())]);
        let conn = open(None, &overrides).unwrap();
        let value: String =
            conn.query_row("SELECT value FROM parameter WHERE name = 'TICKET_TOKEN'", [], |r| r.get(0)).unwrap();
        assert_eq!(value, "secret");
    }

    #[test]
    fn migrations_run_once() {
        let dir = tempfile_dir();
        let path = dir.join("world.db");
        open(Some(&path), &BTreeMap::new()).unwrap().execute("INSERT INTO user (premium) VALUES (0)", []).unwrap();
        let conn = open(Some(&path), &BTreeMap::new()).unwrap();
        assert_eq!(count(&conn, "main.user"), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rarities_survive_a_restart_as_mysql_kept_them() {
        let dir = tempfile_dir();
        let path = dir.join("world.db");
        let rarity = |conn: &Connection, id: i64| -> f64 {
            conn.query_row("SELECT rarity FROM static.achievement_rank WHERE ID = ?1", [id], |r| r.get(0)).unwrap()
        };
        {
            let conn = open(Some(&path), &BTreeMap::new()).unwrap();
            crate::dao::achievement_rank::update_rank_rarity(&conn, 56, 0.5).unwrap();
            assert_eq!(rarity(&conn, 56), 0.5);
        }
        let conn = open(Some(&path), &BTreeMap::new()).unwrap();
        assert_eq!(rarity(&conn, 56), 0.5, "the job's rarity is back after a restart");
        assert_eq!(rarity(&conn, 57), 0.0, "other ranks keep the data's");
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sbrw-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
