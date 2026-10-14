//! Routing between logged-in clients: rooms, one-to-one messages and the
//! core's pushes. Each stanza is what Openfire 4.5.0 sends
//! (`fixtures/xmpp/openfire.txt`), and Openfire builds most of them from the
//! client's own stanza: a copy with `to` replaced and `from` (and `type`)
//! added after the attributes already there.

use std::collections::HashMap;

use crate::element::Element;
use crate::stream::FullJid;
use crate::subject::SBRW_SUBJECT;

const NS_MUC: &str = "http://jabber.org/protocol/muc";
const NS_MUC_USER: &str = "http://jabber.org/protocol/muc#user";
const NS_MUC_OWNER: &str = "http://jabber.org/protocol/muc#owner";
const NS_STANZAS: &str = "urn:ietf:params:xml:ns:xmpp-stanzas";

/// A stanza for one client.
#[derive(Debug, PartialEq)]
pub struct Delivery {
    pub to: String,
    pub stanza: String,
}

struct Occupant {
    nick: String,
    jid: FullJid,
    owner: bool,
    /// The presence the client joined with, less its MUC element.
    presence: Element,
}

struct Room {
    /// Semi-anonymous: other occupants' real JIDs are hidden.
    anonymous: bool,
    persistent: bool,
    /// Created by joining and not yet configured.
    locked: bool,
    occupants: Vec<Occupant>,
}

pub struct Hub {
    domain: String,
    conference: String,
    sessions: Vec<FullJid>,
    rooms: HashMap<String, Room>,
    new_id: Box<dyn FnMut() -> String + Send>,
}

impl Hub {
    /// `new_id` makes the ids of room subjects and `stanza-id`s.
    pub fn new(domain: &str, new_id: Box<dyn FnMut() -> String + Send>) -> Self {
        Hub {
            domain: domain.to_owned(),
            conference: format!("conference.{domain}"),
            sessions: Vec::new(),
            rooms: HashMap::new(),
            new_id,
        }
    }

    /// A public room, as the core creates them at startup: persistent and
    /// semi-anonymous (`OpenFireRestApiCli.createGeneralChatRoom`).
    pub fn create_public_room(&mut self, name: &str) {
        let room = Room { anonymous: true, persistent: true, locked: false, occupants: Vec::new() };
        self.rooms.insert(name.to_lowercase(), room);
    }

    /// A client has logged in. True if it replaced a session with the same
    /// address, which the caller ends with a `conflict` stream error.
    pub fn connect(&mut self, jid: &FullJid) -> bool {
        let replaced = self.sessions.contains(jid);
        if !replaced {
            self.sessions.push(jid.clone());
        }
        replaced
    }

    /// The core's push to a persona, as `OpenFireConnector.send` sends it
    /// from the engine's account and Openfire delivers it.
    pub fn push(&self, persona_id: i64, body: &str) -> Vec<Delivery> {
        let node = format!("sbrw.{persona_id}");
        let to = format!("{node}@{}", self.domain);
        let message = Element::new("message")
            .attr("to", &to)
            .attr("id", "JN_1234567")
            .attr("from", &format!("sbrw.engine.engine@{}/EA_Chat", self.domain))
            .child(Element::new("subject").text(SBRW_SUBJECT))
            .child(Element::new("body").text(body));
        self.sessions.iter().filter(|s| s.node == node).map(|s| as_sent(s, &message)).collect()
    }

    /// Route a stanza the client's stream didn't answer itself.
    pub fn route(&mut self, from: &FullJid, stanza: Element) -> Vec<Delivery> {
        let Some(to) = stanza.attribute("to").map(str::to_owned) else { return Vec::new() };
        let (bare, resource) = to.split_once('/').map_or((to.as_str(), None), |(b, r)| (b, Some(r)));
        let Some((node, domain)) = bare.split_once('@') else { return Vec::new() };
        if domain == self.conference {
            let room = node.to_lowercase();
            return match (stanza.name.as_str(), resource) {
                ("presence", Some(nick)) if stanza.attribute("type") == Some("unavailable") => {
                    self.leave(from, &room, nick, stanza)
                }
                ("presence", Some(nick)) if stanza.attribute("type").is_none() => self.join(from, &room, nick, stanza),
                ("message", None) if stanza.attribute("type") == Some("groupchat") => {
                    self.groupchat(from, &room, stanza)
                }
                ("iq", None) => self.owner_iq(from, &room, stanza),
                _ => Vec::new(),
            };
        }
        if domain == self.domain && stanza.name == "message" {
            let mut message = stanza;
            message.set_attr("from", &from.to_string());
            return self
                .sessions
                .iter()
                .filter(|s| s.node == node && resource.is_none_or(|r| r == s.resource))
                .map(|s| as_sent(s, &message))
                .collect();
        }
        Vec::new()
    }

    fn room_jid(&self, room: &str) -> String {
        format!("{room}@{}", self.conference)
    }

    fn join(&mut self, from: &FullJid, room_name: &str, nick: &str, stanza: Element) -> Vec<Delivery> {
        let room_jid = self.room_jid(room_name);
        let occupant_jid = format!("{room_jid}/{nick}");
        let creating = !self.rooms.contains_key(room_name);
        if creating {
            // Openfire's defaults for a room made by joining it.
            let room = Room { anonymous: false, persistent: false, locked: true, occupants: Vec::new() };
            self.rooms.insert(room_name.to_owned(), room);
        }
        let room = self.rooms.get_mut(room_name).expect("just ensured");
        if room.locked && !creating {
            let mut error = stanza;
            error.set_attr("to", &from.to_string());
            error.set_attr("from", &occupant_jid);
            error.set_attr("type", "error");
            error.push(stanza_error("404", "cancel", "item-not-found"));
            return vec![deliver(from, &error)];
        }

        let mut presence = stanza;
        presence.remove("x", NS_MUC);
        let new = Occupant { nick: nick.to_owned(), jid: from.clone(), owner: creating, presence };
        let mut out = Vec::new();
        for other in &room.occupants {
            out.push(deliver(from, &occupant_presence(&room_jid, room, other, from, &[])));
        }
        let mut statuses = vec!["110"];
        if !room.anonymous {
            statuses.push("100");
        }
        if creating {
            statuses.push("201");
        }
        out.push(deliver(from, &occupant_presence(&room_jid, room, &new, from, &statuses)));
        let subject = Element::new("message")
            .attr("from", &room_jid)
            .attr("type", "groupchat")
            .attr("id", &(self.new_id)())
            .attr("to", &from.to_string())
            .child(Element::new("subject"));
        out.push(Delivery { to: from.to_string(), stanza: subject.to_string() });
        for other in &room.occupants {
            out.push(deliver(&other.jid, &occupant_presence(&room_jid, room, &new, &other.jid, &[])));
        }
        room.occupants.push(new);
        out
    }

    fn leave(&mut self, from: &FullJid, room_name: &str, nick: &str, stanza: Element) -> Vec<Delivery> {
        let room_jid = self.room_jid(room_name);
        let Some(room) = self.rooms.get_mut(room_name) else { return Vec::new() };
        let Some(i) = room.occupants.iter().position(|o| o.jid == *from && o.nick == nick) else { return Vec::new() };
        let mut leaving = room.occupants.remove(i);
        leaving.presence = stanza;
        let mut out = vec![deliver(from, &unavailable(&room_jid, room, &leaving, from, &["110"]))];
        for other in &room.occupants {
            out.push(deliver(&other.jid, &unavailable(&room_jid, room, &leaving, &other.jid, &[])));
        }
        if room.occupants.is_empty() && !room.persistent {
            self.rooms.remove(room_name);
        }
        out
    }

    fn groupchat(&mut self, from: &FullJid, room_name: &str, stanza: Element) -> Vec<Delivery> {
        let room_jid = self.room_jid(room_name);
        let Some(room) = self.rooms.get(room_name) else { return Vec::new() };
        let Some(sender) = room.occupants.iter().find(|o| o.jid == *from) else { return Vec::new() };
        let mut message = stanza;
        message.set_attr("from", &format!("{room_jid}/{}", sender.nick));
        let id = (self.new_id)();
        message.push(Element::new("stanza-id").attr("xmlns", "urn:xmpp:sid:0").attr("id", &id).attr("by", &room_jid));
        room.occupants.iter().map(|o| deliver(&o.jid, &message)).collect()
    }

    /// An owner submitting the room's configuration form; an empty one makes
    /// an instant room (XEP-0045 §10.1.2), which unlocks it.
    fn owner_iq(&mut self, from: &FullJid, room_name: &str, stanza: Element) -> Vec<Delivery> {
        let room_jid = self.room_jid(room_name);
        let Some(room) = self.rooms.get_mut(room_name) else { return Vec::new() };
        let is_owner = room.occupants.iter().any(|o| o.jid == *from && o.owner);
        let submits = stanza.attribute("type") == Some("set")
            && stanza.element("query").and_then(|q| q.attribute("xmlns")) == Some(NS_MUC_OWNER);
        if !is_owner || !submits {
            return Vec::new();
        }
        room.locked = false;
        let mut result = Element::new("iq").attr("type", "result");
        if let Some(id) = stanza.attribute("id") {
            result = result.attr("id", id);
        }
        let result = result.attr("from", &room_jid).attr("to", &from.to_string());
        vec![deliver(from, &result)]
    }
}

/// A stanza for `to` with its `to` left as the sender wrote it.
fn as_sent(to: &FullJid, stanza: &Element) -> Delivery {
    Delivery { to: to.to_string(), stanza: stanza.to_string() }
}

/// A stanza for `to`, addressed to it.
fn deliver(to: &FullJid, stanza: &Element) -> Delivery {
    let mut stanza = stanza.clone();
    stanza.set_attr("to", &to.to_string());
    Delivery { to: to.to_string(), stanza: stanza.to_string() }
}

/// An occupant's presence as `viewer` sees it. Real JIDs show in a
/// non-anonymous room; XEP-0045 also shows them to moderators of a
/// semi-anonymous one, which no recording covers yet.
fn occupant_presence(room_jid: &str, room: &Room, occupant: &Occupant, viewer: &FullJid, statuses: &[&str]) -> Element {
    let (affiliation, role) = if occupant.owner { ("owner", "moderator") } else { ("none", "participant") };
    let mut presence = occupant.presence.clone();
    presence.set_attr("to", &viewer.to_string());
    presence.set_attr("from", &format!("{room_jid}/{}", occupant.nick));
    presence.push(muc_user(room, occupant, affiliation, role, statuses));
    presence
}

fn unavailable(room_jid: &str, room: &Room, occupant: &Occupant, viewer: &FullJid, statuses: &[&str]) -> Element {
    let affiliation = if occupant.owner { "owner" } else { "none" };
    let mut presence = occupant.presence.clone();
    presence.set_attr("to", &viewer.to_string());
    presence.set_attr("from", &format!("{room_jid}/{}", occupant.nick));
    presence.push(muc_user(room, occupant, affiliation, "none", statuses));
    presence
}

fn muc_user(room: &Room, occupant: &Occupant, affiliation: &str, role: &str, statuses: &[&str]) -> Element {
    let mut item = Element::new("item");
    if !room.anonymous {
        item = item.attr("jid", &occupant.jid.to_string());
    }
    let mut x =
        Element::new("x").attr("xmlns", NS_MUC_USER).child(item.attr("affiliation", affiliation).attr("role", role));
    for code in statuses {
        x = x.child(Element::new("status").attr("code", code));
    }
    x
}

fn stanza_error(code: &str, kind: &str, condition: &str) -> Element {
    Element::new("error").attr("code", code).attr("type", kind).child(Element::new(condition).attr("xmlns", NS_STANZAS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framer::Framer;
    use crate::stream::{Accounts, Out, Stream};

    struct Players;

    impl Accounts for Players {
        fn password(&self, username: &str) -> Option<String> {
            // The fixture's engine account logs in like a player; in sbrw-rs the core pushes directly.
            Some(if username == "sbrw.engine.engine" { "sbrw-openfire-token" } else { "0123456789abcdef" }.to_owned())
        }
    }

    /// Openfire's output less what the fork or sbrw-rs leaves out.
    fn expected(openfire: &str) -> String {
        let drop = [
            r"<mechanisms .*?</mechanisms>",
            r#"<c xmlns="http://jabber.org/protocol/caps"[^>]*/>"#,
            r#"<iq type="get" id="server\d+" to="[^"]*" from="[^"]*"><query xmlns="jabber:iq:version"/></iq>"#,
        ];
        drop.iter()
            .fold(openfire.to_owned(), |text, re| regex::Regex::new(re).unwrap().replace_all(&text, "").into_owned())
    }

    struct Client {
        stream: Stream,
        framer: Framer,
        received: String,
    }

    #[test]
    fn rooms_chat_and_pushes_as_openfire_does() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/xmpp/openfire.txt");
        let text = std::fs::read_to_string(path).unwrap();
        let mut ids = 0;
        let mut hub = Hub::new(
            "127.0.0.1",
            Box::new(move || {
                ids += 1;
                format!("uuid{ids}")
            }),
        );
        hub.create_public_room("channel.EN__1");
        hub.create_public_room("channel.EN__2");
        let mut clients: HashMap<&str, Client> = HashMap::new();
        let mut expected_by_client: HashMap<&str, String> = HashMap::new();
        let section =
            text.split("# A joins the public room").nth(1).unwrap().split("# A logs in again").next().unwrap();
        // A's stream is already logged in; B's and the engine's log in here.
        let mut a = Stream::new("127.0.0.1", "stream1");
        let login = "<iq id='auth2' type='set'><query xmlns='jabber:iq:auth'><username>sbrw.100</username><password>0123456789abcdef</password><resource>EA-Chat</resource></query></iq>";
        a.tls_started();
        a.handle(crate::framer::Frame::Stanza(login.into()), &Players).unwrap();
        hub.connect(a.jid().unwrap());
        clients.insert("A", Client { stream: a, framer: Framer::default(), received: String::new() });
        let mut next_stream = 2;

        for line in section.lines() {
            let Some((who, rest)) = line.split_once(' ') else { continue };
            let (dir, text) = rest.split_at(1);
            let text = text.trim_start().replace("\\n", "\n");
            match dir {
                "<" => expected_by_client.entry(who).or_default().push_str(&expected(&text)),
                // The engine's message is the core's send, which sbrw-rs makes with push.
                ">" if who == "E" && text.starts_with("<message") => {
                    let body = Element::parse(&text).unwrap().child_text("body").unwrap();
                    for d in hub.push(100, &body) {
                        let client = clients
                            .values_mut()
                            .find(|c| c.stream.jid().is_some_and(|j| j.to_string() == d.to))
                            .unwrap();
                        client.received.push_str(&d.stanza);
                    }
                }
                ">" => {
                    let client = clients.entry(who).or_insert_with(|| {
                        next_stream += 1;
                        Client {
                            stream: Stream::new("127.0.0.1", &format!("stream{}", next_stream - 1)),
                            framer: Framer::default(),
                            received: String::new(),
                        }
                    });
                    client.framer.push(text.as_bytes());
                    let mut deliveries = Vec::new();
                    while let Some(frame) = client.framer.next_frame().unwrap() {
                        let was_logged_in = client.stream.jid().is_some();
                        match client.stream.handle(frame, &Players) {
                            Ok(outs) => {
                                for out in outs {
                                    match out {
                                        Out::Send(s) => client.received.push_str(&s),
                                        Out::StartTls => client.stream.tls_started(),
                                        Out::Close => panic!("{who} closed"),
                                    }
                                }
                            }
                            Err(stanza) => {
                                let from = client.stream.jid().expect("routed only after login").clone();
                                deliveries.extend(hub.route(&from, stanza));
                            }
                        }
                        if !was_logged_in && let Some(jid) = client.stream.jid() {
                            hub.connect(jid);
                        }
                    }
                    for d in deliveries {
                        let client = clients
                            .values_mut()
                            .find(|c| c.stream.jid().is_some_and(|j| j.to_string() == d.to))
                            .unwrap();
                        client.received.push_str(&d.stanza);
                    }
                }
                _ => {}
            }
        }
        for who in ["A", "B", "E"] {
            assert_eq!(clients[who].received, expected_by_client[who], "what {who} received");
        }
    }
}
