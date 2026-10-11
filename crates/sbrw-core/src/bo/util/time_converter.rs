//! `TimeConverter`: .NET ticks (100 ns since 0001-01-01) for the game client.

use time::PrimitiveDateTime;

// port: bo/util/TimeConverter#generateGregorianCalendar(GregorianCalendar)
/// `newXMLGregorianCalendar(GregorianCalendar.from(t.atZone(systemDefault())))`
/// for a server's local time, with UTC as the system zone.
pub fn generate_gregorian_calendar(local_date_time: PrimitiveDateTime) -> sbrw_xml::DateTime {
    sbrw_xml::DateTime(sbrw_java::time::xml_gregorian_calendar(local_date_time.assume_utc()))
}

// port: bo/util/TimeConverter#getTicks(LocalDateTime)
/// A UTC server's local time, to whole seconds.
pub fn get_ticks_of(local_date_time: PrimitiveDateTime) -> i64 {
    get_ticks(local_date_time.assume_utc().unix_timestamp() * 1000)
}

// port: bo/util/TimeConverter#getTicks(long)
pub fn get_ticks(unix_timestamp_ms: i64) -> i64 {
    unix_timestamp_ms.wrapping_mul(10000).wrapping_add(621355968000000000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn ticks_match_the_recording() {
        assert_eq!(get_ticks_of(datetime!(2025-12-24 08:30:15)), 639021618150000000);
        assert_eq!(get_ticks_of(datetime!(2026-01-01 11:59:59.900)), 639028655990000000);
        assert_eq!(get_ticks(0), 621355968000000000);
    }

    #[test]
    fn calendars_match_the_recording() {
        assert_eq!(generate_gregorian_calendar(datetime!(2026-01-02 12:34:56)).0, "2026-01-02T12:34:56.000Z");
    }
}
