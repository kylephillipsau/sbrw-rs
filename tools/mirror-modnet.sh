#!/usr/bin/env bash
# Mirror the SBRW launcher's ModNet modules (the client patches that turn
# off the game's UDP encryption and XMPP subject check) from
# cdn.soapboxrace.world into assets/mirrors/modnet/files/, checking each
# against the SHA-256 in assets/mirrors/modnet/modules.json.
#
#   tools/mirror-modnet.sh            fetch the files modules.json lists
#   tools/mirror-modnet.sh --update   fetch the CDN's current modules.json first
#
# The files aren't committed: their licences aren't settled here. Keep the
# folder somewhere safe; if the CDN goes, these are the patches to copy into
# the game's folder by hand (docs/vps.md, "If the ModNet CDN goes away").
set -euo pipefail
cd "$(dirname "$0")/.."

base=https://cdn.soapboxrace.world/launcher-modules
dir=assets/mirrors/modnet
mkdir -p "$dir/files"

if [[ "${1:-}" == "--update" ]]; then
    curl -fsS -m 60 "$base/modules.json" -o "$dir/modules.json"
    echo "updated $dir/modules.json"
fi

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

python3 -c 'import json, sys; [print(k, v) for k, v in json.load(open(sys.argv[1])).items()]' "$dir/modules.json" |
while read -r name expected; do
    case "$name" in */* | .* | "") echo "skipping odd name: $name" >&2; continue ;; esac
    target="$dir/files/$name"
    if [[ -f "$target" && "$(sha256 "$target")" == "$expected" ]]; then
        echo "ok        $name"
        continue
    fi
    curl -fsS -m 300 "$base/$name" -o "$target.part"
    actual="$(sha256 "$target.part")"
    if [[ "$actual" != "$expected" ]]; then
        rm -f "$target.part"
        echo "MISMATCH  $name: got $actual, modules.json says $expected" >&2
        exit 1
    fi
    mv "$target.part" "$target"
    echo "fetched   $name"
done
