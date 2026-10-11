# sbrw-rs

A Need for Speed World server in one Rust binary and one Docker container. It
replaces the four-part Soapbox Race World (SBRW) stack: the Java core, the
forked Openfire XMPP server, the Go free-roam server and the race relay. It
works with the standard SBRW launcher.

It's built for playing with friends: SQLite instead of MySQL, no Redis, one
`/data` volume.

**Status:** M2 in progress. Every call the launcher and the game make up to
the driver screen is ported and replays recordings of the original exactly,
including creating and selecting a driver. Milestones are verified against
recordings of the original and the other reference implementations, not with
the real game, so nothing here has been run against the game itself yet. See
[docs/ROADMAP.md](docs/ROADMAP.md) for the design and milestones, and
[docs/port-status.md](docs/port-status.md) for how much of the original is ported.

| Milestone | Status |
|---|---|
| M0 Foundations: launcher sees the server, ModNet, protocol capture, probes | done |
| M1 Register, log in, game boots | done |
| M2 Create a driver, XMPP, spawn in the world | in progress |
| M3 Free-roam multiplayer | |
| M4 Garage and economy | |
| M5 Single-player events, rewards, achievements | |
| M6 Lobbies and race relay | |
| M7 Social and moderation | |
| M8 Operations and real multiplayer | |
| M9 Unpatched client (optional) | |

## Running it

Natively, which is the easiest way to develop because nothing rewrites UDP
source addresses:

```sh
cargo run -- serve --probe-xmpp --probe-udp
```

With Docker:

```sh
docker compose up --build
```

Either way, add `http://127.0.0.1:8680/Engine.svc` as a server in the SBRW
launcher. Players on other machines need `server.public_host` set to an address
they can reach (see below).

| Port | Protocol | Used for |
|---|---|---|
| 8680 | TCP | HTTP: launcher and game API, mod downloads |
| 5222 | TCP | XMPP: chat and server pushes |
| 9999 | UDP | Free-roam |
| 9998 | UDP | Race relay |

`--probe-xmpp` and `--probe-udp` bind those ports and record what the game
sends without ever answering. They are stand-ins until the real servers land.

## Configuration

Settings come from compiled defaults, then `<data dir>/sbrw.toml` (or the file
named by `--config` / `SBRW_CONFIG`), then environment variables of the form
`SBRW__SECTION__KEY`. `sbrw config` prints the effective configuration.

```toml
[server]
public_host = "203.0.113.7"   # what clients are told to connect to; use an IP
data_dir = "data"             # /data in the container

[http]
bind = "0.0.0.0:8680"
# public_port = 80            # if a proxy or NAT changes the port players see

[launcher]
server_name = "My Server"
discord_url = "https://discord.gg/..."
country = "AU"
admins = "..."                # unset fields show the original's defaults: US, ???
owners = "..."

[modding]
enabled = true                # false makes the launcher skip ModNet
server_id = "my-server"

# The original server's settings, by its names; defaults are its Data.sql values.
[parameters]
SESSION_LENGTH_MINUTES = "1440"
TICKET_TOKEN = "secret"       # registration then needs an invite ticket
```

The `[server]`, `[xmpp]`, `[udp]`, `[launcher]` and `[modding]` settings are
written into the original's parameters at startup (`public_host` becomes
`XMPP_IP`, `UDP_FREEROAM_IP` and so on), so the ported code reads them as the
original did. An entry in `[parameters]` wins over all of them.

Accounts and game state live in `<data dir>/world.db` (SQLite).

The same values as environment variables are `SBRW__SERVER__PUBLIC_HOST`,
`SBRW__LAUNCHER__SERVER_NAME`, and so on; parameters keep their names:
`SBRW_PARAMETER_TICKET_TOKEN=secret`.

### Server mods

Files in `<data dir>/mods/` are offered to the launcher through ModNet. The
index the launcher reads (`/mods/index.json`) is generated from the directory,
with SHA-1 checksums. Optional `cars.json` and `events.json` there override the
launcher's Discord presence names.

### Protocol capture

`SBRW_CAPTURE=http,xmpp,udp-freeroam,udp-race` records traffic into
`<data dir>/captures/<timestamp>/`:
- `http.jsonl`
- `xmpp.log`
- a `.pcap` (opens in Wireshark) and `.hexlog` for each UDP server

Captures include session tokens, so don't share them unredacted.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
./tools/fetch-upstream.sh   # upstream sources at pinned commits, into ./upstream
```

### How the port works

sbrw-rs is a 1:1 port of soapbox-race-core. Its expected behaviour is recorded from the original code:

- **[`reference/`](reference/README.md)** runs the original Java core in Docker against MySQL. Randomness, time and tokens are made deterministic. Each scenario in `scenarios/` is recorded into `fixtures/recorded/`: every response, every XMPP push, and the database state at the end. sbrw-rs replays the same scenarios and has to produce the same output.
- **`sbrw-testkit`** replays every scenario against sbrw-rs under the same seed, clock and token counter, and reports each difference by step and field. Run it with `cargo test -p sbrw-testkit --test replay` (add `-- <scenario>` to pick one, or `SBRW_REPLAY_REPORT=1` to see differences for pending scenarios). Scenarios not reproduced yet are listed in `port/replay-pending.toml`; the test fails when one starts passing, so the list only shrinks.
- **`tools/gen.py`** generates code from upstream and from JVM recordings: the SQLite schema and static data, the route table, error codes, TLD lists, and the XML classes. CI checks that the generated files match their inputs.
- **`fixtures/vectors/`** holds JVM outputs for the JDK behaviour the game logic depends on, such as `java.util.Random`.
- **`tools/port.py`** inventories every method and named query in the original (870 units) and tracks which are ported. Rust code marks the unit it ports with `// port: <unit id>`, and `port/decisions.toml` lists deliberate deviations. Progress is in [docs/port-status.md](docs/port-status.md).

| Crate | What it does |
|---|---|
| `sbrw` | The binary and CLI |
| `sbrw-common` | Configuration, protocol capture, and the clock, randomness, ids and effect log the game logic uses |
| `sbrw-java` | Bit-exact versions of JDK and Java-library behaviour the port relies on (`java.util.Random`, commons-validator's email check) |
| `sbrw-script` | Runs the JavaScript in the game data (achievement and reward scripts) on QuickJS, with the bindings shown to scripts as Nashorn showed them Java objects |
| `sbrw-testkit` | Replays reference recordings against sbrw-rs and reports the differences |
| `sbrw-core` | The game logic: upstream's business objects, DAOs and entities, ported 1:1 |
| `sbrw-http` | The launcher and game HTTP API, routed as upstream's RESTEasy did |
| `sbrw-xml` | Upstream's 177 XML classes, generated from JAXB's own model; written byte for byte as JAXB writes them and read as leniently as JAXB reads |
| `sbrw-xmpp` | XMPP (probe only so far) |
| `sbrw-udp` | Free-roam and race UDP (probe only so far) |

## Credits and license

GPL-3.0-or-later. sbrw-rs is a reimplementation informed by:
- [soapbox-race-core](https://github.com/SoapboxRaceWorld/soapbox-race-core) (GPL-3.0)
- [GameLauncher_NFSW](https://github.com/SoapboxRaceWorld/GameLauncher_NFSW) (GPL-3.0)
- [freeroam](https://github.com/WorldUnitedNFS/freeroam) (MPL-2.0; ported files keep their MPL-2.0 notice)
- [NFS-World-Offline](https://github.com/Lawin0129/NFS-World-Offline) (GPL-3.0)
- [setting-up-sbrw](https://github.com/berkayylmao/setting-up-sbrw) (GPL-3.0)

The race relay is implemented from written protocol notes, not from
[sbrw-mp-sync-2018](https://github.com/VladManyanov/sbrw-mp-sync-2018), which has
no license. Need for Speed World is a trademark of Electronic Arts; this project
is not affiliated with EA.
