//! XML written exactly as soapbox-race-core's JAXB marshaller writes it.
//!
//! Upstream marshals with `JAXB_FRAGMENT` and no formatting (`JAXBUtility`):
//! no XML declaration, no namespaces, no whitespace between elements. The
//! details below are pinned by `fixtures/vectors/jaxb_escaping.json`:
//! - text escapes `&`, `<`, `>` and `\r` (as `&#13;`); quotes, tabs, newlines
//!   and other control characters pass through;
//! - an empty string is `<X></X>`, an element with no content is `<X/>`;
//! - null properties are left out entirely.

use std::fmt::Display;

/// Builds one XML document element by element.
#[derive(Debug, Default)]
pub struct XmlWriter {
    out: String,
    /// Open elements, and whether each one has written its start tag yet.
    open: Vec<(String, bool)>,
}

impl XmlWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start an element. If it is closed with nothing inside it becomes `<name/>`.
    pub fn open(&mut self, name: &str) {
        self.flush_start();
        self.open.push((name.to_owned(), false));
    }

    pub fn close(&mut self) {
        let (name, started) = self.open.pop().expect("close without open");
        if started {
            self.out.push_str("</");
            self.out.push_str(&name);
            self.out.push('>');
        } else {
            self.out.push('<');
            self.out.push_str(&name);
            self.out.push_str("/>");
        }
    }

    /// `<name>text</name>`, including when `text` is empty.
    pub fn text(&mut self, name: &str, text: &str) {
        self.open(name);
        self.flush_start();
        escape_text(text, &mut self.out);
        self.close();
    }

    /// A boxed (nullable) property: written only when present.
    pub fn opt_text(&mut self, name: &str, text: Option<&str>) {
        if let Some(text) = text {
            self.text(name, text);
        }
    }

    /// A number or boolean property, formatted with `Display`.
    pub fn value(&mut self, name: &str, value: impl Display) {
        self.text(name, &value.to_string());
    }

    pub fn opt_value<T: Display>(&mut self, name: &str, value: Option<T>) {
        if let Some(value) = value {
            self.value(name, value);
        }
    }

    /// The finished document. Panics if an element is still open.
    pub fn finish(self) -> String {
        assert!(self.open.is_empty(), "unclosed elements: {:?}", self.open);
        self.out
    }

    fn flush_start(&mut self) {
        if let Some((name, started)) = self.open.last_mut()
            && !*started
        {
            self.out.push('<');
            self.out.push_str(name);
            self.out.push('>');
            *started = true;
        }
    }
}

/// Text content escaping, as JAXB's default escape handler does it.
pub fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

/// A type that marshals to a complete XML document.
pub trait ToXml {
    fn to_xml(&self) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        description: Option<String>,
        ban: Option<(String, String)>,
        #[serde(default)]
        nulls: bool,
        xml: String,
    }

    /// The vectors marshal upstream's LoginStatusVO; build the same document.
    fn login_status(case: &Case) -> String {
        let mut w = XmlWriter::new();
        w.open("LoginStatusVO");
        if !case.nulls {
            let banned = case.ban.is_some();
            w.value("UserId", if banned { 0 } else { 7 });
            w.text("LoginToken", if banned { "" } else { "token" });
            w.opt_text("Description", case.description.as_deref());
            if let Some((reason, expires)) = &case.ban {
                w.open("Ban");
                w.text("Reason", reason);
                w.text("Expires", expires);
                w.close();
            }
        }
        w.close();
        w.finish()
    }

    #[test]
    fn matches_jaxb() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/jaxb_escaping.json");
        let vectors: Vectors = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(vectors.cases.len() > 5);
        for case in &vectors.cases {
            assert_eq!(login_status(case), case.xml, "{:?}", case.description);
        }
    }

    #[test]
    fn nesting() {
        let mut w = XmlWriter::new();
        w.open("UserInfo");
        w.value("defaultPersonaIdx", 0);
        w.open("personas");
        w.close();
        w.open("user");
        w.value("fullGameAccess", false);
        w.close();
        w.close();
        assert_eq!(
            w.finish(),
            "<UserInfo><defaultPersonaIdx>0</defaultPersonaIdx><personas/><user><fullGameAccess>false</fullGameAccess></user></UserInfo>"
        );
    }
}
