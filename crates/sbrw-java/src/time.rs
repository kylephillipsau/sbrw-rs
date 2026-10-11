//! `java.time` formatting upstream exposes to players.

use time::{OffsetDateTime, PrimitiveDateTime};

/// `DateTimeFormatter.ofLocalizedDateTime(FormatStyle.FULL).withZone(ZoneId.systemDefault())`
/// applied to a `LocalDateTime`, as the reference JVM does it: locale en_US
/// (CLDR pattern `EEEE, MMMM d, y 'at' h:mm:ss a zzzz`) in UTC. Used for a
/// ban's expiry in `TokenSessionBO.login`.
pub fn format_full_utc(t: PrimitiveDateTime) -> String {
    let hour12 = match t.hour() % 12 {
        0 => 12,
        h => h,
    };
    format!(
        "{}, {} {}, {} at {}:{:02}:{:02} {} Coordinated Universal Time",
        t.weekday(),
        t.month(),
        t.day(),
        t.year(),
        hour12,
        t.minute(),
        t.second(),
        if t.hour() < 12 { "AM" } else { "PM" },
    )
}

/// `LocalDateTime.toString()`: seconds only when not zero, and a fraction of
/// 3, 6 or 9 digits only when there is one. Years 0 to 9999.
pub fn local_date_time_to_string(t: PrimitiveDateTime) -> String {
    let mut s = format!("{:04}-{:02}-{:02}T{:02}:{:02}", t.year(), u8::from(t.month()), t.day(), t.hour(), t.minute());
    let nanos = t.nanosecond();
    if t.second() > 0 || nanos > 0 {
        s.push_str(&format!(":{:02}", t.second()));
        if nanos > 0 && nanos.is_multiple_of(1_000_000) {
            s.push_str(&format!(".{:03}", nanos / 1_000_000));
        } else if nanos > 0 && nanos.is_multiple_of(1_000) {
            s.push_str(&format!(".{:06}", nanos / 1_000));
        } else if nanos > 0 {
            s.push_str(&format!(".{nanos:09}"));
        }
    }
    s
}

/// `XMLGregorianCalendar.toXMLFormat()` of `new GregorianCalendar()` at `t`:
/// milliseconds always, and the zone as `Z` or `±hh:mm`.
pub fn xml_gregorian_calendar(t: OffsetDateTime) -> String {
    let offset = t.offset();
    let zone = if offset.is_utc() {
        "Z".to_owned()
    } else {
        let (h, m, _) = offset.as_hms();
        format!("{}{:02}:{:02}", if offset.is_negative() { '-' } else { '+' }, h.unsigned_abs(), m.unsigned_abs())
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}{zone}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        t.millisecond()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn local_date_times_print_as_java_does() {
        use time::macros::datetime;
        assert_eq!(local_date_time_to_string(datetime!(2026-01-02 12:34:56)), "2026-01-02T12:34:56");
        assert_eq!(local_date_time_to_string(datetime!(2026-01-02 12:34)), "2026-01-02T12:34");
        assert_eq!(local_date_time_to_string(datetime!(2026-01-02 12:34:00.5)), "2026-01-02T12:34:00.500");
        assert_eq!(local_date_time_to_string(datetime!(2026-01-02 00:00:00.000_001)), "2026-01-02T00:00:00.000001");
        assert_eq!(
            local_date_time_to_string(datetime!(2026-01-02 00:00:00.000_000_001)),
            "2026-01-02T00:00:00.000000001"
        );
    }

    #[derive(Deserialize)]
    struct Vectors {
        about: String,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        ends_at: String,
        expires: String,
    }

    #[test]
    fn matches_the_jvm() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/ban_expires.json");
        let vectors: Vectors = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(vectors.about.contains("locale en_US") && vectors.about.contains("zone UTC"), "{}", vectors.about);
        let format = time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
        for case in vectors.cases {
            let t = PrimitiveDateTime::parse(&case.ends_at, format).unwrap();
            assert_eq!(format_full_utc(t), case.expires, "{}", case.ends_at);
        }
    }
}

#[cfg(test)]
mod xml_calendar_tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn formats_like_xml_gregorian_calendar() {
        assert_eq!(xml_gregorian_calendar(datetime!(2026-01-01 12:00 UTC)), "2026-01-01T12:00:00.000Z");
        assert_eq!(xml_gregorian_calendar(datetime!(2026-01-01 12:00:01.25 UTC)), "2026-01-01T12:00:01.250Z");
        assert_eq!(xml_gregorian_calendar(datetime!(2026-01-01 23:00 +11:00)), "2026-01-01T23:00:00.000+11:00");
        assert_eq!(xml_gregorian_calendar(datetime!(2026-01-01 01:00 -03:30)), "2026-01-01T01:00:00.000-03:30");
    }
}
