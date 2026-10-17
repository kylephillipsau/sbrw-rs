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

/// `Double.toString(d)`: the fewest digits that read back as `d`, rounded
/// correctly (ties to even), in Java's layout. This matches JDK 11 for every
/// value it prints in plain decimal (10^-3 <= |d| < 10^7); outside that range
/// JDK 11 sometimes prints extra digits (see the tests).
pub fn double_to_string(d: f64) -> String {
    let negative = d.is_sign_negative() && !d.is_nan();
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let d = d.abs();
    let sci = (1..=17)
        .map(|p| format!("{:.*e}", p - 1, d))
        .find(|s| s.parse::<f64>() == Ok(d))
        .expect("17 digits round-trip");
    let (digits, exponent) = digits_and_exponent(&sci);
    layout(false, &digits, exponent).replacen("", if negative { "-" } else { "" }, 1)
}

/// `Float.toString(f)`, with the same rule and caveat as [`double_to_string`].
pub fn float_to_string(f: f32) -> String {
    let negative = f.is_sign_negative() && !f.is_nan();
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let f = f.abs();
    let sci =
        (1..=9).map(|p| format!("{:.*e}", p - 1, f)).find(|s| s.parse::<f32>() == Ok(f)).expect("9 digits round-trip");
    let (digits, exponent) = digits_and_exponent(&sci);
    layout(false, &digits, exponent).replacen("", if negative { "-" } else { "" }, 1)
}

/// The digits and decimal exponent of `{:.Ne}` output (`1.2345e-5`), without
/// trailing zeros.
fn digits_and_exponent(sci: &str) -> (String, i32) {
    let (mantissa, exponent) = sci.split_once('e').expect("{:e} output");
    let digits = mantissa.replace('.', "");
    let digits = digits.trim_end_matches('0');
    (if digits.is_empty() { "0".into() } else { digits.into() }, exponent.parse().expect("exponent"))
}

/// Java's layout: plain decimal for 10^-3 <= |v| < 10^7, otherwise
/// `d.dddE±n`; always at least one digit after the point.
fn layout(negative: bool, digits: &str, exponent: i32) -> String {
    let mut out = String::with_capacity(digits.len() + 8);
    if negative {
        out.push('-');
    }
    if (-3..7).contains(&exponent) {
        if exponent >= 0 {
            let int_len = exponent as usize + 1;
            if digits.len() <= int_len {
                out.push_str(digits);
                out.extend(std::iter::repeat_n('0', int_len - digits.len()));
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
            out.push_str(digits);
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        out.push_str(if digits.len() > 1 { &digits[1..] } else { "0" });
        out.push('E');
        out.push_str(&exponent.to_string());
    }
    out
}

#[cfg(test)]
mod to_string_tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Vectors {
        float_bits_and_string: Vec<(String, String)>,
        double_bits_and_string: Vec<(String, String)>,
    }

    fn vectors() -> Vectors {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/java_number_to_string.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Exact inside Java's plain-decimal range, where game values live; outside
    /// it JDK 11 sometimes prints extra digits (`2.2856919E9` for the float
    /// 2285691904, shortest `2.285692E9`), but both read back as the same value.
    fn check<T: PartialEq + std::str::FromStr + Copy>(
        cases: &[(String, String)],
        from_bits: impl Fn(&str) -> T,
        to_string: impl Fn(T) -> String,
        plain: impl Fn(T) -> bool,
    ) -> usize {
        let mut outside = 0;
        for (bits, java) in cases {
            let v = from_bits(bits);
            let ours = to_string(v);
            if ours == *java {
                continue;
            }
            assert!(!plain(v), "{bits}: java {java}, ours {ours}");
            let same = |s: &str| s.parse::<T>().ok().is_some_and(|p| p == v);
            assert!(same(java) && same(&ours), "{bits}: java {java} and ours {ours} don't both read back");
            outside += 1;
        }
        outside
    }

    #[test]
    fn floats_match_the_jvm() {
        let v = vectors();
        let outside = check(
            &v.float_bits_and_string,
            |b| f32::from_bits(u32::from_str_radix(b, 16).unwrap()),
            float_to_string,
            |f| !f.is_finite() || f == 0.0 || (1e-3..1e7).contains(&f.abs()),
        );
        assert!(outside < v.float_bits_and_string.len() / 5, "{outside}");
    }

    #[test]
    fn doubles_match_the_jvm() {
        let v = vectors();
        let outside = check(
            &v.double_bits_and_string,
            |b| f64::from_bits(u64::from_str_radix(b, 16).unwrap()),
            double_to_string,
            |d| !d.is_finite() || d == 0.0 || (1e-3..1e7).contains(&d.abs()),
        );
        assert!(outside < v.double_bits_and_string.len() / 100, "{outside}");
    }
}

/// `Double.parseDouble(s)` for the syntax JAXB lets through: decimal with an
/// optional exponent, or a hexadecimal float (`0x1.8p3`). `None` where Java
/// throws. (Callers handle whitespace, `NaN`/`Infinity` and type suffixes.)
pub fn parse_double(s: &str) -> Option<f64> {
    let (negative, unsigned) = match s.as_bytes().first()? {
        b'+' => (false, &s[1..]),
        b'-' => (true, &s[1..]),
        _ => (false, s),
    };
    let value = if let Some(hex) = unsigned.strip_prefix("0x").or_else(|| unsigned.strip_prefix("0X")) {
        parse_hex_float(hex)?
    } else {
        let decimal = unsigned.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
            && unsigned.bytes().next().is_some_and(|b| b.is_ascii_digit() || b == b'.');
        if !decimal {
            return None;
        }
        unsigned.parse::<f64>().ok()?
    };
    Some(if negative { -value } else { value })
}

/// The part of a Java hex float after `0x`: hex digits, optional point, then a
/// mandatory binary exponent `p±n`.
fn parse_hex_float(s: &str) -> Option<f64> {
    let (mantissa, exponent) = s.split_once(['p', 'P'])?;
    let exponent: i32 = exponent.parse().ok()?;
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    let mut value = 0f64;
    for c in int.chars() {
        value = value * 16.0 + f64::from(c.to_digit(16)?);
    }
    let mut scale = 1.0 / 16.0;
    for c in frac.chars() {
        value += f64::from(c.to_digit(16)?) * scale;
        scale /= 16.0;
    }
    Some(value * 2f64.powi(exponent))
}

#[cfg(test)]
mod parse_double_tests {
    use super::*;

    #[test]
    fn java_double_syntax() {
        assert_eq!(parse_double("1e3"), Some(1000.0));
        assert_eq!(parse_double("-2.5"), Some(-2.5));
        assert_eq!(parse_double(".5"), Some(0.5));
        assert_eq!(parse_double("0x1p3"), Some(8.0));
        assert_eq!(parse_double("0x1.8p1"), Some(3.0));
        assert_eq!(parse_double("inf"), None);
        assert_eq!(parse_double("1.5f"), None);
        assert_eq!(parse_double(""), None);
    }
}

/// `String.hashCode()`: over UTF-16 code units, wrapping.
pub fn string_hash_code(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, unit| h.wrapping_mul(31).wrapping_add(i32::from(unit)))
}

/// `String.trim()`: strips every char <= ' ' from both ends.
pub fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// `s.split(sep)` for a single literal character: trailing empty strings are
/// dropped, so `";"` splits to nothing and `""` to one empty string.
pub fn split(s: &str, sep: char) -> Vec<String> {
    let mut parts: Vec<String> = s.split(sep).map(str::to_owned).collect();
    while parts.len() > 1 && parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() && !s.is_empty() {
        parts.clear();
    }
    parts
}

/// `Math.max(double, double)`: NaN if either is, and 0.0 above -0.0.
pub fn math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b { a } else { b }
}

/// `Math.min(double, double)`: NaN if either is, and -0.0 below 0.0.
pub fn math_min(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        return b;
    }
    if a <= b { a } else { b }
}

/// `Math.round(float)`: `floor(a + 0.5)` computed exactly, NaN to 0, and
/// saturating at the `int` range.
pub fn round_float(a: f32) -> i32 {
    // An f32 plus 0.5 is exact in f64, and `as` saturates and maps NaN to 0 as
    // Java's (int) cast does.
    (f64::from(a) + 0.5).floor() as i32
}

/// `Math.round(double)`: the closest `long`, ties up, worked out on the bits as
/// JDK 11 does (so 0.49999999999999994 rounds to 0); NaN is 0 and values
/// past the range saturate.
pub fn round_double(a: f64) -> i64 {
    const SIGNIFICAND_WIDTH: i64 = 53;
    const EXP_BIAS: i64 = 1023;
    const EXP_BIT_MASK: i64 = 0x7FF0_0000_0000_0000;
    const SIGNIF_BIT_MASK: i64 = 0x000F_FFFF_FFFF_FFFF;
    let long_bits = a.to_bits() as i64;
    let biased_exp = (long_bits & EXP_BIT_MASK) >> (SIGNIFICAND_WIDTH - 1);
    let shift = (SIGNIFICAND_WIDTH - 2 + EXP_BIAS) - biased_exp;
    if shift & -64 == 0 {
        let mut r = (long_bits & SIGNIF_BIT_MASK) | (SIGNIF_BIT_MASK + 1);
        if long_bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        // `(long) a`, which `as` matches: saturating, and NaN to 0.
        a as i64
    }
}

#[cfg(test)]
mod string_and_math_tests {
    use super::*;

    #[test]
    fn doubles_round_as_math_round_does() {
        for (a, rounded) in [
            (0.0, 0),
            (0.49999999999999994, 0),
            (0.5, 1),
            (2.5, 3),
            (-2.5, -2),
            (-0.5, 0),
            (-0.6, -1),
            (8046.7, 8047),
            (20116.75, 20117),
            (4503599627370497.0, 4503599627370497),
            (f64::NAN, 0),
            (1e20, i64::MAX),
            (-1e20, i64::MIN),
        ] {
            assert_eq!(round_double(a), rounded, "{a}");
        }
    }

    #[test]
    fn hash_codes_match_the_jvm() {
        // From JDK 11's String.hashCode.
        assert_eq!(string_hash_code("LOGIN"), 72611657);
        assert_eq!(string_hash_code("EVENT"), 66353786);
        assert_eq!(string_hash_code("PROGRESSION"), -1010458981);
        assert_eq!(string_hash_code("COMMERCE"), 180214491);
        assert_eq!(string_hash_code("é"), 233);
        assert_eq!(string_hash_code("😀"), 1772899);
        assert_eq!(string_hash_code(""), 0);
    }

    #[test]
    fn split_drops_trailing_empties() {
        assert_eq!(split("a|1", '|'), ["a", "1"]);
        assert_eq!(split("a|", '|'), ["a"]);
        assert_eq!(split("|1", '|'), ["", "1"]);
        assert_eq!(split("a;;b;;", ';'), ["a", "", "b"]);
        assert_eq!(split(";", ';'), Vec::<String>::new());
        assert_eq!(split("", ';'), [""]);
    }

    #[test]
    fn trims_control_characters() {
        assert_eq!(trim("\t a b \n\u{1}"), "a b");
        assert_eq!(trim("\u{a0}x\u{a0}"), "\u{a0}x\u{a0}");
    }

    #[test]
    fn rounds_like_math_round() {
        assert_eq!(round_float(0.0), 0);
        assert_eq!(round_float(2.5), 3);
        assert_eq!(round_float(-2.5), -2);
        assert_eq!(round_float(-0.5), 0);
        assert_eq!(round_float(0.49999997), 0);
        assert_eq!(round_float(1250.4999), 1250);
        assert_eq!(round_float(f32::NAN), 0);
        assert_eq!(round_float(3e9), i32::MAX);
        assert_eq!(round_float(-3e9), i32::MIN);
    }

    #[test]
    fn min_and_max_like_math() {
        assert_eq!(math_max(0.0, 5.0), 5.0);
        assert_eq!(math_min(1e8, 3.5), 3.5);
        assert!(math_max(0.0, -0.0).is_sign_positive());
        assert!(math_max(-0.0, 0.0).is_sign_positive());
        assert!(math_min(0.0, -0.0).is_sign_negative());
        assert!(math_min(-0.0, 0.0).is_sign_negative());
        assert!(math_max(0.0, f64::NAN).is_nan());
        assert!(math_max(f64::NAN, 0.0).is_nan());
        assert!(math_min(1.0, f64::NAN).is_nan());
    }
}
