//! Simple types as JAXB prints and parses them (`DatatypeConverterImpl`), as
//! pinned by `fixtures/dto/unmarshal.json`.

use sbrw_java::lang::{double_to_string, float_to_string, parse_double};

pub trait XmlValue: Sized {
    /// The lexical form JAXB writes.
    fn print(&self) -> String;
    /// The value JAXB reads from element text, or `None` where it rejects the
    /// text (the property then keeps its value).
    fn parse(text: &str) -> Option<Self>;
}

/// XML whitespace, which JAXB trims from simple values.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `_parseInt`: digits accumulate with wrapping, whitespace anywhere is
/// skipped, a `-` anywhere negates, a `+` is ignored, anything else rejects.
/// Empty text is 0.
fn parse_int(text: &str) -> Option<i32> {
    let (mut value, mut sign) = (0i32, 1i32);
    for c in text.chars() {
        match c {
            c if is_space(c) => {}
            '0'..='9' => value = value.wrapping_mul(10).wrapping_add(c as i32 - '0' as i32),
            '-' => sign = -1,
            '+' => {}
            _ => return None,
        }
    }
    Some(value.wrapping_mul(sign))
}

impl XmlValue for i32 {
    fn print(&self) -> String {
        self.to_string()
    }

    fn parse(text: &str) -> Option<Self> {
        parse_int(text)
    }
}

impl XmlValue for i16 {
    fn print(&self) -> String {
        self.to_string()
    }

    /// `(short) _parseInt`.
    fn parse(text: &str) -> Option<Self> {
        parse_int(text).map(|v| v as i16)
    }
}

impl XmlValue for i8 {
    fn print(&self) -> String {
        self.to_string()
    }

    /// `(byte) _parseInt`.
    fn parse(text: &str) -> Option<Self> {
        parse_int(text).map(|v| v as i8)
    }
}

impl XmlValue for i64 {
    fn print(&self) -> String {
        self.to_string()
    }

    /// `Long.valueOf(trimmed)`: an optional sign and digits, in range.
    fn parse(text: &str) -> Option<Self> {
        trim(text).parse().ok()
    }
}

impl XmlValue for bool {
    fn print(&self) -> String {
        self.to_string()
    }

    /// `_parseBoolean`: `true`/`1` and `false`/`0`, case-sensitive.
    fn parse(text: &str) -> Option<Self> {
        match trim(text) {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    }
}

/// `_parseDouble`/`_parseFloat`: `NaN`, `INF`, `-INF`, or text that starts and
/// ends with a digit, point or sign and that Java parses.
fn parse_real(text: &str) -> Option<f64> {
    let text = trim(text);
    match text {
        "NaN" => return Some(f64::NAN),
        "INF" => return Some(f64::INFINITY),
        "-INF" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'));
    if !edge(text.chars().next()) || !edge(text.chars().last()) {
        return None;
    }
    parse_double(text)
}

impl XmlValue for f64 {
    /// `_printDouble`: `Double.toString`, with `INF`/`-INF` for infinities.
    fn print(&self) -> String {
        match *self {
            f64::INFINITY => "INF".into(),
            f64::NEG_INFINITY => "-INF".into(),
            v => double_to_string(v),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        parse_real(text)
    }
}

impl XmlValue for f32 {
    /// `_printFloat`: `Float.toString`, with `INF`/`-INF` for infinities.
    fn print(&self) -> String {
        match *self {
            f32::INFINITY => "INF".into(),
            f32::NEG_INFINITY => "-INF".into(),
            v => float_to_string(v),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let text = trim(text);
        // Float.parseFloat rounds decimal text straight to float.
        if !text.starts_with("0x") && !text.starts_with("-0x") && !text.starts_with("+0x") {
            if let Some(v) = parse_real(text) {
                return Some(text.parse::<f32>().unwrap_or(v as f32));
            }
            return None;
        }
        parse_real(text).map(|v| v as f32)
    }
}

impl XmlValue for String {
    fn print(&self) -> String {
        self.clone()
    }

    /// Strings are taken as is, whitespace included.
    fn parse(text: &str) -> Option<Self> {
        Some(text.to_owned())
    }
}

/// An `xs:dateTime` (`XMLGregorianCalendar`), kept in its lexical form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DateTime(pub String);

impl XmlValue for DateTime {
    fn print(&self) -> String {
        self.0.clone()
    }

    fn parse(text: &str) -> Option<Self> {
        Some(Self(trim(text).to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints_parse_like_jaxb() {
        assert_eq!(i32::parse(" 12 "), Some(12));
        assert_eq!(i32::parse("+12"), Some(12));
        assert_eq!(i32::parse("1 2"), Some(12));
        assert_eq!(i32::parse("3000000000"), Some(-1_294_967_296));
        assert_eq!(i32::parse(""), Some(0));
        assert_eq!(i32::parse("12.5"), None);
        assert_eq!(i64::parse("+12"), Some(12));
        assert_eq!(i64::parse(""), None);
        assert_eq!(i64::parse("99999999999999999999"), None);
    }

    #[test]
    fn reals_print_and_parse_like_jaxb() {
        assert_eq!(f32::INFINITY.print(), "INF");
        assert_eq!(1.2345678e-4f32.print(), "1.2345678E-4");
        assert_eq!(9_999_999.0f64.print(), "9999999.0");
        assert_eq!(f64::parse("1.5f"), None);
        assert_eq!(f64::parse("Infinity"), None);
        assert_eq!(f64::parse("0x1p3"), Some(8.0));
        assert_eq!(f32::parse(" 2.5 "), Some(2.5));
        assert!(f64::parse("NaN").unwrap().is_nan());
    }

    #[test]
    fn booleans_parse_like_jaxb() {
        assert_eq!(bool::parse("1"), Some(true));
        assert_eq!(bool::parse(" false "), Some(false));
        assert_eq!(bool::parse("TRUE"), None);
    }
}
