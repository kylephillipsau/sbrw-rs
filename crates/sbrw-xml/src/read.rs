//! Reading XML the way `JAXBUtility.unMarshal` does (pinned by
//! `fixtures/dto/unmarshal.json`):
//! - namespaces and prefixes are ignored, names are local names
//!   (upstream wraps the reader in `XMLReaderWithoutNamespace`), so `xsi:nil`
//!   is just an unknown attribute;
//! - an element's text is the text after its last child element (text before
//!   or between children is dropped);
//! - a byte-order mark, a truncated document or no document at all is an error.

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

#[derive(Debug, thiserror::Error)]
pub enum XmlError {
    #[error("invalid XML: {0}")]
    Syntax(String),
    #[error("no root element")]
    NoRoot,
}

/// One element: local name, attributes by local name, child elements, text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Element>,
    text: String,
}

impl Element {
    /// The text after the last child element, as JAXB reads a simple value.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Child elements with this local name, in document order.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

fn start(e: &BytesStart<'_>) -> Result<Element, XmlError> {
    let name = e.local_name().as_ref().to_owned();
    let mut attributes = Vec::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| XmlError::Syntax(err.to_string()))?;
        let key = attr.key;
        if key.as_ref() == "xmlns" || key.prefix().is_some_and(|p| p.as_ref() == "xmlns") {
            continue;
        }
        let value = attr.normalized_value(XmlVersion::Implicit1_0).map_err(|err| XmlError::Syntax(err.to_string()))?;
        attributes.push((key.local_name().as_ref().to_owned(), value.into_owned()));
    }
    Ok(Element { name, attributes, children: Vec::new(), text: String::new() })
}

/// Parse a document and return its root element.
pub fn parse(xml: &str) -> Result<Element, XmlError> {
    // Upstream decodes the body into a String, so a byte-order mark is a
    // character before the root element, which its parser rejects.
    if xml.starts_with('\u{feff}') {
        return Err(XmlError::Syntax("content before the root element".into()));
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    let syntax = |err: quick_xml::Error| XmlError::Syntax(err.to_string());
    loop {
        let event = reader.read_event().map_err(syntax)?;
        match event {
            Event::Start(e) => {
                if root.is_some() {
                    return Err(XmlError::Syntax("content after the root element".into()));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.text.clear();
                }
                stack.push(start(&e)?);
            }
            Event::Empty(e) => {
                if root.is_some() {
                    return Err(XmlError::Syntax("content after the root element".into()));
                }
                let element = start(&e)?;
                match stack.last_mut() {
                    Some(parent) => {
                        parent.text.clear();
                        parent.children.push(element);
                    }
                    None => root = Some(element),
                }
            }
            Event::End(_) => {
                let element = stack.pop().ok_or_else(|| XmlError::Syntax("unexpected end tag".into()))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(element),
                    None => root = Some(element),
                }
            }
            Event::Text(t) => {
                let text = t.xml_content(XmlVersion::Implicit1_0);
                match stack.last_mut() {
                    Some(element) => element.text.push_str(&text),
                    None if text.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) => {}
                    None => return Err(XmlError::Syntax("text outside the root element".into())),
                }
            }
            Event::CData(t) => {
                let text = t.xml_content(XmlVersion::Implicit1_0);
                match stack.last_mut() {
                    Some(element) => element.text.push_str(&text),
                    None => return Err(XmlError::Syntax("CDATA outside the root element".into())),
                }
            }
            Event::GeneralRef(r) => {
                let resolved = match r.resolve_char_ref().map_err(syntax)? {
                    Some(c) => c,
                    None => match r.into_inner().as_ref() {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "apos" => '\'',
                        "quot" => '"',
                        other => return Err(XmlError::Syntax(format!("undeclared entity &{other};"))),
                    },
                };
                match stack.last_mut() {
                    Some(element) => element.text.push(resolved),
                    None => return Err(XmlError::Syntax("reference outside the root element".into())),
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(XmlError::Syntax("unclosed element at end of input".into()));
    }
    root.ok_or(XmlError::NoRoot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_local_and_text_follows_the_last_child() {
        let e = parse(r#"<a:Root xmlns:a="urn:x" xmlns:i="urn:i" i:nil="true"><a:X>1 &amp; &#x41;</a:X><Y>a<b>bold</b>c</Y></a:Root>"#).unwrap();
        assert_eq!(e.name, "Root");
        assert_eq!(e.attribute("nil"), Some("true"));
        assert_eq!(e.children[0].name, "X");
        assert_eq!(e.children[0].text(), "1 & A");
        assert_eq!(e.children[1].text(), "c");
    }

    #[test]
    fn rejects_what_jaxb_rejects() {
        assert!(parse("\u{feff}<X/>").is_err());
        assert!(parse("<X><Y>1</Y>").is_err());
        assert!(parse("").is_err());
        assert!(parse("<?xml version=\"1.0\"?><X/>").is_ok());
    }
}
