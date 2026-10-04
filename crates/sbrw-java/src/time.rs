//! `java.time` formatting upstream exposes to players.

use time::PrimitiveDateTime;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

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
