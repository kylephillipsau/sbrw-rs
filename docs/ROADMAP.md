# sbrw-rs: one-container Rust replacement for the SBRW server stack

## Context
A Need for Speed World private server (Soapbox Race World, SBRW) is four separate programs:
- `soapbox-race-core`: Java EE on Thorntail. 429 files, ~43k lines of code. Last commit 2021-01.
- A forked Openfire XMPP server plus two plugins.
- A free-roam UDP server written in Go.
- A race-sync UDP relay: decompiled Java with no license.

They need MySQL and JDK 10 (end of life), and they're installed with Windows `.bat` files ([berkayylmao/setting-up-sbrw](https://github.com/berkayylmao/setting-up-sbrw)). The goal is a from-scratch Rust server that replaces all of it: one binary in one Docker container, for the owner and a few dozen friends. It was meant to be tested with the real game client under CrossOver on an Apple Silicon Mac. No client is available, so it is verified against reference implementations instead (see Verification approach).

**Fixed decisions**
- **Database:** embedded SQLite in a single `/data` volume.
- **Repo:** new repo at `~/dev/sbrw-rs`, licensed GPL-3.0-or-later. Upstream core, launcher and NFS-World-Offline are GPL-3.0. Free-roam is MPL-2.0, so ported files keep MPL headers. The race relay has no license, so it is reimplemented from written protocol notes and never translated.
- **Client target:** the SBRW launcher plus ModNet-patched client first. The launcher downloads patches from `cdn.soapboxrace.world` (still live on 2026-10-04), and they turn off the game's UDP encryption/CRC and the XMPP subject check. Support for the unmodified client is the optional M9.

## How it works today: protocol facts the design depends on
- **HTTP:**
  - 95 endpoints under `/Engine.svc`: 67 real, 7 partial, 21 stubs.
  - **Any unknown path must return 200 with an empty XML body.** The game breaks on 404.
  - XML has no `<?xml?>` declaration and no namespaces. The root element is the JAXB type name, element casing varies by class, and element order is JAXB's `propOrder`. Namespaces are stripped from input.
  - Errors are HTTP 503 with an `<EngineExceptionTrans>` body.
  - Every response carries `Connection: close` and is gzipped.
- **Auth flow:**
  1. The launcher calls `User/authenticateUser`. The password is a lowercase SHA-1 hex string, and the reply is `LoginStatusVO`, where an empty `Description` means success.
  2. The launcher runs `nfsw.exe <ID> <url>/Engine.svc <token> <userId>`.
  3. The game calls `GetPermanentSession`, which **swaps** the token.
  4. After that, every request is authenticated with the `userId` and `securityToken` headers.
- **Launcher requirements:**
  - `/Modding/GetModInfo` must be a real route. A 404 means "ModNet off"; a catch-all 200 breaks the launcher.
  - `GetServerInformation` must send `modernAuthSupport: false`.
- **XMPP:**
  1. A plaintext stream offers only STARTTLS.
  2. TLS works with OpenSSL set to TLS 1.0 minimum and `DEFAULT:@SECLEVEL=0`. rustls can't do this.
  3. After TLS, the server advertises **no SASL**, so the client uses legacy `jabber:iq:auth` with user `sbrw.<personaId>`, resource `EA-Chat`, and the first 16 characters of the session token as the password.
  4. Chat uses MUC rooms: public `channel.<SHORT>__<n>@conference.<domain>` and client-created `group.channel.*`.
  5. The server pushes 15 payload types (lobby, results, powerups, achievements, presence, chat, kick) as escaped XML in `<message>`, wrapped in `<response status="1" ticket="0">`. Each has a `<subject>` that is a 64-bit hash of (to + body), per nfswo `calculateHashFromData.js`.
- **UDP free-roam (9999):**
  - 58-byte hello, then an 11-byte handshake reply.
  - TLV packets: 0x00 channel, 0x01 player, 0x12 car state.
  - The server replies to every client packet with a 14-slot visibility packet: 300-unit radius, 200 ms spawn queue, ack/resend.
  - The CRC is fake (`01 01 01 01`).
- **UDP race (9998):**
  - The hello carries a 48-byte ticket: grid index, `0a0b0c0d`, sessionId, numRacers, personaId.
  - Control packets wait until all racers have said hello. Packet type 0x07 is identified by length (26/22/18 bytes).
  - Peer packets have a "before" phase (exchange player info) and an "after" phase (relay to the others). No crypto.
- **Data:**
  - 52 tables. The static game data in `3. Data.sql` is ~4 MB, e.g. product 3712 rows, basketdefinition 382 rows of car XML, event 165.
  - **The data includes JavaScript:**
    - ~1,300 reward and card-pack scripts built from ~10 `generator.*` functions;
    - achievement triggers that call Java getters, e.g. `!eventSession.getLobby().getIsPrivate()`.
  - The ~100 `parameter` keys are the server config. Redis was only used for presence and the matchmaking queue, so that becomes in-memory state.

## Architecture
```
one process `sbrw serve`
 HTTP (axum) ─┐                       ┌─ rusqlite /data/world.db (WAL)
 XMPP (5222) ─┼─ call(FnOnce(&mut World)) ─→ World actor thread ─┼─ rquickjs (data scripts)
 timers/jobs ─┘                       └─ in-memory sessions / lobbies / queue
 World ──Outbound pushes──→ XMPP hub      World ──RelayRegistry──→ UDP race (9998)
 UDP free-roam (9999): own task, no per-packet world calls
```
- **One "world" actor thread owns all mutable game state.** HTTP, XMPP and timers send closures to it and await the result. Each call is one SQLite transaction, with `catch_unwind` to roll back on panic.
  - Why: a single SQLite writer, QuickJS (which isn't `Send`) stays on one thread, and the ported game logic is plain synchronous, testable Rust.
  - XML encoding and decoding happen outside the actor.
- **Sessions are in memory, as upstream.** A container restart logs everyone out, and in-progress lobbies and races are lost. That's an accepted trade-off for a friends server; player data in SQLite is unaffected.
- **Pushes** are queued during a transaction and sent after commit through a channel to the XMPP hub.
- **The XMPP hub** answers `is_online` and `group_room_members` for the world through a `PresenceView` trait.
- **Timers** run in one tokio `DelayQueue` task: lobby countdown, DNF timeout, lobby expiry. Generation counters make stale timers no-ops. Periodic jobs: every minute (expired bans, expired items, expired rentals, online stats), every 30 minutes (achievement rarity), nightly (backup).

**Workspace** (edition 2024, Rust 1.97):

| Crate | Responsibility |
|---|---|
| `sbrw` | Binary; clap CLI (`serve`, `healthcheck`, `gen-cert`, `user …`, `admin …`, `config check`, `capture redact`) |
| `sbrw-common` | Config (figment: defaults → `/data/sbrw.toml` → `SBRW__*` env), legacy parameter-key table, `EngineExceptionCode`, capture sink, `RelayRegistry`, `Clock` |
| `sbrw-xml` | `XmlWrite`/`XmlRead`/`XmlRoot` traits on the quick-xml event API (not serde), lenient namespace-stripping reader, JAXB scalar formatting, generated `http.rs`/`xmpp.rs` (committed) |
| `sbrw-script` | rquickjs runtime; prelude with a bean shim (`x.foo` and `x.getFoo()` both work); `generator.*` returns *descriptors* that Rust resolves with a seeded RNG against `StaticData` |
| `sbrw-core` | `StaticData` (in memory), store (rusqlite + rusqlite_migration), one `domain/` module per upstream BO, world actor, sessions/lobbies/matchmaking, Outbound, timers, jobs |
| `sbrw-http` | axum routes; `Auth` and `XmlBody<T>` extractors; `Xml<T>`/literal responses; EngineError → 503; catch-all; launcher JSON; ModNet files; capture middleware; `/health` |
| `sbrw-xmpp` | Stanza framer, stream state machine, OpenSSL acceptor, iq:auth, presence, MUC subset, hub |
| `sbrw-udp` | `PacketCodec` seam (`Plain` now, `NfswCrypto` for M9), free-roam (Go port), race relay (from the spec) |
| `sbrw-testkit` | In-process server fixture; FakeLauncher/FakeGame/FakeXmpp/FakeFreeroam/FakeRacer; scenarios |

**Other key libraries:**
- tokio, tower-http (gzip compression and decompression)
- openssl and tokio-openssl, vendored
- rusqlite (bundled)
- rquickjs (bundled quickjs-ng)
- RustCrypto `aes`, `cfb-mode`, `md-5` and `hmac`, for the M9 UDP crypto
- argon2: stored passwords are `argon2id(sha1hex)`
- tracing, uuid, rand (seedable `StdRng`), insta, proptest, cargo-deny

## Subsystem designs
- **XML DTOs: generated, not hand-written.**
  - `tools/gen_dtos.py` reads the vendored JAXB Java classes. The XSDs don't have SBRW's renames.
  - Rules: root = `@XmlRootElement`, else `@XmlType` name. Element = `@XmlElement(name)`, else the field name. Order = parent's `propOrder`, then the class's own. Primitives are always emitted; boxed and object fields become `Option` and are omitted when `None`. `ArrayOfX` stays its own struct, so "absent" and "empty" are different. Enums, `@XmlAttribute` and `@XmlElementWrapper` are handled.
  - The output for the 28 hand-written xmpp classes gets a manual review.
  - **Verification:** a Maven project in Docker (`maven:3.9-eclipse-temurin-21`) fills every JAXB type with deterministic values and writes golden XML files. Rust tests check byte-for-byte equality and round-trips. Corpus tests parse all 382 basket XML blobs and nfswo's 425 XML files.
- **Data:**
  - `tools/convert_mysql_dump.py` converts `Data.sql` to SQLite syntax as `assets/static.sql` (handles `_binary` and MySQL escapes), which is committed and embedded with `include_str!`.
  - At boot it's loaded into an in-memory SQLite, then `/data/static-overrides.sql` if present, then copied into a typed `Arc<StaticData>` with indexes and pre-parsed basket DTOs.
  - Player data lives in `/data/world.db`: about 31 runtime tables hand-ported, timestamps in unix milliseconds, persona ids starting at 100.
  - Before every migration and nightly, `VACUUM INTO /data/backups/`.
  - Tests assert static row counts.
- **Config:**
  - Typed sections: server, http, xmpp, udp, session, economy, pursuit, treasure_hunt, launcher (the `SERVER_INFO_*` keys), modding, admin, capture.
  - `tools/extract_params.py` turns every `get*Param` call site plus the `parameter` rows into `docs/parameters.md` and a legacy key table. SBRW `parameter` values can then be pasted under `[legacy_parameters]`.
  - `public_host` (default `127.0.0.1`) drives the advertised XMPP, UDP and ModNet addresses.
- **XMPP:**
  - **Framer:** quick-xml with end-name checks off; it tracks nesting depth and caps stanzas at 64 KiB. An optional auto-close fallback exists for the case where the client sends broken stanzas; captures will decide whether it's needed.
  - **TLS:** `xmpp.tls = required|offered|disabled`. OpenSSL is set to security level 0, TLS 1.0 minimum, and `SslOptions::ALL | DONT_INSERT_EMPTY_FRAGMENTS | NO_COMPRESSION`. Cipher list and maximum version are configurable. A self-signed RSA-2048 certificate is created in `/data/tls/` on first start. The raw ClientHello is logged.
  - **Auth:** iq:auth `get` returns the Openfire/nfswo shape, including `clientlock`. `set` accepts a plaintext password or `digest = sha1(streamId + pw)`. One connection per persona; a newer one kicks the older.
  - **Routing:** route on the localpart and accept any domain.
  - **MUC:** join, leave, presence fan-out (with status 110 for self), and groupchat relay. `group.channel.*` rooms are created on first join. One-to-one `chat` messages are relayed. Unknown iq gets a configurable reply and is always logged.
  - **Subject hash:** computed over UTF-16 code units, printed as a signed i64. A config toggle switches to SBRW's constant subject.
- **UDP:**
  - Each server has one socket and one task that owns its state. The `PacketCodec` seam handles whole datagrams.
  - **Free-roam:** a faithful port of the Go constants and slot logic, plus a rate limit, a client cap, and logging of unknown TLVs.
  - **Race:**
    - Implemented from `docs/protocol/race.md`, written before the code.
    - Each hello's ticket is checked against the in-process `RelayRegistry` (live session, matching grid slot and persona). That replaces the old keys and 0x99 mechanism and stops forged sessions.
    - Fixes the old server's gaps: players keyed by address and port (not port alone), rebinding when a racer's address changes, a configurable timeout on the wait-for-all step, and per-player (10 s) and per-session (30 s) idle timeouts.
- **Protocol capture (built in M0, used throughout):**
  - Turned on with `SBRW_CAPTURE=http,xmpp,udp-freeroam,udp-race`. It writes to `/data/captures/<ts>/`:
    - `http.jsonl` (bodies before gzip);
    - `xmpp.log` (plaintext before TLS, decrypted after, plus a ClientHello summary);
    - `.pcap` files and annotated hex logs for UDP.
  - `sbrw capture redact` strips tokens before captures become test fixtures.
  - Probe mode (`--probe-xmpp`, `--probe-udp`) logs connections without answering them.
- **Docker:**
  - Multi-stage build: `rust:1.97-bookworm` builds the binary; the runtime image is `gcr.io/distroless/cc-debian12:nonroot`. Native arm64 locally; CI builds amd64 and arm64 on native runners and merges the manifests.
  - Ports: `EXPOSE 8680/tcp 5222/tcp 9998/udp 9999/udp`. `/data` volume, owned by uid 65532.
  - `HEALTHCHECK` runs `sbrw healthcheck`. A `compose.yaml` is included.
  - The everyday dev loop is native `cargo run -- serve`, because Docker Desktop's UDP proxy rewrites source ports. Docker is for packaging and the VPS, where Linux DNAT keeps the real source addresses.

## Milestones
Each milestone has automated tests and is verified against a reference implementation (see Verification approach). The real-client checks below are not being run: there is no client to run them with (decided 2026-10-11), so they stay as the eventual checklist. Without captures, endpoints are built in the order NFS-World-Offline's routes and data show the client calling them; the catch-all covers the rest.

| # | Scope | Real-client check | Automated tests |
|---|---|---|---|
| **M0** Foundations | **First, a client sanity check:** confirm the SBRW launcher and `nfsw.exe` run under CrossOver by joining a public SBRW server once. After that, any failure is ours rather than Wine's. Then: workspace, CI (fmt, clippy, test, cargo-deny), config, tracing, capture sink, `tools/fetch-upstream.sh` (pinned commits below). HTTP: catch-all, `GetServerInformation`, `Modding/GetModInfo`, `{basePath}/{cars,events,index}.json`, mounted at `/Engine.svc`. XMPP and UDP in probe mode. Dockerfile and compose. | Add `http://127.0.0.1:8680/Engine.svc` in the launcher: it shows online with our name, and ModNet installs cleanly. | Server-info JSON shape; catch-all; `GetModInfo` 200/404 switch |
| **M1** Login and boot | DTO generator and JAXB goldens. User create and auth (argon2id), the `GetPermanentSession` token swap, header auth, EngineException → 503. Boot endpoints (systeminfo, getusersettings, carclasses, GetChatInfo, getrebroadcasters, cryptoticket, getregioninfo, news, social stubs). **E1:** capture when and how the client first touches 5222. | Log in through the launcher; `nfsw.exe` reaches the driver create/select screen. | Golden DTO tests; insta snapshots of the boot sequence; FakeLauncher+FakeGame scenario |
| **M2** Driver, XMPP, spawn | Static-data converter and `StaticData`; world.db V001; personas (create/delete/select/name check), car slots and default car, starter car, inventory. Full XMPP (STARTTLS, iq:auth, presence, public MUC, welcome push). Free-roam handshake with empty slots. **E2:** adjust TLS settings if the handshake fails. | Create a driver, spawn in Tri-City, drive 5+ minutes, see the welcome message, see chat echo. | Static row counts; FakeXmpp with an OpenSSL client pinned to TLS 1.0 + AES128-SHA (auth, room join, push, subject check); subject-hash vectors from node |
| **M3** Free-roam MP | Full port of the Go server. | A testkit "ghost" replaying a captured car stream is visible and moving in-game. | N-client simulations (slots, spawn queue, acks, timeouts); differential test against Go `freeroamd` in a container |
| **M4** Garage and economy | Catalog, basket (car, bundle, card pack, amplifier, powerup, repair), selling, commerce customisation, car rating/class, car damage, inventory, powerup activation push, rquickjs and the descriptor generator, COMMERCE achievements. | Buy, paint and vinyl a car; parts change its rating; sell; open a card pack; use a powerup. | Seeded-RNG domain tests; rating cases from `car_classes`; all ~1,300 reward scripts evaluate without error |
| **M5** SP events | Event list and launch; arbitration (route, drag, pursuit, team escape); legit-race checks; reward math (int truncation at every step, transcribed line by line from `RewardBO`); level-up; item rewards; DNF; durability; achievements push; treasure hunt. | Sprint, circuit, drag and pursuit each pay out; level up; achievement toast; collect treasure-hunt gems. | Table tests transcribed from the Java; all 67 achievement triggers evaluated against fixture contexts |
| **M6** Lobbies and race | Race-now queue, lobbies, invites, private lobbies (via group rooms), countdown → 48-byte tickets → `RelayRegistry` → `LobbyLaunched`, race relay, MP results pushes, EventTimingOut/TimedOut. | The real client plus one FakeRacer fill a lobby, race, and both appear on the results screen. | In-process 2-racer scenario; tests for the wait-for-all timeout, ticket rejection and rebinding |
| **M7** Social and moderation | Friends, block, presence pushes, parties, whispers, reports, motto, badges, news, admin broadcast, kick. | Friend request and presence with a fake client; party invite; broadcast; kick. | Scenario tests |
| **M8** Ops and real MP | Admin CLI (`docker exec … sbrw admin`, via a localhost-only token API), bans, backups, `/health`, auth rate limits, invite tickets, multi-arch image, VPS runbook. Mirror the ModNet modules with sha256s into `assets/mirrors/modnet`. | Run on a VPS from the published image; race a friend on another network. | Docker smoke test in CI |
| **M9** (optional) Stock client | `GetModInfo` → 404; try plaintext first (nfswo replays plaintext free-roam to the stock client); then wire up `NfswCrypto` (ported `session_key.go`, tested against `crypto_test.go` vectors). | An unpatched client spawns and races. | Crypto vectors |

Milestone status is tracked in the README.

## Verification approach
- **Fast loop:** `cargo test --workspace`, i.e. unit tests, golden XML, vectors, the script corpus and testkit scenarios.
- **Reference per layer (the real client isn't available, so these are the oracles):**
  - HTTP and game logic: recordings of the original core from the `reference/` rig, replayed by `sbrw-testkit`.
  - Script semantics: Nashorn results recorded by the rig as `fixtures/vectors/`.
  - XMPP: NFS-World-Offline's XMPP server (built against the real client) and the Openfire fork for push shapes, plus an OpenSSL client pinned to TLS 1.0. Whether the real client completes the TLS handshake stays unverified until a client is available (top risk 1).
  - UDP free-roam: a differential test against the Go `freeroamd` in a container.
- **Real client (not run; kept for when one is available):** `cargo run -- serve` natively with `SBRW_CAPTURE=…`, then the SBRW launcher under CrossOver pointed at `http://127.0.0.1:8680/Engine.svc`. Captures from each session are redacted and committed as regression fixtures.
- **Packaging:** `docker compose up` on the Mac, then the same checklist. The final check is on a Linux VPS with a second player.
- **Only one real client:** testkit fakes replay *captured real traffic* (ghost driver, FakeRacer) until a friend joins at M8. One attempt at a second CrossOver bottle in M3.

## Top risks
1. **Legacy TLS handshake failure.** The ClientHello is captured in M0/M1; every TLS setting is configurable; nfswo's OpenSSL settings are a known-good reference; the CI client is pinned to TLS 1.0.
2. **XML fidelity drift.** Codegen from the Java, byte-exact JAXB goldens, capture fixtures, and the launcher's Discord XPath shapes as an extra check.
3. **Game-math drift** (int truncation, Nashorn semantics). Line-by-line ports linked to the Java source; table tests; seeded RNG.
4. **Race relay unknowns and licensing.** The written spec is the clean-room barrier, filled in from captures, with config toggles (`legacy_ack_byte`, what happens when the wait-for-all step times out).
5. **ModNet CDN disappears.** Mirror the modules (M8); M9 is the fallback.
6. **Scope** (95 endpoints, ~43k lines of Java). Build in capture order; the catch-all keeps everything else harmless; `docs/endpoints.md` tracks real/partial/stub status.

## Upstream references (pinned in `tools/fetch-upstream.sh`, cloned into a gitignored `upstream/`)
- SoapboxRaceWorld/soapbox-race-core `d3edcd0025`
  - API: `core/api/*.java`
  - Game logic: `core/bo/{RewardBO,ItemRewardBO,AchievementBO,BasketBO,CommerceBO,LobbyCountdownBO,TokenSessionBO,UserBO}.java`
  - DTOs: `jaxb/{http,xmpp}/`, `jaxb/util/JAXBUtility.java`
- WorldUnitedNFS/freeroam `ced3aca6e6`: `client.go`, `server.go`, `carstate/`, `crypto/session_key.go`
- VladManyanov/sbrw-mp-sync-2018 `a847f5fc98`: read only to write `docs/protocol/race.md`
- SoapboxRaceWorld/GameLauncher_NFSW `32d1ceb462`: `Client/Auth/Authentication.cs`, `ModNet/ModNetHandler.cs`, `MainScreen.cs` (launch arguments, ModNet), `Proxy/`
- Lawin0129/NFS-World-Offline `67a4df7933`: `src/xmpp/*.js`, `src/utils/calculateHashFromData.js`, `src/freeroam/packets.json`
- This repo: `Files/MySQL scripts/{2. Schema.sql,3. Data.sql}` (fetched by `tools/fetch-upstream.sh`)
