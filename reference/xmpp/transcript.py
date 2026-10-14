#!/usr/bin/env python3
"""Drive the reference Openfire as SBRW's core and game did; print the transcript.

    python3 reference/xmpp/transcript.py HOST XMPP_PORT REST_PORT > fixtures/xmpp/openfire.txt

Setup goes through the REST plugin with the bodies the core sends
(`OpenFireRestApiCli`). Then two players log in over TLS 1.0 as the game does,
join a public room and chat, the engine account sends a push, and a second
login with the same resource kicks the first.

Each line is `<who> > <text>` for what a client sent, `<who> < <text>` for
what it received, and `REST ...` for setup calls. Stream ids and generated
stanza ids are numbered in order of appearance, so runs compare with diff.
"""
import re
import socket
import ssl
import sys
import urllib.request

HOST, PORT, REST_PORT = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
DOMAIN = "127.0.0.1"
TOKEN = "sbrw-openfire-token"
PASSWORD = "0123456789abcdef"  # the first 16 characters of a session token

ids = {}


def normalise(text):
    def stable(kind):
        def sub(m):
            key = (kind, m.group(2))
            if key not in ids:
                ids[key] = f"{kind}{sum(1 for k in ids if k[0] == kind) + 1}"
            return f'{m.group(1)}"{ids[key]}"'
        return sub
    text = re.sub(r'(<stream:stream[^>]* id=)"([a-z0-9]+)"', stable("stream"), text)
    text = re.sub(r'( to="[^"/]*/)([a-z0-9]{8,10})"', lambda m: m.group(1) + ids.get(("stream", m.group(2)), m.group(2)) + '"', text)
    text = re.sub(r'( id=)"([0-9a-f]{8}-[0-9a-f-]{27})"', stable("uuid"), text)
    text = re.sub(r'( id=)"([0-9]+-[0-9]+)"', stable("server"), text)
    return text


def rest(method, path, body=None):
    req = urllib.request.Request(f"http://{HOST}:{REST_PORT}/plugins/restapi/v1/{path}", method=method,
                                 data=body.encode() if body else None,
                                 headers={"Authorization": TOKEN, "Content-Type": "application/xml"})
    with urllib.request.urlopen(req) as resp:
        print(f"REST {method} {path} {body or ''} -> {resp.status}")


class Conn:
    def __init__(self, name):
        self.name = name
        self.sock = socket.create_connection((HOST, PORT), timeout=5)

    def send(self, text):
        print(f"{self.name} > {text}")
        self.sock.sendall(text.encode())

    def recv(self, wait=0.6):
        """Everything the server sends within `wait` seconds."""
        self.sock.settimeout(wait)
        data = b""
        try:
            while True:
                chunk = self.sock.recv(65536)
                if not chunk:
                    break
                data += chunk
        except (socket.timeout, ssl.SSLWantReadError):
            pass
        text = normalise(data.decode())
        if text:
            print(f"{self.name} < {text}")
        return text

    def starttls(self):
        ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
        ctx.minimum_version = ssl.TLSVersion.TLSv1
        ctx.maximum_version = ssl.TLSVersion.TLSv1
        ctx.set_ciphers("DEFAULT:@SECLEVEL=0")
        self.sock = ctx.wrap_socket(self.sock)
        print(f"{self.name} = TLS {self.sock.version()} {self.sock.cipher()[0]}")

    def open_stream(self):
        self.send(f"<?xml version='1.0' ?><stream:stream to='{DOMAIN}' xmlns='jabber:client' "
                  "xmlns:stream='http://etherx.jabber.org/streams' version='1.0'>")
        return self.recv()


def login(name, user, password, resource="EA-Chat"):
    c = Conn(name)
    c.open_stream()
    c.send("<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>")
    c.recv()
    c.starttls()
    c.open_stream()
    c.send(f"<iq id='auth1' type='get'><query xmlns='jabber:iq:auth'><username>{user}</username></query></iq>")
    c.recv()
    c.send(f"<iq id='auth2' type='set'><query xmlns='jabber:iq:auth'><username>{user}</username>"
           f"<password>{password}</password><resource>{resource}</resource></query></iq>")
    c.recv()
    return c


def chat_room(name):
    """OpenFireRestApiCli.createGeneralChatRoom's MUCRoomEntity, as JAXB writes it."""
    return (f"<chatRoom><roomName>{name}</roomName><naturalName>{name}</naturalName>"
            f"<description>{name}</description><maxUsers>0</maxUsers><persistent>true</persistent>"
            "<publicRoom>false</publicRoom><registrationEnabled>false</registrationEnabled>"
            "<canAnyoneDiscoverJID>false</canAnyoneDiscoverJID><canOccupantsChangeSubject>false</canOccupantsChangeSubject>"
            "<canOccupantsInvite>false</canOccupantsInvite><canChangeNickname>false</canChangeNickname>"
            "<logEnabled>true</logEnabled><loginRestrictedToNickname>false</loginRestrictedToNickname>"
            "<membersOnly>false</membersOnly><moderated>false</moderated>"
            "<broadcastPresenceRoles><broadcastPresenceRole>moderator</broadcastPresenceRole>"
            "<broadcastPresenceRole>participant</broadcastPresenceRole>"
            "<broadcastPresenceRole>visitor</broadcastPresenceRole></broadcastPresenceRoles></chatRoom>")


def main():
    # OpenFireRestApiCli.init and UserBO.createXmppUser.
    rest("POST", "users", f"<user><username>sbrw.engine.engine</username><password>{TOKEN}</password></user>")
    for n in (1, 2):
        rest("POST", "chatrooms", chat_room(f"channel.EN__{n}"))
    for user in ("sbrw.100", "sbrw.101"):
        rest("POST", "users", f"<user><username>{user}</username><password>{PASSWORD}</password></user>")

    print("# A logs in with a wrong password, then the right one")
    a = Conn("A")
    a.open_stream()
    a.send("<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>")
    a.recv()
    a.starttls()
    a.open_stream()
    a.send("<iq id='auth0' type='set'><query xmlns='jabber:iq:auth'><username>sbrw.100</username>"
           "<password>wrong</password><resource>EA-Chat</resource></query></iq>")
    a.recv()
    a.send("<iq id='auth1' type='get'><query xmlns='jabber:iq:auth'><username>sbrw.100</username></query></iq>")
    a.recv()
    a.send(f"<iq id='auth2' type='set'><query xmlns='jabber:iq:auth'><username>sbrw.100</username>"
           f"<password>{PASSWORD}</password><resource>EA-Chat</resource></query></iq>")
    a.recv()
    a.send("<presence/>")
    a.recv()

    print("# A joins the public room, then B")
    room = f"channel.EN__1@conference.{DOMAIN}"
    a.send(f"<presence to='{room}/sbrw.100'><x xmlns='http://jabber.org/protocol/muc'/></presence>")
    a.recv()
    b = login("B", "sbrw.101", PASSWORD)
    b.send("<presence/>")
    b.recv()
    b.send(f"<presence to='{room}/sbrw.101'><x xmlns='http://jabber.org/protocol/muc'/></presence>")
    b.recv()
    a.recv()

    print("# A chats in the room")
    a.send(f"<message to='{room}' type='groupchat'><body>hello</body></message>")
    a.recv()
    b.recv()

    print("# B leaves the room")
    b.send(f"<presence to='{room}/sbrw.101' type='unavailable'/>")
    b.recv()
    a.recv()

    print("# The engine pushes to A, as OpenFireConnector.send does")
    engine = login("E", "sbrw.engine.engine", TOKEN, resource="EA_Chat")
    engine.send(f"<message to='sbrw.100@{DOMAIN}' id='JN_1234567'><subject>1337733113377331</subject>"
                "<body>&lt;response status='1' ticket='0'&gt;&lt;/response&gt;</body></message>")
    engine.recv()
    a.recv()

    print("# A logs in again with the same resource")
    a2 = login("A2", "sbrw.100", PASSWORD)
    a.recv()
    a2.recv()


if __name__ == "__main__":
    main()
