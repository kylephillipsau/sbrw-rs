#!/usr/bin/env python3
"""Drive the reference Openfire as the game would and print the transcript.

    python3 reference/xmpp/transcript.py HOST PORT

Each line is `>` (client) or `<` (server) and the raw text. TLS is pinned to
1.0 with the old ciphers allowed, as the game's stack needs.
"""
import socket
import ssl
import sys
import time

HOST, PORT = sys.argv[1], int(sys.argv[2])
DOMAIN = "127.0.0.1"


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
        text = data.decode()
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


def register(user, password):
    c = Conn("setup")
    c.open_stream()
    c.send("<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>")
    c.recv()
    c.starttls()
    c.open_stream()
    c.send(f"<iq id='reg1' type='set'><query xmlns='jabber:iq:register'><username>{user}</username>"
           f"<password>{password}</password></query></iq>")
    c.recv()
    c.send("</stream:stream>")
    c.recv()


if __name__ == "__main__":
    for user in ("sbrw.100", "sbrw.101"):
        register(user, "0123456789abcdef")
    a = login("A", "sbrw.100", "0123456789abcdef")
    a.send("<iq id='auth3' type='set'><query xmlns='jabber:iq:auth'><username>sbrw.100</username>"
           "<password>wrong</password><resource>EA-Chat</resource></query></iq>")
    a.recv()
    a.send("<presence/>")
    a.recv()
    a.send(f"<presence to='channel.EN__1@conference.{DOMAIN}/sbrw.100'><x xmlns='http://jabber.org/protocol/muc'/></presence>")
    a.recv()
    time.sleep(0.2)
