# sbrw-rs

A Need for Speed World server in one Rust binary and one Docker container.
It replaces the four programs a Soapbox Race World (SBRW) server needs: the
Java core, a forked Openfire XMPP server, the Go free-roam server and the
race relay. Players connect with the standard SBRW launcher.

It is meant for playing with friends, so it keeps everything in SQLite and
one `/data` volume, with no MySQL or Redis to run.

## Status

The core is a 1:1 port of soapbox-race-core. All 870 of its units are
ported or have a recorded decision ([port/decisions.toml](port/decisions.toml)),
and every recorded scenario replays the original's responses, XMPP pushes
and database state exactly. XMPP matches recordings of Openfire, and
free-roam matches the Go server.

Nothing has been run against the game itself yet. Testing with the game
starts with racing a friend on another network; [docs/ROADMAP.md](docs/ROADMAP.md)
has the plan.

It covers:
- accounts, drivers, cars, the garage, the shops and the inventory
- single-player events with their rewards, achievements and the daily treasure hunt
- lobbies, invitations, private lobbies and the race relay
- free-roam, chat, friends, presence, badges and reports
- the in-game /kick, /ban and /unban, and announcements
- invite tickets, password recovery by email and the original's web pages
- for operators, `sbrw admin`, nightly backups, rate limits and a health check

## Running it

Run it natively with `cargo run -- serve`, or in Docker with
`docker compose up --build`. Running natively is easier for development,
since nothing rewrites UDP source addresses. Then add
`http://127.0.0.1:8680/Engine.svc` as a server in the SBRW launcher.

Players on other machines need `server.public_host` set to an address they
can reach. [docs/vps.md](docs/vps.md) covers hosting on a VPS with the
published image, `ghcr.io/kylephillipsau/sbrw-rs:main` (amd64 and arm64).

| Port | Protocol | Used for |
|---|---|---|
| 8680 | TCP | HTTP: launcher and game API, mod downloads |
| 5222 | TCP | XMPP: chat and server pushes |
| 9999 | UDP | Free-roam |
| 9998 | UDP | Race relay |

`--probe-xmpp` and `--probe-udp` replace the XMPP and UDP servers with
probes that record what the game sends and never answer.

## Administration

```sh
sbrw admin ban DRIVER --for 3d --reason "wall riding"   # a driver's name or an account's email
sbrw admin unban player@example.com
sbrw admin kick DRIVER
sbrw admin announce Server restarts in 5 minutes
sbrw admin ticket alice                # an invite ticket, when TICKET_TOKEN is set
sbrw admin set-admin player@example.com            # in-game /kick, /ban, /unban
echo 'new password' | sbrw admin set-password player@example.com
sbrw admin tokens                      # ADMIN_AUTH and the rest, as this run has them
sbrw admin backup                      # a copy of world.db in <data dir>/backups now
```

Ban lengths read as the in-game `/ban` reads them (`1d`, `2h30m`, `45m`),
and a ban without `--for` is permanent. A ban also kicks the player.

The commands go to `/admin/` on the HTTP port, which only answers
connections from the same machine that carry the token `serve` writes to
`<data dir>/admin.token` (readable only by its user) at each start. In Docker, run them with
`docker exec <container> sbrw admin …`.

## Configuration

Settings come from compiled defaults, then `<data dir>/sbrw.toml` (or the
file named by `--config` or `SBRW_CONFIG`), then environment variables such
as `SBRW__SERVER__PUBLIC_HOST`. `sbrw config` prints the result.

```toml
[server]
public_host = "203.0.113.7"   # what clients are told to connect to; use an IP
data_dir = "data"             # /data in the container

[http]
bind = "0.0.0.0:8680"
# public_port = 80            # if a proxy or NAT changes the port players see
auth_failures = 10            # failed logins, registrations and recovery attempts an
auth_window_secs = 300        #   address may make in this window; 0 is no limit

[udp]                         # free-roam, as the Go server's [UDP] settings
visibility_radius = 300.0     # how far apart players see each other
max_visible_players = 14
player_spawn_delay_ms = 200   # one more player comes into view this often
max_clients = 100             # free-roam players at once; 0 is no cap
max_packets_per_sec = 100     # per address, with bursts of twice that; 0 is no limit

[udp.race_relay]              # see docs/protocol/race.md, "sbrw-rs differs"
wait_for_all_secs = 60        # then a race starts with the racers there; 0 waits forever
racer_idle_secs = 10          # a silent racer stops holding the others up
session_idle_secs = 30        # a started race with no traffic this long is over
legacy_ack_byte = true        # answer every datagram with "1", as the original relay

[launcher]
server_name = "My Server"
discord_url = "https://discord.gg/..."
country = "AU"
admins = "..."                # unset fields show the original's defaults: US, ???
owners = "..."

[modding]
enabled = true                # false makes the launcher skip ModNet
server_id = "my-server"

[backup]                      # copies of world.db in <data dir>/backups
nightly_at = "04:00"          # UTC; "" turns nightly backups off
keep = 7                      # nightly copies kept; one before each migration is kept too

[mail]                        # for password recovery links; unset, none are sent
smtp_url = "smtps://me%40gmail.com:app-password@smtp.gmail.com"

# The original server's settings, by its names; defaults are its Data.sql values.
[parameters]
SESSION_LENGTH_MINUTES = "1440"
TICKET_TOKEN = "secret"       # registration then needs an invite ticket
ANNOUNCEMENT_AUTH = "secret"  # lets POST /Engine.svc/SendAnnouncement message everyone
EMAIL_FROM = "me@gmail.com"   # the sender of recovery mails
```

Parameters keep their names as environment variables:
`SBRW_PARAMETER_TICKET_TOKEN=secret`. The `[server]`, `[xmpp]`, `[udp]`,
`[launcher]` and `[modding]` settings, apart from free-roam's, are written
into the parameters at startup (`public_host` becomes `XMPP_IP`,
`UDP_FREEROAM_IP` and so on), so the ported code reads them as the original
did. An entry in `[parameters]` overrides them.

The game data's `ADMIN_AUTH` and `ANNOUNCEMENT_AUTH` are placeholders, so
unless they are set here, each is a fresh random secret at every start.
`sbrw admin tokens` prints them.

The original's web pages are served too. `/forgotPasswd.jsp` asks for a
recovery link, which leads to `/password.jsp`. `/ticket.jsp?ticketAuth=…`,
`/reload-parameters.jsp?adminAuth=…`,
`/reload-login-announcements.jsp?adminAuth=…` and
`/send-announcement.jsp?announcementAuth=…` are forms for the endpoints of
the same names.

Accounts and game state live in `<data dir>/world.db` (SQLite). A backup is
a complete copy: to restore one, stop the server and copy it over
`world.db`.

### Server mods

Files in `<data dir>/mods/` are offered to the launcher through ModNet,
listed in `/mods/index.json` with SHA-1 checksums. Optional `cars.json` and
`events.json` in the same directory override the names the launcher shows
in Discord presence.

### Protocol capture

`SBRW_CAPTURE=http,xmpp,udp-freeroam,udp-race` records traffic into
`<data dir>/captures/<timestamp>/`: `http.jsonl`, `xmpp.log`, and a `.pcap`
(for Wireshark) and `.hexlog` for each UDP server. Captures hold session
tokens, so redact them before sharing.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
./tools/fetch-upstream.sh   # upstream sources at pinned commits, into ./upstream
```

### How it's tested

The expected behaviour is recorded from the original code.
[`reference/`](reference/README.md) runs the original Java core in Docker
against MySQL, with randomness, time and tokens made deterministic, and
records each scenario in `scenarios/` into `fixtures/recorded/`: every
response, every XMPP push and the database state at the end. `sbrw-testkit`
replays the same scenarios against sbrw-rs under the same seed, clock and
token counter, and reports each difference by step and field. Run it with
`cargo test -p sbrw-testkit --test replay`, adding `-- <scenario>` to run
one.

`fixtures/vectors/` holds outputs recorded from the JVM and the Go
free-roam server: JDK behaviour the game logic depends on, such as
`java.util.Random` and `HashMap` order; the original's results for every
reward and achievement script in the game data, every car model's rating
and the event reward arithmetic; and free-roam's answers
([docs/protocol/freeroam.md](docs/protocol/freeroam.md)).

`tools/port.py` lists the original's 870 units, its methods and named
queries. Rust code marks the unit it ports with `// port: <unit id>`, and
`port/decisions.toml` records each deliberate difference
([docs/port-status.md](docs/port-status.md)). `tools/gen.py` generates the
SQLite schema and static data, the route table, error codes, TLD lists and
the XML classes from upstream, and CI checks that they match their inputs.

| Crate | What it does |
|---|---|
| `sbrw` | The binary and CLI |
| `sbrw-common` | Configuration, protocol capture, and the clock, randomness, ids and effect log the game logic uses |
| `sbrw-java` | Bit-exact JDK and Java-library behaviour the port relies on (`java.util.Random`, commons-validator's email check) |
| `sbrw-script` | Runs the game data's JavaScript (achievement and reward scripts) on QuickJS, with the bindings Nashorn gave them |
| `sbrw-testkit` | Replays the recordings against sbrw-rs and reports the differences; a fake free-roam client, and a ghost that replays a real player's captured car states |
| `sbrw-core` | The game logic: upstream's business objects, DAOs and entities, ported 1:1 |
| `sbrw-http` | The launcher and game HTTP API, routed as upstream's RESTEasy did |
| `sbrw-xml` | Upstream's 177 XML classes, generated from JAXB's model, written byte for byte as JAXB writes them and read as leniently as it reads |
| `sbrw-xmpp` | XMPP: STARTTLS, login, rooms, chat and pushes |
| `sbrw-udp` | Free-roam, ported from the Go server, and the race relay |

## Credits and license

sbrw-rs is licensed GPL-3.0-or-later ([LICENSE](LICENSE)). It builds on:
- [soapbox-race-core](https://github.com/SoapboxRaceWorld/soapbox-race-core) (GPL-3.0), the core it ports
- [freeroam](https://github.com/WorldUnitedNFS/freeroam) (MPL-2.0); the files ported from it keep their MPL-2.0 notice
- [GameLauncher_NFSW](https://github.com/SoapboxRaceWorld/GameLauncher_NFSW) (GPL-3.0)
- [NFS-World-Offline](https://github.com/Lawin0129/NFS-World-Offline) (GPL-3.0)
- [setting-up-sbrw](https://github.com/berkayylmao/setting-up-sbrw) (GPL-3.0), the source of the static game data in `assets/static.sql`

The race relay is written from protocol notes
([docs/protocol/race.md](docs/protocol/race.md)). The original relay,
[sbrw-mp-sync-2018](https://github.com/VladManyanov/sbrw-mp-sync-2018), has
no license, so none of its code is used.

This repository contains none of the game client's files. Need for Speed World is a
trademark of Electronic Arts; this project is not affiliated with EA.
