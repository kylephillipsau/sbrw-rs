#!/usr/bin/env bash
# Clone the upstream projects sbrw-rs is based on into ./upstream (gitignored),
# at the exact commits its ports and protocol notes were written against.
#
# Licensing: soapbox-race-core, the launcher, NFS-World-Offline and the setup
# guide are GPL-3.0; freeroam is MPL-2.0. sbrw-mp-sync-2018 has no license: read
# it only to write docs/protocol/race.md, never copy or translate its code.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p upstream

fetch() {
    local name=$1 url=$2 sha=$3 dir=upstream/$1
    if [ -d "$dir/.git" ] && [ "$(git -C "$dir" rev-parse HEAD)" = "$sha" ]; then
        echo "$name: up to date"
        return
    fi
    rm -rf "$dir"
    git init -q "$dir"
    git -C "$dir" remote add origin "$url"
    git -C "$dir" fetch -q --depth 1 origin "$sha"
    git -C "$dir" checkout -q FETCH_HEAD
    echo "$name: $sha"
}

fetch soapbox-race-core https://github.com/SoapboxRaceWorld/soapbox-race-core d3edcd002514eb5265ad44484d79c4d2541ab055
fetch freeroam https://github.com/WorldUnitedNFS/freeroam ced3aca6e6230a59e3bf5fce457e69c3041f9e1c
fetch sbrw-mp-sync-2018 https://github.com/VladManyanov/sbrw-mp-sync-2018 a847f5fc98c17715014962debf3a89f0d881a0ac
fetch GameLauncher_NFSW https://github.com/SoapboxRaceWorld/GameLauncher_NFSW 32d1ceb46238c5b61622a50d56cfcbedaf0a9adf
fetch NFS-World-Offline https://github.com/Lawin0129/NFS-World-Offline 67a4df7933ba0a97dd04258dc175587319ba2fad
# Schema.sql and Data.sql (the static game data) live in "Files/MySQL scripts".
fetch setting-up-sbrw https://github.com/berkayylmao/setting-up-sbrw 2089af624f027ecce968a11aa050f3ace93aff43
