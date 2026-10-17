#!/usr/bin/env python3
"""Compare two multi_vectors.py recordings, ignoring the clock fields:

    python3 reference/freeroam/multi_vectors.py 127.0.0.1 9999 > /tmp/sbrw.json   # against `sbrw serve`
    python3 reference/freeroam/compare.py fixtures/vectors/freeroam_multi.json /tmp/sbrw.json

Every reply carries the server's tick at bytes 3..5 and the tick difference
at 5..7, which differ from run to run; the rest must match. Exits non-zero
on a difference.
"""
import json
import sys


def exchanges(path):
    out = []
    for case in json.load(open(path))["cases"]:
        for e in case["exchanges"]:
            reply = e["reply"] and e["reply"][:6] + "x" * 8 + e["reply"][14:]
            out.append((case["name"], case["players"][e["player"]], e["send"], reply))
    return out


a, b = exchanges(sys.argv[1]), exchanges(sys.argv[2])
differences = [(x, y) for x, y in zip(a, b) if x != y]
for x, y in differences:
    print(f"{x[0]} {x[1]}: {x[3]}\n  vs {y[3]}")
print(f"{len(a)} and {len(b)} exchanges, {len(differences)} differences")
sys.exit(1 if differences or len(a) != len(b) else 0)
