//! Upstream's `LocalDateTime`s: UTC wall-clock times, stored in MySQL `DATETIME`
//! columns as `YYYY-MM-DD hh:mm:ss`.

use rusqlite::Row;
use sbrw_common::Env;
use time::macros::format_description;
use time::{Duration, PrimitiveDateTime};

const FORMAT: &[time::format_description::BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// `LocalDateTime.now()` on a UTC server.
pub fn now(env: &Env) -> PrimitiveDateTime {
    let t = env.now();
    PrimitiveDateTime::new(t.date(), t.time())
}

/// How MySQL stores a value in a `DATETIME(0)` column: fractional seconds are
/// rounded half up, not truncated.
pub fn to_sql(t: PrimitiveDateTime) -> String {
    let whole = t.replace_nanosecond(0).expect("0 is a valid nanosecond");
    let rounded = if t.nanosecond() >= 500_000_000 { whole + Duration::SECOND } else { whole };
    rounded.format(FORMAT).expect("UTC datetimes always format")
}

pub fn opt_to_sql(t: Option<PrimitiveDateTime>) -> Option<String> {
    t.map(to_sql)
}

pub fn from_sql(text: &str) -> Result<PrimitiveDateTime, time::error::Parse> {
    PrimitiveDateTime::parse(text, FORMAT)
}

/// A nullable `DATETIME` column.
pub fn column(row: &Row<'_>, name: &str) -> rusqlite::Result<Option<PrimitiveDateTime>> {
    let text: Option<String> = row.get(name)?;
    text.map(|t| {
        from_sql(&t).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn rounds_like_mysql() {
        assert_eq!(to_sql(datetime!(2026-01-01 12:00:00)), "2026-01-01 12:00:00");
        assert_eq!(to_sql(datetime!(2026-01-01 12:00:00.499)), "2026-01-01 12:00:00");
        assert_eq!(to_sql(datetime!(2026-01-01 12:00:00.5)), "2026-01-01 12:00:01");
        assert_eq!(to_sql(datetime!(2026-12-31 23:59:59.999)), "2027-01-01 00:00:00");
        assert_eq!(from_sql("2026-01-02 03:04:05").unwrap(), datetime!(2026-01-02 03:04:05));
    }
}
