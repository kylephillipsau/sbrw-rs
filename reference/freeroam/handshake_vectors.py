#!/usr/bin/env python3
"""Handshake replies from the Go freeroamd, as vectors for sbrw-udp:

    docker run -d --name sbrw-freeroamd -p 19999:9999/udp -v "$PWD/upstream/freeroam:/src:ro" \\
        golang:1.22 sh -c 'cp -r /src /app && cd /app && go run ./cmd/freeroamd'
    python3 reference/freeroam/handshake_vectors.py 127.0.0.1 19999 > fixtures/vectors/freeroam_handshake.json

Each hello comes from a fresh socket, so each is a new client. The server's
tick is its clock, so the vectors record the reply and the test recomputes it
from the client's tick and the server tick inside the reply.
"""
import json
import socket
import sys

HOST, PORT = sys.argv[1], int(sys.argv[2])


def hello(tick, length=58, kind=0x06):
    packet = bytearray(length)
    if length > 2:
        packet[2] = kind
    if length >= 54:
        packet[52:54] = tick.to_bytes(2, "big")
    return bytes(packet)


def exchange(packet):
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
        s.settimeout(0.5)
        s.sendto(packet, (HOST, PORT))
        try:
            return s.recv(64).hex()
        except socket.timeout:
            return None


cases = []
for tick in (0x0000, 0x0001, 0x7FFF, 0x8000, 0xBEEF, 0xFFFF):
    cases.append({"packet": hello(tick).hex(), "reply": exchange(hello(tick))})
for packet in (hello(0x1234, length=57), hello(0x1234, length=59), hello(0x1234, kind=0x07)):
    cases.append({"packet": packet.hex(), "reply": exchange(packet)})
print(json.dumps({"about": "freeroamd (WorldUnitedNFS/freeroam) on Go 1.22: [hello packet, reply or null]", "cases": cases}, indent=2))
