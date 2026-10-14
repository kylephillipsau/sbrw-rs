//! A stanza as an element tree, written the way Openfire writes it
//! (`fixtures/xmpp/openfire.txt`): attributes in double quotes, `<`, `>` and
//! `&` escaped in text but not quotes, and empty elements self-closed when the
//! server built them but written out in full when they came from the client.

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// The name as written, prefix included.
    pub name: String,
    /// Every attribute as written, `xmlns` included, in order.
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// Parsed from the client: written as `<a></a>` when empty.
    parsed: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid XML: {0}")]
pub struct XmlError(String);

impl Element {
    pub fn new(name: &str) -> Self {
        Element { name: name.to_owned(), attributes: Vec::new(), children: Vec::new(), parsed: false }
    }

    pub fn attr(mut self, name: &str, value: &str) -> Self {
        self.attributes.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn child(mut self, child: Element) -> Self {
        self.children.push(Node::Element(child));
        self
    }

    pub fn text(mut self, text: &str) -> Self {
        self.children.push(Node::Text(text.to_owned()));
        self
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// The first child element with this name.
    pub fn element(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// All the text directly inside this element.
    pub fn text_content(&self) -> String {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                Node::Element(_) => None,
            })
            .collect()
    }

    /// The text of the first child element with this name.
    pub fn child_text(&self, name: &str) -> Option<String> {
        self.element(name).map(Element::text_content)
    }

    pub fn parse(xml: &str) -> Result<Element, XmlError> {
        let mut reader = quick_xml::Reader::from_str(xml);
        let mut stack: Vec<Element> = Vec::new();
        let err = |e: &dyn std::fmt::Display| XmlError(e.to_string());
        loop {
            let event = reader.read_event().map_err(|e| err(&e))?;
            let finished = match event {
                Event::Start(e) => {
                    stack.push(start(&e)?);
                    None
                }
                Event::Empty(e) => Some(start(&e)?),
                Event::End(_) => Some(stack.pop().ok_or_else(|| XmlError("unexpected end tag".into()))?),
                Event::Text(t) => {
                    push_text(&mut stack, &t.xml_content(XmlVersion::Implicit1_0));
                    None
                }
                Event::CData(t) => {
                    push_text(&mut stack, &t.xml_content(XmlVersion::Implicit1_0));
                    None
                }
                Event::GeneralRef(r) => {
                    let c = match r.resolve_char_ref().map_err(|e| err(&e))? {
                        Some(c) => c,
                        None => match r.into_inner().as_ref() {
                            "lt" => '<',
                            "gt" => '>',
                            "amp" => '&',
                            "apos" => '\'',
                            "quot" => '"',
                            other => return Err(XmlError(format!("undeclared entity &{other};"))),
                        },
                    };
                    push_text(&mut stack, c.encode_utf8(&mut [0; 4]));
                    None
                }
                Event::Eof => return Err(XmlError("unclosed element".into())),
                Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => None,
            };
            if let Some(element) = finished {
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::Element(element)),
                    None => return Ok(element),
                }
            }
        }
    }

    pub fn write(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attributes {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            escape(out, v, true);
            out.push('"');
        }
        if self.children.is_empty() && !self.parsed {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for child in &self.children {
            match child {
                Node::Element(e) => e.write(out),
                Node::Text(t) => escape(out, t, false),
            }
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push('>');
    }
}

impl std::fmt::Display for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = String::new();
        self.write(&mut out);
        f.write_str(&out)
    }
}

fn start(e: &BytesStart<'_>) -> Result<Element, XmlError> {
    let mut element = Element::new(e.name().as_ref());
    element.parsed = true;
    for attr in e.attributes() {
        let attr = attr.map_err(|e| XmlError(e.to_string()))?;
        let value = attr.normalized_value(XmlVersion::Implicit1_0).map_err(|e| XmlError(e.to_string()))?;
        element.attributes.push((attr.key.as_ref().to_owned(), value.into_owned()));
    }
    Ok(element)
}

fn push_text(stack: &mut [Element], text: &str) {
    if let Some(element) = stack.last_mut() {
        match element.children.last_mut() {
            Some(Node::Text(t)) => t.push_str(text),
            _ => element.children.push(Node::Text(text.to_owned())),
        }
    }
}

fn escape(out: &mut String, s: &str, attribute: bool) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_as_openfire_does() {
        // From fixtures/xmpp/openfire.txt: the push body's quotes come out
        // unescaped, and an empty element from the client is written in full.
        let body = Element::parse(
            "<body>&lt;Message&gt;a &quot;b&quot; &apos;c&apos; d &amp;amp; caf\u{e9}&lt;/Message&gt;\n</body>",
        )
        .unwrap();
        assert_eq!(body.to_string(), "<body>&lt;Message&gt;a \"b\" 'c' d &amp;amp; caf\u{e9}&lt;/Message&gt;\n</body>");
        let query = Element::parse("<query xmlns='jabber:iq:nonsense'/>").unwrap();
        assert_eq!(query.to_string(), r#"<query xmlns="jabber:iq:nonsense"></query>"#);
        let built = Element::new("query").attr("xmlns", "jabber:iq:auth").child(Element::new("resource"));
        assert_eq!(built.to_string(), r#"<query xmlns="jabber:iq:auth"><resource/></query>"#);
    }

    #[test]
    fn reads_names_attributes_and_text() {
        let iq = Element::parse("<iq id='a1' type='set'><query xmlns='jabber:iq:auth'><username>sbrw.100</username><resource>EA-Chat</resource></query></iq>").unwrap();
        assert_eq!(iq.attribute("type"), Some("set"));
        let query = iq.element("query").unwrap();
        assert_eq!(query.attribute("xmlns"), Some("jabber:iq:auth"));
        assert_eq!(query.child_text("username").as_deref(), Some("sbrw.100"));
        assert_eq!(query.child_text("password"), None);
    }
}
