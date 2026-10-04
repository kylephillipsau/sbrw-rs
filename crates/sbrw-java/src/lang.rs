//! `java.lang` parsing.

/// `Float.valueOf(s)` / `Float.parseFloat(s)`: surrounding whitespace (any
/// char <= ' ') is ignored, an `f`/`F`/`d`/`D` suffix is allowed, and the
/// special values are spelled `NaN` and `Infinity`. `None` where Java throws
/// `NumberFormatException`. Hexadecimal floats are not supported.
pub fn parse_float(s: &str) -> Option<f32> {
    let s = s.trim_matches(|c: char| c <= ' ');
    let (sign, unsigned) = match s.as_bytes().first()? {
        b'+' => (1.0, &s[1..]),
        b'-' => (-1.0, &s[1..]),
        _ => (1.0, s),
    };
    match unsigned {
        "NaN" => return Some(f32::NAN),
        "Infinity" => return Some(sign * f32::INFINITY),
        _ => {}
    }
    let digits = unsigned.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(unsigned);
    let valid = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        && digits.bytes().any(|b| b.is_ascii_digit());
    if !valid {
        return None;
    }
    digits.parse::<f32>().ok().map(|v| sign * v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_float_syntax() {
        assert_eq!(parse_float("1.5"), Some(1.5));
        assert_eq!(parse_float(" 1.5f\n"), Some(1.5));
        assert_eq!(parse_float("-2e3"), Some(-2000.0));
        assert_eq!(parse_float(".5"), Some(0.5));
        assert_eq!(parse_float("5."), Some(5.0));
        assert_eq!(parse_float("Infinity"), Some(f32::INFINITY));
        assert!(parse_float("NaN").unwrap().is_nan());
        assert_eq!(parse_float("inf"), None);
        assert_eq!(parse_float("1,5"), None);
        assert_eq!(parse_float(""), None);
        assert_eq!(parse_float("f"), None);
    }
}
