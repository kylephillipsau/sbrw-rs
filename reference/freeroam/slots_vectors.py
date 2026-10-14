#!/usr/bin/env python3
"""What the Go freeroamd answers a lone client, as vectors for sbrw-udp:

    python3 reference/freeroam/slots_vectors.py HOST PORT > fixtures/vectors/freeroam_slots.json

Run freeroamd as in handshake_vectors.py. The channel, player and car-state
subpackets are real ones from the game, taken from a slot packet in
NFS-World-Offline's src/freeroam/packets.json. Each scenario is one client on
its own socket, and scenarios are 7 s apart so freeroamd has dropped the
previous client (it drops clients after 5 s of silence) and every slot stays
empty: other players in view are M3.
"""
import base64
import json
import os
import socket
import sys
import time

HOST, PORT = sys.argv[1], int(sys.argv[2])
CAPTURE = os.path.join(os.path.dirname(__file__), "../../upstream/NFS-World-Offline/src/freeroam/packets.json")


def subpackets():
    """The first full slot in the capture's first slot packet, and a second car state."""
    packet = base64.b64decode(json.load(open(CAPTURE))[4])
    i, slots = 12, []
    while i < len(packet) - 4:
        if packet[i:i + 2] == b"\xff\xff":
            i += 2
            continue
        i += 1
        found = {}
        while packet[i] != 0xFF:
            kind, length = packet[i], packet[i + 1]
            found[kind] = packet[i + 2:i + 2 + length]
            i += 2 + length
        i += 1
        slots.append(found)
    return slots[0][0x00], slots[0][0x01], slots[0][0x12], slots[1][0x12]


CHANNEL, PLAYER, CAR, CAR2 = subpackets()


def tlv(kind, data):
    return bytes([kind, len(data)]) + data


def client_packet(*tlvs, ack=0):
    """16 header bytes (the ack counter at 8..10), the subpackets, 5 trailer bytes."""
    header = bytearray(16)
    header[8:10] = ack.to_bytes(2, "big")
    return bytes(header) + b"".join(tlvs) + bytes(5)


def hello(tick):
    packet = bytearray(58)
    packet[2] = 0x06
    packet[52:54] = tick.to_bytes(2, "big")
    return bytes(packet)


SCENARIOS = {
    "ready_at_once": [
        hello(0x1234),
        client_packet(tlv(0x00, CHANNEL), tlv(0x01, PLAYER), tlv(0x12, CAR)),
        client_packet(tlv(0x12, CAR2), ack=1),
        client_packet(ack=2),
        bytes(20),
        bytes(9),
    ],
    "ready_bit_by_bit": [
        hello(0xFFF0),
        client_packet(tlv(0x12, CAR)),
        client_packet(tlv(0x00, CHANNEL)),
        client_packet(tlv(0x01, PLAYER)),
    ],
    "no_hello": [
        client_packet(tlv(0x00, CHANNEL), tlv(0x01, PLAYER), tlv(0x12, CAR)),
    ],
    "hello_again": [
        hello(0x0100),
        client_packet(tlv(0x00, CHANNEL), tlv(0x01, PLAYER), tlv(0x12, CAR)),
        hello(0x0200),
        client_packet(),
    ],
}


def run(packets):
    exchanges = []
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
        s.settimeout(0.3)
        for packet in packets:
            s.sendto(packet, (HOST, PORT))
            try:
                reply = s.recv(2048).hex()
            except socket.timeout:
                reply = None
            exchanges.append({"send": packet.hex(), "reply": reply})
    return exchanges


cases = []
for name, packets in SCENARIOS.items():
    cases.append({"name": name, "exchanges": run(packets)})
    time.sleep(7)
print(json.dumps({"about": "freeroamd (WorldUnitedNFS/freeroam) on Go 1.22, one client at a time", "cases": cases}, indent=2))
