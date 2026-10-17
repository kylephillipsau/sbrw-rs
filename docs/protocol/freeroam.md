# Free-roam: what the game expects on UDP 9999

Every fact below names its source. The real client hasn't been seen talking to
sbrw-rs, so nothing here comes from a capture of our own yet. Sources:

- **go**: WorldUnitedNFS/freeroam at the pinned commit (`upstream/freeroam`), the free-roam server SBRW ran. MPL-2.0; `crates/sbrw-udp/src/{freeroam,carstate}.rs` are ports of it and keep its header.
- **capture**: NFS-World-Offline's `src/freeroam/packets.json`: a handshake reply and 21,162 slot packets a public SBRW server sent one player, which nfswo replays to the stock client. It has no packets from the client.
- **vectors**: what the Go server answered the scripts in `reference/freeroam/` (see The reference rig).

The ModNet patches turn off the game's UDP encryption and CRC (roadmap). Without them the packets are encrypted with `crypto/session_key.go`, which is M9.

## Packets

Integers are big-endian unless noted.

| Packet | Bytes | Source |
|---|---|---|
| Hello, client to server | 58 bytes; byte 2 is `0x06`; the client's tick at 52..54 | go `Server.RunPacketRead` |
| Handshake reply | seq (2), `0x01`, server tick (2), tick difference (2, signed), `49 26 03 01` | go `Client.replyHandshake` |
| Client packet | 16 header bytes with the ack counter at 8..10; subpackets; 5 trailer bytes | go `Client.processPacket` |
| Slot packet, server to client | seq (2), `0x02`, server tick (2), tick difference (2), seq (2), `ff ff 00`, the slots, `01 01 01 01` | go `Client.sendPlayerSlots` |

- A **tick** is a millisecond clock truncated to 16 bits. The difference is `client tick − server tick` at hello, kept for the client's lifetime (go `newClient`).
- **seq** counts the server's packets to that client from 0, the handshake reply included (go `getSeq`).
- The rest of the client's header and the trailer are ignored. What the real client puts at 8..10 is assumed to be the last seq it received; the scripts send that.
- **Subpackets** are type, length (one byte), data: channel info (`0x00`), player info (`0x01`), car state (`0x12`). Other types are skipped (go `processPacket`).
  - Channel info: byte 1 is 1 when the player wants players in its channel first; the channel name, NUL-terminated, starts at byte 2 (`channel.EN__1`, capture).
  - Player info: the persona name, NUL-terminated, at 1..33; the persona id, little-endian, at 41..45 (go; 65 bytes in the capture).
- **A slot** is `ff ff` when empty. Otherwise `00`, subpackets, `ff`:
  - **full**: channel info, player info, car state;
  - **position**: the car state alone;
  - **unchanged**: nothing, so the slot reads `00 ff`.
- Each player's slots are the same width. The default is 14 (go `Config.UDP.MaxVisiblePlayers`), and all of the capture's slot packets have 14.

## Car states (`0x12`)

Bit fields, most significant bit first (go `carstate/`, `binary/better_bitstream.go`):

- 16 bits of sim time, then 12 header bits. The fifth header bit is set for a car on the ground.
- A **ground** state's fields: position y, z, x; linear velocity y, z, x; orientation quaternion y, z, x, w; angular velocity y, z, x; front and rear wheel angles; 13 bits of effect flags.
- An **air** state's fields: yaw, pitch and roll; position y, z, x; linear velocity y, z, x; angular velocity y, z, x.
- Each field is `numBits` wide. A raw value at or above the field's threshold reads one more bit and becomes `extra + 2·raw − threshold`. The value is then `(raw + 0.5) · scale + offset`, and y is negated. The widths, thresholds and constants are in `carstate.rs`.
- Reading past the end gives zero bits, so **every car state of two or more bytes decodes**. The Go server never rejects one, even though `CarPosPacket.Update` panics on a decode error, because that error can't happen. A state of fewer than two bytes panics before it is kept, which drops the rest of that packet.
- The capture's car states are 23 to 26 bytes, with some of 50 to 99. The decoder reads only the first state.
- Go fuses `x·y + z` into one multiply-add on arm64, so arm64 decodes differ in the last bits. amd64 Go, production's platform, rounds each step like Rust, and the vectors come from amd64.

## What the server does

From go `client.go` and `server.go`, confirmed by the vectors:

- **Ready.** A client is shown to others once it has sent channel info, player info and a car state, in any packets and any order. Until then its packets get no answer (go `IsReady`).
- **Who is in view.** These are the other ready players within 300 units of the client in x and y, sorted by distance truncated to a whole number (go `getClosestPlayers`). Ties keep Go's map order, which is random; sbrw-rs keeps hello order.
  - With channel filtering on, players in the client's channel come first.
  - The list is cut to the number of slots, so the nearest 14 are kept.
  - `DisableRadiusSync` drops the radius.
- **Slots change only when the client sends.**
  - A player who left view loses its slot at once.
  - A new one joins a queue. Every 200 ms, on a ticker that starts at the client's hello, the first queued player takes the first free slot (go `startSpawnProcessor`, `processNextPendingPlayer`).
  - A queued player who leaves view stays in the queue. It still takes a slot at its tick and loses it at the client's next packet, which delays the players behind it (vectors `stale_waiting`).
- **What a slot sends.** For each slot in order (go `sendPlayerSlots`):
  1. unchanged, if its full slot has gone out and no car state has arrived since;
  2. position only, once three full slots are in this packet;
  3. full, if it hasn't gone out yet;
  4. position, if the full slot was acked or missed fewer than 5 times;
  5. full again, resetting the miss count.

  A resent identical car state still counts as new: arrival time is what is compared (vectors `two_in_range`).
- **Acks.** At each client packet, every unacked slot is acked if the counter equals the seq its full slot went out in. Otherwise its miss count goes up (`uint8`).
  - A new slot starts with seq 0, so a client acking 0 acks it before anything is sent, and it is never resent (vectors `acks`).
- **Hello again.** Each hello makes a new client, even from the same address. To everyone else the player has gone, and it comes back as a new player once ready (vectors `hello_again`).
- **Timeouts.** Clients silent for 5 s are dropped by a sweep that runs every second (go `RunTimer`).
- **Datagrams** longer than 1,024 bytes are cut to that (go `recvbuf`). sbrw-rs reads the same way.

## Not ported

- **The live map (`fms/`).** It's a websocket on 127.0.0.1:6996 that streams names, positions and rotations, and it is the only user of rotation (quaternion to Euler angles).
- **The persona allow-list.** `AllowedPersonas` kicks a client whose player info names another persona, but freeroamd never sets it.
- **Runtime setters** for the spawn delay and radius sync, which nothing calls.
- **gops and `config.toml`.** The settings are sbrw's `[udp]` section.

## The reference rig

- `carstate_vectors.py` runs `carstate_dump.go` inside the Go module (golang:1.22, linux/amd64), giving 1,025 decodes of real and edge-case car states → `fixtures/vectors/freeroam_carstate.json`.
- `handshake_vectors.py` and `slots_vectors.py` drive a lone client → `freeroam_handshake.json`, `freeroam_slots.json`.
- `multi_vectors.py` drives up to 16 players in nine scenarios → `freeroam_multi.json`.
  - Run it on the same Docker network as freeroamd, so no port proxy merges source addresses.
  - Each player sends only mid-way between its own ticks, positions keep everyone's whole distances distinct, and nothing is observed in the second when a silent player may or may not have been swept. That keeps two recordings identical.
  - Running the same script against `sbrw serve` gives the recorded replies apart from the clock fields.
- `ghost_track.py` takes one player's car states from the capture for the testkit's ghost (`fixtures/freeroam/ghost.json`).

## Unknowns

- What the real client sends in the header bytes other than the ack counter, and in the trailer.
- Whether the real client acks with the last seq it received.
- How often the real client sends. The capture's car states are about 100 ms apart.
