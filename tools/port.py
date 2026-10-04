#!/usr/bin/env python3
"""Track the 1:1 port of soapbox-race-core.

  tools/port.py inventory   regenerate port/inventory.json from upstream/
  tools/port.py status      rewrite docs/port-status.md
  tools/port.py check       fail if a port marker or decision names an unknown unit

A unit is something with behaviour to port: a method of an API resource, BO,
DAO, utility, engine, event or XMPP class, or a JPA named query. Generated JAXB
DTOs are excluded (they are generated for Rust too) and so are trivial entity
getters/setters (their behaviour is the schema).

A unit counts as done when either
  - a Rust comment `// port: <unit id>` sits on the item that ports it, or
  - port/decisions.toml records it as `replaced` or `dropped`, with a reason.
"""

import json
import re
import sys
import tomllib
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CORE = ROOT / "upstream/soapbox-race-core"
SRC = CORE / "src/main/java/com/soapboxrace/core"
INVENTORY = ROOT / "port/inventory.json"
DECISIONS = ROOT / "port/decisions.toml"
STATUS = ROOT / "docs/port-status.md"
UPSTREAM_SHA = "d3edcd002514eb5265ad44484d79c4d2541ab055"

# Package directory (relative to core/) -> kind. Order is the bottom-up port order.
KINDS = {
    "jpa": "entity",
    "jpa/convert": "util",
    "jpa/util": "util",
    "dao/util": "dao",
    "dao": "dao",
    "engine": "engine",
    "vo": "util",
    "bo/util": "util",
    "events": "event",
    "xmpp": "xmpp",
    "bo": "bo",
    "api/util": "api",
    "api": "api",
}

CONTROL = {"if", "for", "while", "switch", "return", "new", "else", "catch", "synchronized", "throw", "try", "do"}
METHOD = re.compile(
    r"^    (?:(?:public|protected|private|static|final|abstract|synchronized|default|native)\s+)*"
    r"(?:<[^>]+>\s+)?([\w.<>\[\], ?]+?)\s+(\w+)\s*\(([^)]*)\)?"
)
CONSTRUCTOR = re.compile(r"^    (?:public|protected|private)\s+(\w+)\s*\(([^)]*)\)?")
FIELD_INJECT = re.compile(r"^\s+(?:private|protected|public)?\s*(?:final\s+)?([\w<>]+)\s+\w+\s*;")
ANNOTATION = re.compile(r"^\s+@(\w+)(?:\((.*)\))?\s*$")
NAMED_QUERY = re.compile(r'@NamedQuery\(\s*name\s*=\s*"([^"]+)"\s*,\s*(?:query\s*=\s*)?((?:"[^"]*"\s*\+?\s*)+)', re.S)


MEDIA_TYPES = {
    "MediaType.APPLICATION_XML": "application/xml",
    "MediaType.APPLICATION_JSON": "application/json",
    "MediaType.TEXT_HTML": "text/html",
    "MediaType.TEXT_PLAIN": "text/plain",
}


def media_type(arg):
    """The value of a @Produces annotation, e.g. `MediaType.APPLICATION_XML` or `"text/html"`."""
    if not arg:
        return None
    arg = arg.strip().strip("{}").split(",")[0].strip()
    return MEDIA_TYPES.get(arg, arg.strip('"'))


def simple_type(t: str) -> str:
    t = re.sub(r"<.*>", "", t.strip())
    t = re.sub(r"@\w+\s+", "", t)
    t = t.replace("final ", "").strip()
    return t.split(".")[-1].replace("...", "[]")


def param_types(params: str) -> list[str]:
    params = re.sub(r"@\w+(\([^)]*\))?\s*", "", params)
    depth, parts, cur = 0, [], ""
    for ch in params:
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        parts.append(cur)
    return [simple_type(" ".join(p.split()[:-1])) for p in parts if p.strip()]


def body_lines(lines: list[str], start: int) -> tuple[int, list[str]]:
    """Return (end index, body lines) for the member starting at `start`."""
    depth, seen_open = 0, False
    for i in range(start, len(lines)):
        stripped = re.sub(r'"(?:\\.|[^"\\])*"', '""', lines[i])
        stripped = re.sub(r"//.*", "", stripped)
        depth += stripped.count("{") - stripped.count("}")
        if "{" in stripped:
            seen_open = True
        if seen_open and depth <= 0:
            return i, lines[start : i + 1]
        if not seen_open and stripped.rstrip().endswith(";"):
            return i, lines[start : i + 1]  # abstract / interface method
    return len(lines) - 1, lines[start:]


def is_trivial_accessor(name: str, body: list[str]) -> bool:
    if not re.match(r"(get|set|is)[A-Z]", name):
        return False
    statements = [l.strip() for l in body[1:-1] if l.strip() and not l.strip().startswith("//")]
    if len(statements) != 1:
        return False
    s = statements[0]
    return bool(re.fullmatch(r"return (this\.)?\w+;", s) or re.fullmatch(r"(this\.)?\w+ = \w+;", s))


def parse_file(path: Path, kind: str) -> dict:
    rel = path.relative_to(SRC).as_posix()
    cls = path.stem
    lines = path.read_text(encoding="utf-8").splitlines()
    text = "\n".join(lines)

    deps, units, pending = [], [], []
    class_annotations = []
    i = 0
    in_class = False
    while i < len(lines):
        line = lines[i]
        if not in_class:
            m = ANNOTATION.match(line) or re.match(r"^@(\w+)", line)
            if m:
                class_annotations.append(m.group(1))
            if re.match(r"^(public\s+)?(abstract\s+)?(final\s+)?(class|interface|enum)\s", line):
                in_class = True
            i += 1
            continue

        ann = ANNOTATION.match(line)
        if ann and line.startswith("    @") and not line.startswith("     "):
            pending.append((ann.group(1), ann.group(2)))
            i += 1
            continue

        if line.startswith("    ") and not line.startswith("     "):
            names = {a for a, _ in pending}
            if names & {"EJB", "Inject", "PersistenceContext", "Resource"}:
                f = FIELD_INJECT.match(line)
                if f and "(" not in line:
                    deps.append(simple_type(f.group(1)))
            ctor = CONSTRUCTOR.match(line)
            meth = METHOD.match(line)
            name = None
            if ctor and ctor.group(1) == cls:
                name, params, ret = "<init>", ctor.group(2), ""
            elif meth and meth.group(2) not in CONTROL and "=" not in line.split("(")[0] and meth.group(1).split()[-1] not in CONTROL:
                name, params, ret = meth.group(2), meth.group(3), meth.group(1)
            if name:
                # Parameter lists may wrap onto following lines.
                j, full = i, line
                while full.count("(") > full.count(")") and j + 1 < len(lines):
                    j += 1
                    full += " " + lines[j].strip()
                params = full[full.index("(") + 1 : full.rindex(")")] if ")" in full else params
                end, body = body_lines(lines, i)
                annotations = [a for a, _ in pending]
                http = None
                ann_map = dict(pending)
                produces = None
                for verb in ("GET", "POST", "PUT", "DELETE"):
                    if verb in ann_map:
                        sub = (ann_map.get("Path") or "").strip('"')
                        http = f"{verb} {sub}".strip()
                        produces = media_type(ann_map.get("Produces"))
                trivial = kind == "entity" and is_trivial_accessor(name, body)
                if not trivial:
                    units.append(
                        {
                            "id": f"{rel.removesuffix('.java')}#{name}({','.join(param_types(params))})",
                            "name": name,
                            "line": i + 1,
                            "lines": end - i + 1,
                            "annotations": annotations,
                            **({"http": http} if http else {}),
                            **({"produces": produces} if produces else {}),
                        }
                    )
                pending = []
                i = end + 1
                continue
            if not line.strip().startswith("@"):
                pending = []
        i += 1

    # Upstream wraps annotations with empty `//` comments to stop the formatter joining lines.
    for qname, query in NAMED_QUERY.findall(re.sub(r"//[ \t]*$", "", text, flags=re.M)):
        jpql = "".join(re.findall(r'"([^"]*)"', query))
        units.append({"id": f"{rel.removesuffix('.java')}@{qname}", "name": qname, "query": " ".join(jpql.split())})

    class_path = None
    m = re.search(r'^@Path\("([^"]*)"\)', text, re.M)
    if m:
        class_path = m.group(1)
    return {
        "file": rel,
        "class": cls,
        "kind": kind,
        "loc": len(lines),
        "annotations": class_annotations,
        **({"path": class_path} if class_path else {}),
        "deps": sorted(set(deps)),
        "units": units,
    }


def inventory() -> None:
    if not SRC.exists():
        sys.exit("upstream/soapbox-race-core is missing; run tools/fetch-upstream.sh")
    classes = []
    for pkg, kind in KINDS.items():
        for path in sorted((SRC / pkg).glob("*.java")):
            classes.append(parse_file(path, kind))

    # Port order: dependencies first, then the package order in KINDS.
    by_name = {c["class"]: c for c in classes}
    rank = {pkg: n for n, pkg in enumerate(KINDS)}
    order, done, visiting = [], set(), set()

    def visit(c):
        if c["class"] in done or c["class"] in visiting:
            return
        visiting.add(c["class"])
        for d in c["deps"]:
            if d in by_name:
                visit(by_name[d])
        visiting.discard(c["class"])
        done.add(c["class"])
        order.append(c)

    for c in sorted(classes, key=lambda c: (rank[str(Path(c["file"]).parent)], c["file"])):
        visit(c)

    INVENTORY.parent.mkdir(exist_ok=True)
    INVENTORY.write_text(
        json.dumps({"upstream": f"SoapboxRaceWorld/soapbox-race-core@{UPSTREAM_SHA}", "classes": order}, indent=1)
        + "\n"
    )
    units = sum(len(c["units"]) for c in order)
    print(f"{len(order)} classes, {units} units -> {INVENTORY.relative_to(ROOT)}")


def load():
    inv = json.loads(INVENTORY.read_text())
    decisions = tomllib.loads(DECISIONS.read_text()) if DECISIONS.exists() else {}
    markers = defaultdict(list)
    for rs in (ROOT / "crates").rglob("*.rs"):
        for n, line in enumerate(rs.read_text(encoding="utf-8").splitlines(), 1):
            m = re.match(r"\s*//\s*port:\s*(\S+)", line)
            if m:
                markers[m.group(1)].append(f"{rs.relative_to(ROOT)}:{n}")
    return inv, decisions, markers


def check() -> int:
    inv, decisions, markers = load()
    known = {u["id"] for c in inv["classes"] for u in c["units"]}
    errors = [f"unknown unit in port marker: {k} ({', '.join(v)})" for k, v in markers.items() if k not in known]
    for key, d in decisions.items():
        if key not in known:
            errors.append(f"unknown unit in port/decisions.toml: {key}")
        if d.get("status") not in ("replaced", "dropped"):
            errors.append(f"{key}: status must be 'replaced' or 'dropped'")
        if not d.get("reason"):
            errors.append(f"{key}: a reason is required")
    for e in errors:
        print(e, file=sys.stderr)
    return 1 if errors else 0


def status() -> None:
    inv, decisions, markers = load()
    rows, totals = [], defaultdict(lambda: [0, 0])
    for c in inv["classes"]:
        n = len(c["units"])
        if n == 0:
            continue
        done = sum(1 for u in c["units"] if u["id"] in markers or u["id"] in decisions)
        totals[c["kind"]][0] += done
        totals[c["kind"]][1] += n
        rows.append((c, done, n))
    all_done = sum(d for d, _ in totals.values())
    all_units = sum(n for _, n in totals.values())
    out = [
        "# Port status",
        "",
        f"Generated by `tools/port.py status` from `port/inventory.json` ({inv['upstream']}).",
        "A unit is done when Rust code carries `// port: <unit id>` or `port/decisions.toml` replaces or drops it.",
        "",
        f"**{all_done} / {all_units} units ({100 * all_done // max(all_units, 1)}%)**",
        "",
        "| Kind | Done | Units |",
        "|---|---:|---:|",
    ]
    out += [f"| {k} | {d} | {n} |" for k, (d, n) in sorted(totals.items())]
    out += ["", "Classes in port order (dependencies first):", "", "| Class | Kind | Done | Units | Depends on |", "|---|---|---:|---:|---|"]
    for c, done, n in rows:
        deps = ", ".join(c["deps"]) or ""
        out.append(f"| `{c['file']}` | {c['kind']} | {done} | {n} | {deps} |")
    STATUS.write_text("\n".join(out) + "\n")
    print(f"{all_done}/{all_units} units done -> {STATUS.relative_to(ROOT)}")


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "inventory":
        inventory()
    elif cmd == "status":
        status()
    elif cmd == "check":
        sys.exit(check())
    else:
        sys.exit(__doc__)
