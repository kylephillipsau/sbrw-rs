//! `api/util/MiscUtils`: reading an admin command's arguments.

use anyhow::anyhow;

use crate::Fault;

// port: api/util/MiscUtils#lengthToSeconds(String)
/// A length like `1d2h30m` in seconds: every run of digits followed by `d`,
/// `h`, `m` or `s`, added up. Each run is a Java `int`, and so is its product
/// with the unit, which can overflow.
pub fn length_to_seconds(string: &str) -> Result<i64, Fault> {
    if string == "0" || string.is_empty() {
        return Ok(0);
    }
    let mut seconds = 0i64;
    for (unit, interval) in [(b'd', 86400), (b'h', 3600), (b'm', 60), (b's', 1)] {
        let bytes = string.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if !bytes[i].is_ascii_digit() {
                i += 1;
                continue;
            }
            // `([0-9]+)<unit>`: a run matches only from its start, followed by the unit.
            let end = i + bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
            if bytes.get(end) == Some(&unit) {
                let digits = &string[i..end];
                let n: i32 = digits.parse().map_err(|_| anyhow!("For input string: \"{digits}\""))?;
                seconds += i64::from(n.wrapping_mul(interval));
                i = end + 1;
            } else {
                i = end;
            }
        }
    }
    Ok(seconds)
}

// port: api/util/MiscUtils#argsToString(String[],int,int)
/// The arguments from `start` to `end`, joined by spaces.
pub fn args_to_string(args: &[&str], start: usize, end: usize) -> Result<String, Fault> {
    if start >= end {
        return Err(anyhow!("StringIndexOutOfBoundsException: index -1, length 0").into());
    }
    Ok(args[start..end].join(" "))
}

// port: api/util/MiscUtils#lengthToMiliseconds(String)
pub fn length_to_miliseconds(string: &str) -> Result<i64, Fault> {
    Ok(length_to_seconds(string)? * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_add_up_their_units() {
        let cases = [
            ("1d", 86400),
            ("2h30m", 9000),
            ("1d1d", 172800),
            ("90s", 90),
            ("0", 0),
            ("", 0),
            ("12", 0),
            ("Cheating", 0),
            ("x5mx", 300),
            ("1y2m", 120),
            // 30000 days overflows an int: 30000 × 86400 wraps.
            ("30000d", i64::from(30000i32.wrapping_mul(86400))),
        ];
        for (text, seconds) in cases {
            assert_eq!(length_to_seconds(text).unwrap(), seconds, "{text}");
        }
        assert!(length_to_seconds("99999999999d").is_err(), "not an int");
    }

    #[test]
    fn args_join_with_spaces() {
        assert_eq!(args_to_string(&["ban", "1d", "Being", "rude"], 2, 4).unwrap(), "Being rude");
        assert_eq!(args_to_string(&["ban", "x"], 1, 2).unwrap(), "x");
        assert!(args_to_string(&["ban"], 1, 1).is_err());
    }
}
