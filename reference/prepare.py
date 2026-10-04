#!/usr/bin/env python3
"""Assemble reference/build/core: the upstream core, made deterministic, plus the harness.

The patches are mechanical and counted. If upstream changes and a count no
longer matches, this fails instead of silently recording nondeterministic
fixtures. Afterwards no unpatched source of randomness or wall-clock time may
remain in the core.
"""

import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UPSTREAM = ROOT / "upstream/soapbox-race-core"
REFERENCE = ROOT / "reference"
OUT = REFERENCE / "build/core"
D = "com.soapboxrace.reference.Determinism"

# (pattern, replacement, expected number of replacements)
PATCHES = [
    (r"new Random\(\)", f"{D}.random()", 6),
    (r"Math\.random\(\)", f"{D}.random().nextDouble()", 4),
    (r"RandomUtils\.nextInt\(", f"{D}.nextInt(", 1),
    (r"Collections\.shuffle\((\w+)\)", rf"Collections.shuffle(\1, {D}.random())", 1),
    (r"UUID\.randomUUID\(\)", f"{D}.uuid()", 3),
    (r"LocalDateTime\.now\(\)", f"LocalDateTime.now({D}.clock())", 19),
    (r"LocalDate\.now\(\)", f"LocalDate.now({D}.clock())", 6),
    (r"OffsetDateTime\.now\(\)", f"OffsetDateTime.now({D}.clock())", 1),
    (r"System\.currentTimeMillis\(\)", f"{D}.millis()", 12),
    (r"new Date\(\)", f"new Date({D}.millis())", 7),
    (r"new GregorianCalendar\(\)", f"GregorianCalendar.from(java.time.ZonedDateTime.now({D}.clock()))", 1),
]

# Anything matching these after patching is an unhandled source of nondeterminism.
LEFTOVERS = re.compile(
    r"new Random\(|Math\.random\(|RandomUtils\.|RandomStringUtils\.|ThreadLocalRandom|SecureRandom|"
    r"Collections\.shuffle\(\w+\)|UUID\.randomUUID|\.now\(\)|currentTimeMillis|nanoTime|"
    r"new GregorianCalendar\(\)|Calendar\.getInstance\(\)|new Date\(\)"
)


def main() -> int:
    if not UPSTREAM.exists():
        print("upstream/soapbox-race-core is missing; run tools/fetch-upstream.sh", file=sys.stderr)
        return 1
    if OUT.exists():
        shutil.rmtree(OUT)
    shutil.copytree(UPSTREAM / "src/main", OUT / "src/main")
    shutil.copy(REFERENCE / "pom.xml", OUT / "pom.xml")
    shutil.copytree(REFERENCE / "overlay", OUT, dirs_exist_ok=True)
    shutil.copytree(REFERENCE / "harness", OUT / "src/main/java", dirs_exist_ok=True)

    core = OUT / "src/main/java/com/soapboxrace/core"
    sources = sorted(core.rglob("*.java"))
    texts = {p: p.read_text(encoding="utf-8") for p in sources}
    failed = False
    for pattern, replacement, expected in PATCHES:
        total = 0
        for p, text in texts.items():
            texts[p], n = re.subn(pattern, replacement, text)
            total += n
        if total != expected:
            print(f"patch {pattern!r}: expected {expected} replacements, made {total}", file=sys.stderr)
            failed = True
    for p, text in texts.items():
        p.write_text(text, encoding="utf-8")
        for n, line in enumerate(text.splitlines(), 1):
            if LEFTOVERS.search(line.replace(f"{D}.", "")):
                print(f"unpatched nondeterminism: {p.relative_to(OUT)}:{n}: {line.strip()}", file=sys.stderr)
                failed = True
    if failed:
        return 1
    print(f"prepared {OUT.relative_to(ROOT)} ({len(sources)} core sources patched)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
