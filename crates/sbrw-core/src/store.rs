//! The database: runtime tables in `main` (a file, or memory for tests), and
//! upstream's static game data in an attached in-memory `static` schema.
//!
//! The static data is upstream's `Data.sql`, converted by `tools/gen.py schema`
//! into `assets/static.sql`. It is parsed once per process into a template and
//! page-copied into each new connection. Queries use unqualified table names:
//! SQLite resolves them in `main` first, then `static`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use anyhow::Context;
use rusqlite::{Connection, MAIN_DB, params};

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
