//! Database snapshots of sbrw-rs in the recorder's format (`reference/harness/.../Db.java`).
//!
//! The recorder dumps MySQL; sbrw-rs stores the same tables and columns in
//! SQLite. These conventions make the two dumps identical:
//! - booleans are columns declared `BOOLEAN` (MySQL `bit(1)`), dumped as JSON
//!   booleans;
//! - datetimes are `TEXT` in MySQL's own format, `YYYY-MM-DD hh:mm:ss` plus
//!   `.ffffff` only when there are fractional seconds;
//! - blobs are dumped as `0x`-prefixed lowercase hex;
//! - tables without rows are left out, and rows are ordered by the first column.

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use serde_json::Value;

use crate::scenario::{Db, Row};

/// Every non-empty table except SQLite's internal ones and `skip`.
pub fn sqlite(conn: &Connection, skip: &[&str]) -> rusqlite::Result<Db> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let mut db = Db::new();
    for table in tables.into_iter().filter(|t| !skip.contains(&t.as_str())) {
        let columns: Vec<(String, String)> = conn
            .prepare(&format!(
                "SELECT name, type FROM pragma_table_info('{}') ORDER BY cid",
                table.replace('\'', "''")
            ))?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{}\" ORDER BY 1", table.replace('"', "\"\"")))?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut r = Row::new();
            for (i, (name, declared)) in columns.iter().enumerate() {
                r.insert(name.clone(), value(row.get_ref(i)?, declared));
            }
            out.push(r);
        }
        if !out.is_empty() {
            db.insert(table, out);
        }
    }
    Ok(db)
}

fn value(v: ValueRef<'_>, declared: &str) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) if declared.eq_ignore_ascii_case("BOOLEAN") => Value::Bool(i != 0),
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::String(b.iter().fold(String::from("0x"), |mut s, byte| {
            s.push_str(&format!("{byte:02x}"));
            s
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dumps_like_the_recorder() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE user (ID INTEGER PRIMARY KEY, EMAIL TEXT, premium BOOLEAN NOT NULL, created TEXT, hash BLOB);
             CREATE TABLE empty (ID INTEGER PRIMARY KEY);
             CREATE TABLE migrations (version INTEGER);
             INSERT INTO migrations VALUES (1);
             INSERT INTO user VALUES (2, 'b@example.com', 1, '2026-01-01 12:00:00', x'00ff');
             INSERT INTO user VALUES (1, NULL, 0, '2026-01-01 12:00:00.500000', NULL);",
        )
        .unwrap();
        let db = sqlite(&conn, &["migrations"]).unwrap();
        assert_eq!(db.keys().collect::<Vec<_>>(), ["user"]);
        let rows = serde_json::to_value(&db["user"]).unwrap();
        assert_eq!(
            rows,
            json!([
                { "ID": 1, "EMAIL": null, "premium": false, "created": "2026-01-01 12:00:00.500000", "hash": null },
                { "ID": 2, "EMAIL": "b@example.com", "premium": true, "created": "2026-01-01 12:00:00", "hash": "0x00ff" }
            ])
        );
        let columns: Vec<&String> = db["user"][0].keys().collect();
        assert_eq!(columns, ["ID", "EMAIL", "premium", "created", "hash"]);
    }
}
