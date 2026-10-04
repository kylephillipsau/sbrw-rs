//! Writing XML as JAXB does (pinned by `fixtures/vectors/jaxb_escaping.json`):
//! - text escapes `&`, `<`, `>` and `\r` (as `&#13;`); quotes, tabs, newlines
//!   and other control characters pass through;
//! - an empty string is `<X></X>`, an element with no content is `<X/>`;
//! - null properties are left out entirely.

use crate::{XmlType, XmlValue};

#[derive(Debug, Default)]
pub struct XmlWriter {
    out: String,
    open: Vec<Frame>,
}

#[derive(Debug)]
struct Frame {
    name: String,
    /// Attributes collected until the start tag is written.
    attributes: String,
    started: bool,
}

impl XmlWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start an element. If it is closed with nothing inside it becomes `<name/>`.
    pub fn open(&mut self, name: &str) {
        self.flush_start();
        self.open.push(Frame { name: name.to_owned(), attributes: String::new(), started: false });
    }

    /// An attribute of the element just opened.
    pub fn attribute(&mut self, name: &str, value: &str) {
        let frame = self.open.last_mut().expect("attribute outside an element");
        assert!(!frame.started, "attribute after content");
        frame.attributes.push(' ');
        frame.attributes.push_str(name);
        frame.attributes.push_str("=\"");
        for c in value.chars() {
            match c {
                '"' => frame.attributes.push_str("&quot;"),
                c => escape_char(c, &mut frame.attributes),
            }
        }
        frame.attributes.push('"');
    }

    pub fn close(&mut self) {
        let frame = self.open.pop().expect("close without open");
        if frame.started {
            self.out.push_str("</");
            self.out.push_str(&frame.name);
            self.out.push('>');
        } else {
            self.out.push('<');
            self.out.push_str(&frame.name);
            self.out.push_str(&frame.attributes);
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

    /// A simple-typed property.
    pub fn value<V: XmlValue>(&mut self, name: &str, value: &V) {
        self.text(name, &value.print());
    }

    /// A class-typed property, or a whole document's root.
    pub fn bean<T: XmlType>(&mut self, name: &str, value: &T) {
        self.open(name);
        value.write_attributes(self);
        value.write_children(self);
        self.close();
    }

    /// The finished document. Panics if an element is still open.
    pub fn finish(self) -> String {
        assert!(self.open.is_empty(), "unclosed elements: {:?}", self.open);
        self.out
    }

    fn flush_start(&mut self) {
        if let Some(frame) = self.open.last_mut()
            && !frame.started
        {
            self.out.push('<');
            self.out.push_str(&frame.name);
            self.out.push_str(&frame.attributes);
            self.out.push('>');
            frame.started = true;
        }
    }
}

/// Text content escaping, as JAXB's default escape handler does it.
pub fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        escape_char(c, out);
    }
}

fn escape_char(c: char, out: &mut String) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '\r' => out.push_str("&#13;"),
        c => out.push(c),
    }
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
            w.value("UserId", &if banned { 0i64 } else { 7 });
            w.text("LoginToken", if banned { "" } else { "token" });
            if let Some(description) = &case.description {
                w.text("Description", description);
            }
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
    fn attributes_and_nesting() {
        let mut w = XmlWriter::new();
        w.open("response");
        w.attribute("status", "1");
        w.attribute("ticket", "0");
        w.open("LobbyInvite");
        w.value("IsPrivate", &false);
        w.close();
        w.close();
        assert_eq!(
            w.finish(),
            r#"<response status="1" ticket="0"><LobbyInvite><IsPrivate>false</IsPrivate></LobbyInvite></response>"#
        );

        let mut w = XmlWriter::new();
        w.open("response");
        w.attribute("q", "a\"&<b");
        w.close();
        assert_eq!(w.finish(), r#"<response q="a&quot;&amp;&lt;b"/>"#);
    }
}
