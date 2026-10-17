#!/usr/bin/env python3
"""What the Go freeroamd answers several players at once, as vectors for sbrw-udp.

Run freeroamd and this script on one Docker network, so no port proxy sits
between them and every socket keeps its own source address:

    docker network create sbrw-freeroam
    docker run -d --name sbrw-freeroamd --network sbrw-freeroam --platform linux/amd64 \\
        -v "$PWD/upstream/freeroam:/src:ro" golang:1.22 sh -c 'cp -r /src /app && cd /app && go run ./cmd/freeroamd'
    docker run --rm --network sbrw-freeroam -v "$PWD:/w:ro" python:3.12-slim \\
        python3 /w/reference/freeroam/multi_vectors.py sbrw-freeroamd 9999 > fixtures/vectors/freeroam_multi.json

Each scenario is a set of players on their own sockets, sending at scheduled
times, and every reply is recorded with the time it was sent. Scenarios are
7 s apart so freeroamd has dropped the previous players.

freeroamd moves one waiting player into a free slot every 200 ms on a ticker
that starts at each player's hello, and it reads slots only when the player
sends. So each player sends only mid-way between its own ticks (hello + 100 +
200k ms): which ticks came before a send is then certain, and the Rust test
replays the recorded times. The script checks every send kept 30 ms from a
tick. Likewise players are placed so that no two of anyone's distances
truncate to the same whole number, since freeroamd breaks such ties in Go's
random map order, and nothing is observed between 5 and 6 s of a player's
silence, when freeroamd's once-a-second sweep may or may not have dropped it.

Car states are ground states built here at chosen positions. Channel and
player info are real ones from NFS-World-Offline's packets.json, with each
player's name and persona id patched in.
"""
import base64
import json
import math
import os
import socket
import sys
import time

HOST, PORT = sys.argv[1], int(sys.argv[2])
CAPTURE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                       "../../upstream/NFS-World-Offline/src/freeroam/packets.json")


def real_infos():
    """The channel and player info of the first full slot in the capture's first slot packet."""
    packet = base64.b64decode(json.load(open(CAPTURE))[4])
    i, found = 13, {}
    while packet[i] != 0xFF:
        kind, length = packet[i], packet[i + 1]
        found[kind] = packet[i + 2:i + 2 + length]
        i += 2 + length
    return found[0x00], found[0x01]


REAL_CHANNEL, REAL_PLAYER = real_infos()


def channel_info(name, social):
    data = bytearray(REAL_CHANNEL)
    data[1] = 1 if social else 0
    data[2:] = bytes(len(data) - 2)
    data[2:2 + len(name)] = name.encode()
    return bytes(data)


def player_info(name, persona_id):
    data = bytearray(REAL_PLAYER)
    data[1:33] = bytes(32)
    data[1:1 + len(name)] = name.encode()
    data[41:45] = persona_id.to_bytes(4, "little")
    return bytes(data)


class Bits:
    def __init__(self):
        self.bits = []

    def put(self, value, n):
        self.bits += [(value >> (n - 1 - i)) & 1 for i in range(n)]

    def field(self, raw, n, threshold):
        """The inverse of carstate's DecodeFloat for a raw value."""
        if raw < threshold:
            self.put(raw, n)
        else:
            extra = (raw + threshold) % 2
            self.put((raw + threshold - extra) // 2, n)
            self.put(extra, 1)

    def bytes(self, length):
        bits = self.bits + [0] * (length * 8 - len(self.bits))
        return bytes(int("".join(map(str, bits[i:i + 8])), 2) for i in range(0, len(bits), 8))


def raw_x(x):
    return round(x / 0.059999999 - 0.5)


def raw_y(y):
    return round((5000 - y) / 0.039999999 - 0.5)


def decoded(x, y):
    """The position freeroamd decodes, by the same float steps."""
    return (raw_x(x) + 0.5) * 0.059999999 + 0.0, -((raw_y(y) + 0.5) * 0.039999999 + -5000.0)


def ground_state(sim_time, x, y):
    """A 26-byte ground car state at (x, y), at rest, upright."""
    b = Bits()
    b.put(sim_time & 0xFFFF, 16)
    b.put(0b100110000000, 12)  # header bits of a real ground state; the fifth is "on the ground"
    b.field(raw_y(y), 17, 62144)
    b.field(900, 11, 96)  # z
    b.field(raw_x(x), 17, 62144)
    b.field(10000, 14, 0x31E0)  # linear velocity y, z, x: about zero
    b.field(600, 10, 0x350)
    b.field(10000, 14, 0x31E0)
    b.field(255, 8, 1)  # orientation y, z, x, w: about (0, 0, 0, 1)
    b.field(400, 9, 0xE0)
    b.field(255, 8, 1)
    b.field(799, 9, 0xE0)
    for _ in range(3):
        b.field(150, 8, 0xD3)  # angular velocity: about zero
    b.field(50, 6, 0x1B)  # wheels straight
    b.field(50, 6, 0x1B)
    b.put(0, 13)  # effects
    return b.bytes(26)


def tlv(kind, data):
    return bytes([kind, len(data)]) + data


class Player:
    def __init__(self, index, name, x, y, channel="channel.EN__1", social=False, ack="echo"):
        self.index, self.name, self.x, self.y = index, name, x, y
        self.channel, self.social, self.ack = channel, social, ack
        self.sim_time = 0x1000 * (index + 1)
        self.last_seq = 0
        self.last_car = None

    def car(self, x=None, y=None, same=False):
        if not same:
            if x is not None:
                self.x, self.y = x, y
            self.sim_time += 33
            self.last_car = ground_state(self.sim_time, self.x, self.y)
        return self.last_car

    def packet(self, *tlvs):
        """16 header bytes (the ack counter at 8..10), the subpackets, 5 trailer bytes."""
        header = bytearray(16)
        ack = self.last_seq if self.ack == "echo" else self.ack
        header[8:10] = ack.to_bytes(2, "big")
        return bytes(header) + b"".join(tlvs) + bytes(5)

    def hello(self, tick):
        packet = bytearray(58)
        packet[2] = 0x06
        packet[52:54] = tick.to_bytes(2, "big")
        return bytes(packet)

    def ready(self):
        return self.packet(tlv(0x00, channel_info(self.channel, self.social)),
                           tlv(0x01, player_info(self.name, 100 + self.index)), tlv(0x12, self.car()))


def check_distances(players):
    """No two of a player's distances to the others truncate alike, and none is near the radius."""
    for p in players:
        here = decoded(p.x, p.y)
        whole = []
        for q in players:
            if q is p:
                continue
            there = decoded(q.x, q.y)
            d = math.sqrt((here[0] - there[0]) ** 2 + (here[1] - there[1]) ** 2)
            assert abs(d - 300) > 1, (p.name, q.name, d)
            whole.append(int(d))
        assert len(set(whole)) == len(whole), (p.name, whole)


def run(name, players, steps):
    """steps: (player, k, action, *args). A player's first hello goes at its offset, and
    its k-th send at offset + 100 + 200k ms, mid-way between its ticks. A later hello
    goes at a half-integer k, a multiple of 200 ms after the first, so the player's new
    ticks keep the old phase.

    Actions: "hello", "ready" (channel, player and car), "partial" (channel and player
    only), "car" (x, y, or "same" to resend the last car state), "poll" (no subpackets).
    """
    spacing = 180 // (len(players) - 1) if len(players) > 1 else 0
    offsets = {p.index: spacing * i for i, p in enumerate(players)}
    socks = {p.index: socket.socket(socket.AF_INET, socket.SOCK_DGRAM) for p in players}
    for s in socks.values():
        s.bind(("0.0.0.0", 0))
    timed = []
    for player, k, action, *args in steps:
        if action == "hello" and k == 0:
            at = offsets[player.index]
        else:
            assert action != "hello" or k % 1 == 0.5, "a later hello goes at a half-integer k"
            at = offsets[player.index] + 100 + 200 * k
        timed.append((at, len(timed), player, action, args))
    timed.sort(key=lambda t: (t[0], t[1]))
    hello_at, exchanges = {}, []
    start = time.monotonic()
    for i, (at, _, player, action, args) in enumerate(timed):
        delay = start + at / 1000 - time.monotonic()
        if delay > 0:
            time.sleep(delay)
        if action == "hello":
            packet = player.hello(0x0100 * (player.index + 1))
        elif action == "ready":
            packet = player.ready()
        elif action == "partial":
            packet = player.packet(tlv(0x00, channel_info(player.channel, player.social)),
                                   tlv(0x01, player_info(player.name, 100 + player.index)))
        elif action == "car":
            packet = player.packet(tlv(0x12, player.car(same=True) if args == ["same"] else player.car(*args)))
        else:
            assert action == "poll", action
            packet = player.packet()
        sock = socks[player.index]
        sent = (time.monotonic() - start) * 1000
        sock.sendto(packet, (HOST, PORT))
        if action == "hello":
            hello_at[player.index] = sent
        else:
            since = (sent - hello_at[player.index]) % 200
            assert 30 <= since <= 170, f"{name}: {player.name} sent {since:.1f} ms after a tick"
        upcoming = timed[i + 1][0] if i + 1 < len(timed) else at + 100
        sock.settimeout(max(0.003, min(0.02, start + upcoming / 1000 - time.monotonic() - 0.002)))
        try:
            reply = sock.recv(4096)
            player.last_seq = int.from_bytes(reply[0:2], "big")
            reply = reply.hex()
        except socket.timeout:
            reply = None
        exchanges.append({"at": round(sent, 2), "player": player.index, "send": packet.hex(), "reply": reply})
    for s in socks.values():
        s.close()
    return {"name": name, "players": [p.name for p in players], "exchanges": exchanges}


def two_in_range():
    """Two players see each other: full slot, then "nothing new", positions, acks echoed.
    A resent identical car state still counts as new."""
    a, b = Player(0, "ALPHA", 5000, 1000), Player(1, "BRAVO", 5100, 1000)
    check_distances([a, b])
    steps = [(a, 0, "hello"), (b, 0, "hello"), (a, 0, "ready"), (b, 0, "ready")]
    steps += [(a, 1, "poll"), (b, 1, "poll"), (a, 2, "poll"), (b, 2, "poll"), (a, 3, "poll")]
    steps += [(b, 3, "car", 5110, 1000), (a, 4, "poll"), (a, 4.25, "poll")]
    steps += [(b, 4, "car", "same"), (a, 5, "poll"), (a, 5.25, "poll"), (b, 5, "poll")]
    return run("two_in_range", [a, b], steps)


def acks():
    """A never acks (0xFFFF): a full slot again after five misses. C acks 0, which matches
    the zero a new slot starts with, so it counts as acked before anything is sent."""
    a = Player(0, "ALPHA", 5000, 1000, ack=0xFFFF)
    b = Player(1, "BRAVO", 5100, 1000)
    c = Player(2, "CHARLIE", 5037, 1000, ack=0)
    check_distances([a, b, c])
    steps = [(p, 0, "hello") for p in (a, b, c)] + [(p, 0, "ready") for p in (a, b, c)]
    steps += [(a, 1, "poll"), (c, 1, "poll")]
    for k in range(2, 16):
        steps += [(b, k, "car", 5100 + k, 1000), (a, k + 0.25, "poll"), (c, k + 0.25, "poll")]
    steps += [(b, 16, "poll")]
    return run("acks", [a, b, c], steps)


def range_edges():
    """Out of range, then driving in (seen after a tick), out (gone at once) and back in.
    D sends channel and player info but no car state, so it is never seen."""
    a = Player(0, "ALPHA", 5000, 1000)
    b = Player(1, "BRAVO", 5400, 1000)
    d = Player(2, "DELTA", 5050, 1050)
    check_distances([a, b, d])
    steps = [(p, 0, "hello") for p in (a, b, d)]
    steps += [(a, 0, "ready"), (b, 0, "ready"), (d, 0, "partial")]
    steps += [(a, 1, "poll"), (b, 1, "poll"), (a, 2, "poll")]
    steps += [(b, 2, "car", 5250, 1000), (a, 3, "poll"), (a, 4, "poll"), (a, 5, "poll"), (b, 5, "poll")]
    steps += [(b, 6, "car", 5350, 1000), (a, 6.25, "poll"), (a, 7, "poll")]
    steps += [(b, 7, "car", 5200, 1000), (a, 8, "poll"), (a, 9, "poll"), (a, 10, "poll"), (b, 10, "poll")]
    steps += [(d, 10, "poll")]
    return run("range_edges", [a, b, d], steps)


def stale_waiting():
    """B and C wait for A's slots; B leaves range before A's tick and stays in the queue,
    even through a send from A, takes a slot anyway, and is dropped at A's next send, so
    C shows a tick late."""
    a = Player(0, "ALPHA", 5000, 1000)
    b = Player(1, "BRAVO", 5050, 1000)
    c = Player(2, "CHARLIE", 5000, 1080)
    check_distances([a, b, c])
    steps = [(p, 0, "hello") for p in (a, b, c)] + [(p, 0, "ready") for p in (a, b, c)]
    steps += [(a, 1, "poll"), (b, 0.75, "car", 5600, 1000), (a, 1.25, "poll"), (a, 2, "poll"), (a, 3, "poll")]
    steps += [(a, 4, "poll")]
    steps += [(c, 4, "poll")]
    return run("stale_waiting", [a, b, c], steps)


def social():
    """A filters by channel, so players in its channel queue first; D doesn't filter."""
    a = Player(0, "ALPHA", 5000, 1000, channel="channel.EN__1", social=True)
    b = Player(1, "BRAVO", 5150, 1000, channel="channel.EN__1")
    c = Player(2, "CHARLIE", 5000, 1060, channel="channel.ES__1")
    d = Player(3, "DELTA", 4990, 1035, channel="channel.EN__1")
    check_distances([a, b, c, d])
    steps = [(p, 0, "hello") for p in (a, b, c, d)] + [(p, 0, "ready") for p in (a, b, c, d)]
    for k in range(1, 6):
        steps += [(a, k, "poll"), (d, k, "poll")]
    return run("social", [a, b, c, d], steps)


def many_at_once():
    """Five players queue for A while A is silent; A then gets three full slots and two
    positions only, and the other two full slots at its next send."""
    a = Player(0, "ALPHA", 5000, 1000)
    others = [Player(i + 1, f"P{i + 1}", 5000 + 10 * m, 1000) for i, m in enumerate((1, 4, 10, 12, 17))]
    players = [a] + others
    check_distances(players)
    steps = [(p, 0, "hello") for p in players] + [(p, 0, "ready") for p in players]
    steps += [(a, 1, "poll"), (a, 7, "poll"), (a, 8, "poll"), (a, 9, "poll")]
    steps += [(p, 8, "poll") for p in others]
    return run("many_at_once", players, steps)


def fourteen_nearest():
    """Fifteen players around A: A sees the fourteen nearest, one more every 200 ms."""
    golomb = (0, 1, 4, 11, 26, 32, 56, 68, 76, 115, 117, 134, 150, 163, 168, 177)
    players = [Player(i, f"P{i}", 5000 + 1.5 * m, 1000) for i, m in enumerate(golomb)]
    check_distances(players)
    a = players[0]
    steps = [(p, 0, "hello") for p in players] + [(p, 0, "ready") for p in players]
    steps += [(a, k, "poll") for k in range(1, 19)]
    steps += [(p, 18, "poll") for p in players[1:]]
    return run("fourteen_nearest", players, steps)


def hello_again():
    """B says hello again: A drops B's slot at once and sees the new B once it is ready.
    The second time B is ready again before A sends, and still A sees a new player."""
    a, b = Player(0, "ALPHA", 5000, 1000), Player(1, "BRAVO", 5100, 1000)
    check_distances([a, b])
    steps = [(a, 0, "hello"), (b, 0, "hello"), (a, 0, "ready"), (b, 0, "ready")]
    steps += [(a, 1, "poll"), (a, 2, "poll"), (a, 3, "poll"), (b, 2.5, "hello"), (b, 3, "poll")]
    steps += [(a, 4, "poll"), (b, 3.25, "ready"), (a, 5, "poll"), (a, 6, "poll"), (a, 7, "poll"), (b, 6, "poll")]
    steps += [(b, 7.5, "hello"), (b, 8, "ready"), (a, 9, "poll"), (a, 10, "poll"), (a, 11, "poll"), (b, 10, "poll")]
    return run("hello_again", [a, b], steps)


def timeout():
    """B goes silent: A sees "nothing new" for B until B is dropped after 5 s."""
    a, b = Player(0, "ALPHA", 5000, 1000), Player(1, "BRAVO", 5100, 1000)
    check_distances([a, b])
    steps = [(a, 0, "hello"), (b, 0, "hello"), (a, 0, "ready"), (b, 0, "ready")]
    steps += [(a, k, "poll") for k in (1, 2, 3, 12, 23)]
    # B's last send is at about 280 ms; at 6.5 s it has been silent for over 6 s.
    steps += [(a, 32, "poll"), (a, 33, "poll")]
    return run("timeout", [a, b], steps)


SCENARIOS = [two_in_range, acks, range_edges, stale_waiting, social, many_at_once, fourteen_nearest,
             hello_again, timeout]

cases = []
for scenario in SCENARIOS:
    cases.append(scenario())
    time.sleep(7)
print("{")
print('"about": "freeroamd (WorldUnitedNFS/freeroam) on Go 1.22, linux/amd64, several players at once; at is ms since the scenario began",')
print('"cases": [')
print(",\n".join(json.dumps(c, separators=(",", ":")) for c in cases))
print("]}")
