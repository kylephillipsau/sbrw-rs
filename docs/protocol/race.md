# Race relay: what the game expects on UDP 9998

This is the clean-room barrier for the race relay. The server SBRW ran has no
license, so sbrw-rs doesn't translate it. These notes describe what it does on
the wire, and `crates/sbrw-udp/src/race.rs` is written from them alone.
Sources:

- **relay**: VladManyanov/sbrw-mp-sync-2018 at the pinned commit (`upstream/sbrw-mp-sync-2018`), a decompiled Netty server from 2018. It was read only to write this file.
- **core**: `LobbyCountdownBO.onTimeout` in soapbox-race-core (GPL-3.0), which makes the tickets; sbrw-rs ports it in `bo/lobby_countdown.rs`.

No capture of the game racing has been seen, so the client's side is only what
the relay reads from it. The ModNet patches turn off the game's UDP encryption
and CRC, as for free-roam.

## Tickets

When a lobby's countdown runs out, core gives each racer a 48-byte relay ticket
in their session (`relayCryptoTicket`, base64) and sends `LobbyLaunched` with
the relay's host and port (`UDP_RACE_IP`, `UDP_RACE_PORT`). The grid is the
lobby's entrants by persona id, highest first.

| Bytes | Field |
|---|---|
| 0 | grid index |
| 1..5 | `0a 0b 0c 0d` |
| 5..9 | event session id, big-endian |
| 9 | number of racers |
| 10..14 | persona id, big-endian |
| 14..48 | zero |

The game puts the ticket in its hello at byte 4, so the relay reads the
session id at 9..13, the racer count at 13 and the persona at 14..18.

## Datagrams

Integers are big-endian. The relay sorts what arrives by its first bytes and
its length:

| From the client | How it's recognised | What the relay reads |
|---|---|---|
| Hello | byte 0 is `00`, byte 3 is `06` | the ticket at 4..52; a 16-byte key at 52..68; the client's time at 69..71 |
| Sync hello | byte 0 `00`, byte 3 `07`, 26 bytes long | a counter at 9..11; a session id at 16..20; byte 20 holds the player index (top three bits) and the player count (bits 1..3) |
| Sync | byte 0 `00`, byte 3 `07`, 22 bytes | the counter at 9..11 |
| Sync keep-alive | byte 0 `00`, byte 3 `07`, 18 bytes | the counter at 9..11 |
| Player packet | byte 0 is `01` | a 10-byte header, subpackets from byte 10, then `ff` and five trailer bytes |
| Player info (a player packet before the racers have met) | byte 0 `01` and bytes 6..10 all `ff` | as a player packet |
| Security | byte 0 is `99` | an 8-byte password at 1..9 and a 16-byte key at 9..25 |

A **subpacket** is type, length (one byte), then that many bytes. In a player
packet the relay looks for player info (`02`) and a car state (`12`).

### What the relay sends

Every reply except the one-byte acknowledgement ends with the four bytes
`01 01 01 01`, where the game would have a CRC.

- **Acknowledgement**: one byte, ASCII `1`, for every datagram, before
  anything else happens to it. For a security packet with the right password
  it's `0` instead.
- **Control packets** (hello and sync replies):
  `00`, the racer's control counter (2 bytes), type, the server's time (2), the
  client time from the racer's hello (2), payload.
  - The control counter counts this racer's control packets, from 1.
  - The server's time is the wall clock in milliseconds, cut to 16 bits.
  - The client time is only set when the racer's hello is answered, so it's
    `0000` in sync replies sent before that.
  - Type `01` is the hello reply, with no payload.
  - Type `02` is a sync reply. Its payload is the sync counter (2), a mask
    (2), then:
    - for sync start: `00 06`, the player index (always `00`), the session id
      from the racer's sync hello (4), the slot bits, `ff`;
    - for sync: `01 03 00 40 b7 ff`;
    - for a keep-alive: `ff`.
  - The sync counter is the client's counter plus one.
  - The mask is all ones except for one bit. When the client's counter is
    between 1 and 16, bit 16 − counter is clear (1 gives `7fff`, 2 gives
    `bfff`, 16 gives `fffe`). Otherwise the mask is `ffff`.
  - The slot bits are one bit per player: for n players from 2 to 7 they are
    2ⁿ − 1 (`03` … `7f`). Any other count gives `ff`. n is the player count
    from that racer's own sync hello (bits 1..3 of byte 20), not the number of
    racers in the session.
- **Relayed player packets**: `01`, the sender's player index, the sender's
  relay counter (2), a second counter (2), `ff ff`, a payload, `ff`. They go to
  every other racer in the session.
  - The player index is the one from the sender's latest sync hello (the top
    three bits of byte 20), or `00` before it has sent one.
  - The relay counter counts this sender's relayed packets, from 1. Each
    relayed packet goes to all the others with the same value.
  - The payload and the second counter are described under
    [Meeting and racing](#meeting-and-racing).

## What the relay does

### Racers and sessions

- **A hello makes a new racer**, with fresh counters and nothing known about
  them yet. The racer joins the session named in its ticket. The session is
  created by its first hello, and its size is that hello's racer count.
- **Racers are known by source port alone**, across all sessions. A later
  datagram from another address with the same port is taken to be the same
  racer, and two racers on the same port overwrite each other. A racer whose
  port changes is a stranger until it says hello again. Then its old self
  stays in the session, which can no longer be full.
- **Every racer** means the session's racers in the order of a Java
  `HashMap` keyed by port. sbrw-rs goes in grid order.
- **Nothing ends.** Racers and sessions are kept until the relay restarts.

### Hello

- When the session has as many racers as its size, every racer in it gets a
  hello reply. This happens again on every later hello into a full session,
  so each racer gets another.
- Without a password set, every ticket is taken. With a password, a hello is
  only taken if a security packet with that password had left a file named
  after its 16-byte key; taking it deletes the file. Core never sends security
  packets.

### Sync

The game then sends sync hellos, syncs and keep-alives. Only racers who have
said hello are answered.

- **A sync hello** is answered at once with sync start. The relay notes the
  racer's player index from byte 20.
  - When every racer in the session has sent one, each of them gets sync start
    again, built from their own sync hello. That includes the racer whose
    sync hello completed the set, so it gets two.
  - The relay keeps each racer's sync hello buffer after Netty has released
    it, and reads the others' again for this round. Netty throws on a released
    buffer, which ends the round part way, or the buffer has been reused for
    another datagram. Which racers get this second sync start is a matter of
    luck. sbrw-rs keeps a copy and sends it to all of them.
- **A sync** is answered with sync; **a keep-alive** with a keep-alive. Both
  echo the counter as above.

### Meeting and racing

A racer is **ready** once the relay has seen both its player info (`02`) and a
car state (`12`), in any of its player-info-shaped packets. Only those are
read for this.

- The relay reads subpackets from byte 10, in at most four rounds, stopping
  at `ff`. Each round takes a `02` if one is next, then a `12` if one is next.
  So `02 12`, `12 02` and `02 12 02 12` are all read, but a subpacket of any
  other type ends the reading: with `10` first, nothing is read. The newest of
  each type is kept, and they can come in different packets.
- **Meeting.** A player-info-shaped packet from a racer, when every racer who
  has said hello is ready, makes every one of them send its details to the
  others. Each sends its player info subpacket. Once that racer has been
  relayed in the racing phase (below), it sends its last car state instead.
  Only player-info-shaped packets are read for these, so "last" means the
  newest `12` in one of those, not in a racing packet.
  - The second counter is `ffff` until the racer is first relayed in the
    racing phase. After that, each racing packet sets it to `0002` and each
    meeting packet adds one.
- **Racing.** Any player packet from a racer, when every racer who has said
  hello is ready, goes to the others. Its payload is the client's bytes from
  10 up to the `ff` and five trailer bytes before the end, so its subpackets
  as sent.
  - The second counter of a racing packet is always `0002`.
  - A player-info-shaped packet does both: meeting first, then racing.
- "Every racer who has said hello" doesn't wait for the session to fill. A
  lone ready racer sends to nobody.

### Errors

The relay's checks read bytes without minding the length. Any read past the
end throws, which stops that datagram (it has already been acknowledged). Netty
reads at most 2,048 bytes of a datagram.

## sbrw-rs differs

These are the original's gaps, and how sbrw-rs closes them. The settings are in
the `[udp.race_relay]` section.

- **Tickets are checked.** A hello is taken only if the world launched its
  event session (`RelayRegistry`), the racer count matches, and the persona has
  that grid index. Anything else gets the acknowledgement and nothing more,
  and is logged. There's no password, key file or security packet. A security
  packet gets the plain `1`.
- **Racers are known by address and port.**
- **Rebinding.** A hello for a persona already in the session replaces that
  racer, wherever it comes from. The old address is forgotten, and the session
  doesn't grow. The original would keep both and never fill again.
- **Waiting for everyone has a limit.** If a session isn't full
  `wait_for_all_secs` after its first hello (default 60, since loading a
  track can be slow), the racers who are there get their hello replies and
  the race goes on without the rest. A racer who says hello later gets its
  reply at once. `wait_for_all_secs = 0` waits forever, as the original
  does.
- **Silent racers stop holding the others up.**
  - A racer silent for `racer_idle_secs` (default 10) is left out when the relay
    asks whether every racer has sent a sync hello, or is ready. It's still
    sent to, and counts again as soon as it sends anything. Nothing would make
    the game say hello again, so the racer isn't dropped.
  - A session with no datagrams for `session_idle_secs` (default 30) since it
    started (its hello replies went out) is dropped, and the registry forgets
    it. A session still waiting for everyone doesn't idle; one waiting forever
    is dropped when the registry forgets it, an hour after the launch.
- **The acknowledgement byte** is kept, since nobody knows whether the game
  needs it. `legacy_ack_byte = false` turns it off.
- **Short or malformed datagrams** are dropped after the acknowledgement, as
  the original's exceptions do. A hello must be at least 71 bytes.
- **Subpacket lengths are unsigned.** The original reads the length byte as
  signed, so a subpacket of 128 bytes or more breaks its reading.
- **Sync start's slot bits** keep the original's table, so a count of 8 gives
  `ff` as it does.

## Unknowns

- What the game sends in the header bytes the relay doesn't read, and in the
  trailer.
- Whether the game needs the acknowledgement byte, or just ignores a one-byte
  datagram.
- What the game does with a sync start when a racer never turned up, which is
  what the wait-for-all limit leads to.
- How often the game sends keep-alives while loading, which decides whether
  10 seconds of silence is too short.
