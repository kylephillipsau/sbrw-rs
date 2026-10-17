#!/usr/bin/env python3
"""Car states decoded by freeroam's own carstate package, as vectors for sbrw-udp:

    python3 reference/freeroam/carstate_vectors.py > fixtures/vectors/freeroam_carstate.json

Runs carstate_dump.go inside the freeroam module in golang:1.22 for
linux/amd64. The platform matters: on arm64 Go fuses `x*y + z` into one
multiply-add, which rounds differently, while amd64 Go (and Rust) round each
step. Production free-roam servers are amd64.

The car states are real ones, taken from the slot packets in
NFS-World-Offline's src/freeroam/packets.json: a spread of ground states, air
states and the longer states some clients send. Then edge cases: every prefix
of a real ground and air state, all-ones states (every field takes the
long-form branch of DecodeFloat), cmd/postest's packet, and seeded random bytes.
"""
import base64
import json
import os
import random
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
UPSTREAM = os.path.join(HERE, "../../upstream")
CAPTURE = os.path.join(UPSTREAM, "NFS-World-Offline/src/freeroam/packets.json")


def capture_car_states():
    """Distinct car-state subpackets (type 0x12) in the capture's slot packets, in order."""
    seen, cars = set(), []
    for item in json.load(open(CAPTURE)):
        if item is None:
            continue
        packet = base64.b64decode(item)
        if packet[2] != 0x02:
            continue
        i = 12
        while i < len(packet) - 4:
            if packet[i:i + 2] == b"\xff\xff":
                i += 2
                continue
            i += 1
            while packet[i] != 0xFF:
                kind, length = packet[i], packet[i + 1]
                car = packet[i + 2:i + 2 + length]
                if kind == 0x12 and car not in seen:
                    seen.add(car)
                    cars.append(car)
                i += 2 + length
            i += 1
    return cars


def spread(items, n):
    return items[:: max(1, len(items) // n)][:n]


def inputs():
    cars = capture_car_states()
    ground = [c for c in cars if len(c) <= 26 and c[2] & 0x08]
    air = [c for c in cars if len(c) <= 26 and not c[2] & 0x08]
    longer = [c for c in cars if len(c) > 26]
    chosen = spread(ground, 400) + spread(air, 300) + spread(longer, 150)
    edge = []
    for real in (ground[0], air[0]):
        edge += [real[:n] for n in range(2, len(real))]
    edge += [b"\xff" * n for n in range(2, 31)]
    edge.append(bytes.fromhex("2ea6900e626f45cbfa27a97e6e570f4b932b2d2b3668187f"))  # cmd/postest
    rng = random.Random(9999)
    edge += [bytes(rng.randrange(256) for _ in range(rng.randrange(2, 40))) for _ in range(100)]
    return chosen + edge


def decode(packets):
    script = (
        "cp -r /src /app && mkdir -p /app/cmd/carstatedump && cp /ref/carstate_dump.go /app/cmd/carstatedump/main.go"
        " && cd /app && go run ./cmd/carstatedump"
    )
    run = subprocess.run(
        ["docker", "run", "--rm", "-i", "--platform", "linux/amd64",
         "-v", os.path.abspath(os.path.join(UPSTREAM, "freeroam")) + ":/src:ro",
         "-v", HERE + ":/ref:ro", "golang:1.22", "sh", "-c", script],
        input="".join(p.hex() + "\n" for p in packets), capture_output=True, text=True, check=True,
    )
    rows = [json.loads(line) for line in run.stdout.splitlines()]
    assert [r["packet"] for r in rows] == [p.hex() for p in packets], run.stderr
    return rows


rows = decode(inputs())
print("{")
print('"about": "carstate.PacketReader.Decode of freeroamd (WorldUnitedNFS/freeroam) on Go 1.22, linux/amd64; floats as IEEE 754 bits",')
print('"cases": [')
print(",\n".join(json.dumps(r, separators=(",", ":")) for r in rows))
print("]}")
