//! One client's stream up to and including login: the stream header,
//! STARTTLS, `jabber:iq:auth`, and the replies that need no other session.
//! Each reply is what Openfire 4.5.0 with the nonSASL plugin sends
//! (`fixtures/xmpp/openfire.txt`), less the SASL mechanisms the fork hides
//! from the game and the entity-caps hash. See `docs/protocol/xmpp.md`.

use sha1::{Digest, Sha1};

use crate::element::Element;
use crate::framer::Frame;

const NS_TLS: &str = "urn:ietf:params:xml:ns:xmpp-tls";
const NS_AUTH: &str = "jabber:iq:auth";
const NS_STANZAS: &str = "urn:ietf:params:xml:ns:xmpp-stanzas";

/// Who may log in, and with what.
pub trait Accounts {
    /// The XMPP password for this username, if it has one.
    fn password(&self, username: &str) -> Option<String>;
}

#[derive(Debug, PartialEq)]
pub enum Out {
    Send(String),
    /// Switch the connection to TLS before reading on.
    StartTls,
    /// The client sent its initial presence: it can now be sent stored messages.
    Available,
    Close,
}

/// A logged-in client's address.
#[derive(Debug, Clone, PartialEq)]
pub struct FullJid {
    pub node: String,
    pub domain: String,
    pub resource: String,
}

impl std::fmt::Display for FullJid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}/{}", self.node, self.domain, self.resource)
    }
}

pub struct Stream {
    domain: String,
    id: String,
    tls: bool,
    jid: Option<FullJid>,
}

impl Stream {
    pub fn new(domain: &str, id: &str) -> Self {
        Stream { domain: domain.to_owned(), id: id.to_owned(), tls: false, jid: None }
    }

    /// The logged-in address, once iq:auth has succeeded.
    pub fn jid(&self) -> Option<&FullJid> {
        self.jid.as_ref()
    }

    /// The connection is now TLS.
    pub fn tls_started(&mut self) {
        self.tls = true;
    }

    /// Handle one frame. A stanza this doesn't answer comes back as `Err` for
    /// the caller to route.
    pub fn handle(&mut self, frame: Frame, accounts: &dyn Accounts) -> Result<Vec<Out>, Element> {
        match frame {
            Frame::StreamOpen(_) => Ok(vec![Out::Send(self.header())]),
            Frame::StreamClose => Ok(vec![Out::Send("</stream:stream>".into()), Out::Close]),
            Frame::Stanza(xml) => {
                let Ok(stanza) = Element::parse(&xml) else { return Ok(vec![Out::Close]) };
                self.stanza(stanza, accounts)
            }
        }
    }

    fn header(&self) -> String {
        let mut features = String::new();
        if !self.tls {
            features.push_str(&format!("<starttls xmlns=\"{NS_TLS}\"></starttls>"));
        }
        features.push_str(
            "<ver xmlns=\"urn:xmpp:features:rosterver\"/><auth xmlns=\"http://jabber.org/features/iq-auth\"/>",
        );
        format!(
            "<?xml version='1.0' encoding='UTF-8'?><stream:stream xmlns:stream=\"http://etherx.jabber.org/streams\" \
             xmlns=\"jabber:client\" from=\"{}\" id=\"{}\" xml:lang=\"en\" version=\"1.0\"><stream:features>{features}</stream:features>",
            self.domain, self.id
        )
    }

    fn stanza(&mut self, stanza: Element, accounts: &dyn Accounts) -> Result<Vec<Out>, Element> {
        let send = |e: Element| Ok(vec![Out::Send(e.to_string())]);
        if stanza.name == "starttls" && !self.tls {
            return Ok(vec![Out::Send(format!("<proceed xmlns=\"{NS_TLS}\"/>")), Out::StartTls]);
        }
        if stanza.name == "iq" {
            let query = stanza.elements().next();
            if query.and_then(|q| q.attribute("xmlns")) == Some(NS_AUTH) {
                return send(self.iq_auth(&stanza, accounts));
            }
            let Some(jid) = &self.jid else { return Ok(vec![]) };
            let to = stanza.attribute("to");
            if to.is_none() || to == Some(self.domain.as_str()) {
                // Nothing on the server itself answers any other namespace.
                return send(error_reply(
                    &stanza,
                    Some(&self.domain),
                    &jid.to_string(),
                    "501",
                    "cancel",
                    "feature-not-implemented",
                ));
            }
        }
        if stanza.name == "presence"
            && stanza.attributes.is_empty()
            && let Some(jid) = &self.jid
        {
            let jid = jid.to_string();
            let echo = Element::new("presence").attr("from", &jid).attr("to", &jid);
            return Ok(vec![Out::Send(echo.to_string()), Out::Available]);
        }
        Err(stanza)
    }

    /// The nonSASL plugin's `IQAuthHandler`, as Openfire 4.5.0 runs it.
    fn iq_auth(&mut self, iq: &Element, accounts: &dyn Accounts) -> Element {
        let id = iq.attribute("id").unwrap_or_default();
        let query = iq.elements().next().expect("checked by the caller");
        let username = query.child_text("username");
        if iq.attribute("type") == Some("get") {
            let mut user = Element::new("username");
            if let Some(username) = &username {
                user = user.text(username);
            }
            let fields = Element::new("query")
                .attr("xmlns", NS_AUTH)
                .child(user)
                .child(Element::new("password"))
                .child(Element::new("digest"))
                .child(Element::new("resource"));
            return Element::new("iq").attr("type", "result").attr("id", id).child(fields);
        }
        // Once logged in, Openfire answers any further set with success.
        if let Some(jid) = &self.jid {
            return Element::new("iq").attr("type", "result").attr("id", id).attr("to", &jid.to_string());
        }
        let not_authorized =
            || error_reply(iq, None, &format!("{}/{}", self.domain, self.id), "401", "auth", "not-authorized");
        let Some(username) = username.map(|u| u.to_lowercase()) else { return not_authorized() };
        let resource = query.child_text("resource").unwrap_or_default();
        if resource.is_empty() {
            // nonSASL answers not-acceptable; not yet recorded.
            return error_reply(iq, None, &format!("{}/{}", self.domain, self.id), "406", "modify", "not-acceptable");
        }
        let Some(expected) = accounts.password(&username) else { return not_authorized() };
        let ok = match (query.child_text("password"), query.child_text("digest")) {
            (Some(password), _) => password == expected,
            (None, Some(digest)) => {
                digest.to_lowercase() == hex::encode(Sha1::digest(format!("{}{expected}", self.id)))
            }
            (None, None) => false,
        };
        if !ok {
            return not_authorized();
        }
        let jid = FullJid { node: username, domain: self.domain.clone(), resource };
        let reply = Element::new("iq").attr("type", "result").attr("id", id).attr("to", &jid.to_string());
        self.jid = Some(jid);
        reply
    }
}

/// An error reply that echoes the request's payload, as Openfire's do.
fn error_reply(request: &Element, from: Option<&str>, to: &str, code: &str, kind: &str, condition: &str) -> Element {
    let mut reply = Element::new(&request.name).attr("type", "error");
    if let Some(id) = request.attribute("id") {
        reply = reply.attr("id", id);
    }
    if let Some(from) = from {
        reply = reply.attr("from", from);
    }
    reply = reply.attr("to", to);
    for child in request.elements() {
        reply = reply.child(child.clone());
    }
    let error = Element::new("error")
        .attr("code", code)
        .attr("type", kind)
        .child(Element::new(condition).attr("xmlns", NS_STANZAS));
    reply.child(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framer::Framer;

    struct Players;

    impl Accounts for Players {
        fn password(&self, username: &str) -> Option<String> {
            username.starts_with("sbrw.").then(|| "0123456789abcdef".to_owned())
        }
    }

    /// The fixture's lines for one client, up to the first line starting with `until`.
    fn fixture(client: &str, until: &str) -> Vec<(char, String)> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/xmpp/openfire.txt");
        let text = std::fs::read_to_string(path).unwrap();
        let mut lines = Vec::new();
        for line in text.lines() {
            if line.starts_with(until) {
                break;
            }
            for dir in ['>', '<'] {
                if let Some(rest) = line.strip_prefix(&format!("{client} {dir} ")) {
                    lines.push((dir, rest.replace("\\n", "\n")));
                }
            }
        }
        lines
    }

    /// Openfire's output less what the fork or sbrw-rs leaves out.
    fn expected(openfire: &str) -> String {
        let mechanisms = regex::Regex::new(r"<mechanisms .*?</mechanisms>").unwrap();
        let caps = regex::Regex::new(r#"<c xmlns="http://jabber.org/protocol/caps"[^>]*/>"#).unwrap();
        caps.replace_all(&mechanisms.replace_all(openfire, ""), "").into_owned()
    }

    #[test]
    fn logs_in_as_openfire_does() {
        let lines = fixture("A", "# A joins the public room");
        assert!(lines.len() > 10);
        let mut stream = Stream::new("127.0.0.1", "stream1");
        let mut framer = Framer::default();
        let (mut sent, mut received) = (String::new(), String::new());
        for (dir, text) in lines {
            if dir == '<' {
                received.push_str(&expected(&text));
                continue;
            }
            framer.push(text.as_bytes());
            while let Some(frame) = framer.next_frame().unwrap() {
                for out in stream.handle(frame, &Players).unwrap_or_else(|s| panic!("unanswered: {s}")) {
                    match out {
                        Out::Send(s) => sent.push_str(&s),
                        Out::StartTls => stream.tls_started(),
                        Out::Available => {}
                        Out::Close => panic!("closed"),
                    }
                }
            }
        }
        assert_eq!(sent, received);
        assert_eq!(stream.jid().map(ToString::to_string).as_deref(), Some("sbrw.100@127.0.0.1/EA-Chat"));
    }

    #[test]
    fn accepts_a_digest_of_the_stream_id_and_password() {
        let mut stream = Stream::new("127.0.0.1", "abc");
        let digest = hex::encode(Sha1::digest("abc0123456789abcdef"));
        let iq = format!(
            "<iq id='1' type='set'><query xmlns='jabber:iq:auth'><username>sbrw.7</username><digest>{digest}</digest><resource>EA-Chat</resource></query></iq>"
        );
        let out = stream.handle(Frame::Stanza(iq), &Players).unwrap();
        assert_eq!(out, [Out::Send(r#"<iq type="result" id="1" to="sbrw.7@127.0.0.1/EA-Chat"/>"#.into())]);
    }
}
