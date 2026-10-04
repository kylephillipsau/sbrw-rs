//! Apache Commons Validator 1.7: the email check upstream applies at registration.
//!
//! Ported from `org.apache.commons.validator.routines.{EmailValidator,
//! DomainValidator, InetAddressValidator}` (Apache-2.0). Only the default
//! instances are needed: `EmailValidator.getInstance()`, which allows neither
//! local hostnames nor bare TLDs. Java regex semantics are spelled out
//! explicitly where Rust's differ: `.` excludes every Java line terminator,
//! `\s`/`\S`/`\d`/`\p{Alnum}`/`\p{Cntrl}` are ASCII-only, and lengths count
//! UTF-16 code units. The TLD lists are the JVM's own (`tlds.rs`).

mod tlds;

use std::sync::LazyLock;

use regex::Regex;

/// Java's `.`: anything except a line terminator.
const ANY: &str = r"[^\n\r\x{85}\x{2028}\x{2029}]";
/// Java's `\s` (without UNICODE_CHARACTER_CLASS), as a regex class body.
const SPACE: &str = r" \t\n\x0B\x0C\r";

fn is_java_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r')
}

/// `EmailValidator.getInstance().isValid(email)`.
pub fn is_valid_email(email: &str) -> bool {
    // "check this first - it's cheap!"
    if email.ends_with('.') {
        return false;
    }
    static EMAIL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!("^({ANY}+)@([^{SPACE}]+)$")).expect("EMAIL_REGEX"));
    let Some(m) = EMAIL.captures(email) else { return false };
    is_valid_user(&m[1]) && is_valid_email_domain(&m[2])
}

fn is_valid_user(user: &str) -> bool {
    const MAX_USERNAME_LEN: usize = 64;
    if utf16_len(user) > MAX_USERNAME_LEN {
        return false;
    }
    static USER: LazyLock<Regex> = LazyLock::new(|| {
        let special = r#"\x00-\x1F\x7F\(\)<>@,;:'\\"\.\[\]"#;
        let valid_chars = format!(r"(?:\\{ANY})|[^{SPACE}{special}]");
        let quoted_user = r#"(?:"(?:\\"|[^"])*")"#;
        let word = format!("(?:(?:{valid_chars}|')+|{quoted_user})");
        Regex::new(&format!(r"^{word}(?:\.{word})*$")).expect("USER_REGEX")
    });
    USER.is_match(user)
}

fn is_valid_email_domain(domain: &str) -> bool {
    static IP_DOMAIN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!(r"^\[({ANY}*)\]$")).expect("IP_DOMAIN_REGEX"));
    if let Some(m) = IP_DOMAIN.captures(domain) {
        return is_valid_inet_address(&m[1]);
    }
    is_valid_domain(domain)
}

/// `DomainValidator.getInstance(false).isValid(domain)`.
pub fn is_valid_domain(domain: &str) -> bool {
    const MAX_DOMAIN_LENGTH: usize = 253;
    let domain = unicode_to_ascii(domain);
    if utf16_len(&domain) > MAX_DOMAIN_LENGTH {
        return false;
    }
    // DOMAIN_NAME_REGEX. Upstream's atomic groups `(?>...)` don't change what a
    // label can match here, so plain groups are equivalent.
    static DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
        let label = "[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?";
        let top = "[a-zA-Z](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?";
        Regex::new(&format!(r"^(?:{label}\.)+({top})\.?$")).expect("DOMAIN_NAME_REGEX")
    });
    match DOMAIN.captures(&domain) {
        Some(m) => is_valid_tld(&m[1]),
        // allowLocal is false for the default instance, so no bare hostnames.
        None => false,
    }
}

fn is_valid_tld(tld: &str) -> bool {
    // toLowerCase(Locale.ENGLISH)
    let key = unicode_to_ascii(tld).to_lowercase();
    let key = key.strip_prefix('.').unwrap_or(&key);
    [&tlds::INFRASTRUCTURE_TLDS[..], &tlds::GENERIC_TLDS, &tlds::COUNTRY_CODE_TLDS]
        .iter()
        .any(|list| list.binary_search(&key).is_ok())
}

/// `DomainValidator.unicodeToASCII`: `IDN.toASCII`, keeping a trailing full stop.
fn unicode_to_ascii(input: &str) -> String {
    if input.is_ascii() {
        return input.to_owned();
    }
    let Ok(ascii) = idna::domain_to_ascii(input) else { return input.to_owned() };
    match input.chars().last() {
        Some('.' | '\u{3002}' | '\u{FF0E}' | '\u{FF61}') if !ascii.ends_with('.') => ascii + ".",
        _ => ascii,
    }
}

/// `InetAddressValidator.getInstance().isValid(address)`.
pub fn is_valid_inet_address(address: &str) -> bool {
    is_valid_inet4_address(address) || is_valid_inet6_address(address)
}

fn is_valid_inet4_address(address: &str) -> bool {
    static IPV4: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})$").expect("IPV4_REGEX"));
    let Some(m) = IPV4.captures(address) else { return false };
    m.iter().skip(1).flatten().all(|segment| {
        let s = segment.as_str();
        s.parse::<u32>().is_ok_and(|n| n <= 255) && !(s.len() > 1 && s.starts_with('0'))
    })
}

fn is_valid_inet6_address(address: &str) -> bool {
    const IPV6_MAX_HEX_GROUPS: usize = 8;
    let parts: Vec<&str> = address.split('/').collect();
    if parts.len() > 2 {
        return false;
    }
    if parts.len() == 2 {
        let bits = parts[1];
        if !(1..=3).contains(&bits.len()) || !bits.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        if bits.parse::<u32>().is_ok_and(|b| b > 128) {
            return false;
        }
    }
    let parts: Vec<&str> = parts[0].split('%').collect();
    if parts.len() > 2 {
        return false;
    }
    if parts.len() == 2 {
        let zone = parts[1];
        if zone.is_empty() || zone.chars().any(|c| is_java_space(c) || c == '/' || c == '%') {
            return false;
        }
    }
    let address = parts[0];
    let compressed = address.contains("::");
    if compressed && address.find("::") != address.rfind("::") {
        return false;
    }
    if (address.starts_with(':') && !address.starts_with("::")) || (address.ends_with(':') && !address.ends_with("::"))
    {
        return false;
    }
    let mut octets = java_split(address, ':');
    if compressed {
        if address.ends_with("::") {
            octets.push("");
        } else if address.starts_with("::") && !octets.is_empty() {
            octets.remove(0);
        }
    }
    if octets.len() > IPV6_MAX_HEX_GROUPS {
        return false;
    }
    let (mut valid, mut empty) = (0, 0);
    for (index, octet) in octets.iter().enumerate() {
        if octet.is_empty() {
            empty += 1;
            if empty > 1 {
                return false;
            }
        } else {
            empty = 0;
            if index == octets.len() - 1 && octet.contains('.') {
                if !is_valid_inet4_address(octet) {
                    return false;
                }
                valid += 2;
                continue;
            }
            if utf16_len(octet) > 4 {
                return false;
            }
            match java_parse_hex(octet) {
                Some(n) if (0..=0xffff).contains(&n) => {}
                _ => return false,
            }
        }
        valid += 1;
    }
    !(valid > IPV6_MAX_HEX_GROUPS || (valid < IPV6_MAX_HEX_GROUPS && !compressed))
}

/// `String.split(String.valueOf(c))`: trailing empty strings are dropped.
fn java_split(s: &str, c: char) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split(c).collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts == [""] && !s.is_empty() {
        parts.clear();
    }
    parts
}

/// `Integer.parseInt(s, 16)`, which accepts a leading `+` or `-`.
fn java_parse_hex(s: &str) -> Option<i64> {
    let (negative, digits) = match s.as_bytes().first()? {
        b'+' => (false, &s[1..]),
        b'-' => (true, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = i64::from_str_radix(digits, 16).ok()?;
    Some(if negative { -n } else { n })
}

/// `String.length()`: UTF-16 code units.
fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
        tlds: std::collections::BTreeMap<String, Vec<String>>,
    }

    #[derive(Deserialize)]
    struct Case {
        email: String,
        valid: bool,
    }

    fn vectors() -> Vectors {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/email_validator.json");
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn matches_commons_validator() {
        let failures: Vec<String> = vectors()
            .cases
            .into_iter()
            .filter(|c| is_valid_email(&c.email) != c.valid)
            .map(|c| format!("{:?}: expected {}", c.email, c.valid))
            .collect();
        assert!(failures.is_empty(), "{failures:#?}");
    }

    #[test]
    fn tld_lists_are_the_jvms() {
        let tlds = vectors().tlds;
        assert_eq!(tlds["GENERIC_RO"], tlds::GENERIC_TLDS);
        assert_eq!(tlds["COUNTRY_CODE_RO"], tlds::COUNTRY_CODE_TLDS);
        assert_eq!(tlds["INFRASTRUCTURE_RO"], tlds::INFRASTRUCTURE_TLDS);
        assert_eq!(tlds["LOCAL_RO"], tlds::LOCAL_TLDS);
    }

    #[test]
    fn java_helpers() {
        assert_eq!(java_split("a::b::", ':'), ["a", "", "b"]);
        assert_eq!(java_split("::1", ':'), ["", "", "1"]);
        assert_eq!(java_split("", ':'), [""]);
        assert_eq!(java_split(":::", ':'), Vec::<&str>::new());
        assert_eq!(java_parse_hex("+ff"), Some(255));
        assert_eq!(java_parse_hex("-1"), Some(-1));
        assert_eq!(java_parse_hex("+"), None);
        assert!(is_valid_inet_address("::1"));
        assert!(is_valid_inet_address("fe80::1%eth0"));
        assert!(is_valid_inet_address("::ffff:192.168.0.1"));
        assert!(is_valid_inet_address("+ff::1"));
        assert!(!is_valid_inet_address("1::2::3"));
        assert!(!is_valid_inet_address("192.168.01.1"));
    }
}
