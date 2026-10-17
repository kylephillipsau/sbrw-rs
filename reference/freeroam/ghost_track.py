#!/usr/bin/env python3
"""One real player's car-state stream, for the testkit's ghost:

    python3 reference/freeroam/ghost_track.py > fixtures/freeroam/ghost.json

NFS-World-Offline's src/freeroam/packets.json holds the slot packets a public
SBRW server sent one player. A player keeps its slot while in view, so one
slot index from a full slot (channel, player and car state) until the slot
empties or another player's full slot replaces it is one player's stream.
This takes the first stream in the capture with at least 50 distinct car
states at most 200 ms apart, which moves at least 250 units in all while
staying within 200 units of where it began, so a player parked at its start
sees all of it.

The player's name becomes GHOST and its persona id 100; the channel info and
car states are as captured. Each car state comes with the milliseconds since
the previous one, from the sim time in its first two bytes.
"""
import base64
import json
import math
import os
import sys

CAPTURE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                       "../../upstream/NFS-World-Offline/src/freeroam/packets.json")


class Bits:
    def __init__(self, data):
        self.data, self.at = data, 0

    def read(self, n):
        n = min(n, len(self.data) * 8 - self.at)
        value = 0
        for _ in range(n):
            value = (value << 1) | ((self.data[self.at // 8] >> (7 - self.at % 8)) & 1)
            self.at += 1
        return value

    def field(self, n, threshold, multiply, add):
        """carstate's DecodeFloat."""
        raw = self.read(n)
        if raw >= threshold:
            raw = self.read(1) + 2 * raw - threshold
        return (raw + 0.5) * multiply + add


def position(car):
    """x and y, as carstate decodes them."""
    bits = Bits(car)
    bits.read(16)
    header = bits.read(12)
    if header & 0x80:  # on the ground
        y = bits.field(17, 62144, 0.039999999, -5000)
        bits.field(11, 96, 0.12774999, -112)
        x = bits.field(17, 62144, 0.059999999, 0)
    else:
        for n, threshold in ((9, 0xE0), (8, 0x70), (9, 0xE0)):
            bits.field(n, threshold, 1, 0)
        y = bits.field(17, 62144, 0.15000001, -15000)
        bits.field(11, 96, 0.639999999, -512)
        x = bits.field(17, 62144, 0.15000001, -15000)
    return x, -y


def slots(packet):
    out, i = [], 12
    while i < len(packet) - 4:
        if packet[i:i + 2] == b"\xff\xff":
            out.append(None)
            i += 2
            continue
        i += 1
        found = {}
        while packet[i] != 0xFF:
            kind, length = packet[i], packet[i + 1]
            found[kind] = packet[i + 2:i + 2 + length]
            i += 2 + length
        i += 1
        out.append(found)
    return out


def streams():
    """Each slot's runs of one player, in the order they end."""
    current = {}
    for item in json.load(open(CAPTURE)):
        packet = base64.b64decode(item) if item else b""
        if len(packet) < 3 or packet[2] != 0x02:
            continue
        for index, slot in enumerate(slots(packet)):
            run = current.get(index)
            if slot is None:
                if run:
                    yield run
                current[index] = None
                continue
            if 0x01 in slot and (run is None or run["player"][1:33] != slot[0x01][1:33]):
                if run:
                    yield run
                run = current[index] = {"channel": slot[0x00], "player": slot[0x01], "cars": []}
            if run is not None and 0x12 in slot and (not run["cars"] or run["cars"][-1] != slot[0x12]):
                run["cars"].append(slot[0x12])


def gaps(cars):
    return [(int.from_bytes(b[:2], "big") - int.from_bytes(a[:2], "big")) & 0xFFFF for a, b in zip(cars, cars[1:])]


def suitable(run):
    cars = run["cars"]
    if len(cars) < 50 or max(gaps(cars)) > 200:
        return False
    points = [position(c) for c in cars]
    reach = max(math.dist(points[0], p) for p in points)
    path = sum(math.dist(a, b) for a, b in zip(points, points[1:]))
    return reach <= 200 and path >= 250


run = next(r for r in streams() if suitable(r))
player = bytearray(run["player"])
player[1:33] = b"GHOST".ljust(32, b"\0")
player[41:45] = (100).to_bytes(4, "little")
cars = run["cars"]
print(json.dumps({
    "about": "One player's car states from NFS-World-Offline's packets.json; see reference/freeroam/ghost_track.py",
    "channel": run["channel"].hex(),
    "player": player.hex(),
    "cars": [[gap, car.hex()] for gap, car in zip([0] + gaps(cars), cars)],
}, indent=1))
