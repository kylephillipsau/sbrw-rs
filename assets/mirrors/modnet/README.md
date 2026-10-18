# ModNet modules

`modules.json` is the SBRW launcher's list of ModNet files and their SHA-256s,
as `https://cdn.soapboxrace.world/launcher-modules/modules.json` served it on
2026-10-18 (last modified there 2023-05-16). The launcher downloads each file
into the game's folder from the same place when its hash differs; the address
is built into the launcher (`URLs.ModNet`), so sbrw-rs can't serve them itself.

`tools/mirror-modnet.sh` downloads them into `files/` (not committed) and
checks each hash. See `docs/vps.md` for using a mirror if the CDN goes away.
