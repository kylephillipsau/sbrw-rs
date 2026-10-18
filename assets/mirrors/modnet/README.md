# ModNet modules

`modules.json` is the SBRW launcher's list of ModNet files and their SHA-256s,
as `https://cdn.soapboxrace.world/launcher-modules/modules.json` served it on
2026-10-18 (last modified there 2023-05-16). The launcher downloads each file
into the game's folder from the same place when its hash differs; the address
is built into the launcher (`URLs.ModNet`), so sbrw-rs can't serve them itself.

`tools/mirror-modnet.sh` downloads them into `files/` (not committed) and
checks each hash. See `docs/vps.md` for using a mirror if the CDN goes away.

## Licences

Checked on 2026-10-18 from each file's version information and its
project's licence:

| File | What it is | Licence |
|---|---|---|
| `ModLoader.asi` | SBRW's ModNet loader, [SoapboxRaceWorld/ModLoader](https://github.com/SoapboxRaceWorld/ModLoader) | MIT, with [bit7z](https://github.com/rikyoz/bit7z) (MPL-2.0) and [Detours](https://github.com/microsoft/Detours) (MIT) built in |
| `dinput8.dll` | [Ultimate ASI Loader](https://github.com/ThirteenAG/Ultimate-ASI-Loader), which loads `ModLoader.asi` | MIT |
| `fmt.dll` | [{fmt}](https://github.com/fmtlib/fmt) | MIT |
| `7z.dll` | 7-Zip (copyright 1999-2018) | LGPL-2.1 |
| `LIBEAY32.dll`, `SSLEAY32.dll` | OpenSSL 1.0.2s | OpenSSL licence |
| `zlib1.dll` | zlib 1.2.11 | zlib licence |
| `global.ini` | the ASI loader's settings | none needed |

Each licence allows redistributing the file with its notice; for 7-Zip and
bit7z, also say where their source is.

## Why they aren't committed

Git would keep 4.6 MB of Windows binaries in the history for good, and
nothing here uses them: the launcher fetches its own copies from the CDN, so
a mirror only matters once the CDN is gone. This repository also can't
vouch for the builds. The CDN's `ModLoader.asi` (2023) is newer than its
published source, last pushed in January 2021, and OpenSSL 1.0.2 has had no
fixes since 2019.

If the CDN looks at risk, publish the checked files as a GitHub Release with
each licence's notice, not in git.
