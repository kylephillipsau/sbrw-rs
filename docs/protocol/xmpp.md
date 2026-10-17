# XMPP: what the game expects on port 5222

Every fact below names its source. The real client hasn't been seen talking to
sbrw-rs, so nothing here comes from a capture yet. Sources:

- **core**: soapbox-race-core at the pinned commit (`upstream/soapbox-race-core`).
- **fork**: [SoapboxRaceWorld/openfire](https://github.com/SoapboxRaceWorld/openfire), the XMPP server SBRW ran. It is Openfire's `master` of late July 2019 plus two commits (`9ae8d25a43`, `028873a146`), both to `SASLAuthentication`.
- **nonsasl**: [igniterealtime/openfire-nonSaslAuthentication-plugin](https://github.com/igniterealtime/openfire-nonSaslAuthentication-plugin), which SBRW's setup guide installs for `jabber:iq:auth`.
- **restapi**: [SoapboxRaceWorld/openfire-restAPI-plugin](https://github.com/SoapboxRaceWorld/openfire-restAPI-plugin), the REST plugin the core drives.
- **guide**: berkayylmao/setting-up-sbrw (`upstream/setting-up-sbrw`), the Openfire install and launch settings.
- **nfswo**: NFS-World-Offline (`upstream/NFS-World-Offline/src/xmpp`), a single-player server written against the stock client.

## Who connects

| Client | JID | Resource | Password | Source |
|---|---|---|---|---|
| The game | `sbrw.<personaId>@<XMPP_IP>` | `EA-Chat` | first 16 characters of the session token | core `UserBO.createXmppUser`, `OpenFireRestApiCli.getAllPersonaByGroup` |
| The core | `sbrw.engine.engine@<XMPP_IP>` | `EA_Chat` | `OPENFIRE_TOKEN` | core `OpenFireConnector` |

- The game gets the host, port, prefix (`sbrw`) and rooms from `GetChatInfo` (core `Session.getChatInfo`).
- The core creates or updates an Openfire account for every persona of the user at `User/GetPermanentSession`, with the new token's first 16 characters as the password (core `User.getPermanentSession`, `UserBO.createXmppUser`). The rig records those calls as `openfire` side effects.
- In sbrw-rs the core and the XMPP server are one process, so the engine account isn't a real connection. Accounts aren't stored either: the password is checked against the live session.

## Stream and TLS

1. The game opens a plaintext stream. The server answers with features offering STARTTLS (nfswo; stock Openfire does the same).
2. TLS: SBRW ran Openfire on JDK 10 with `-Djsse.enableCBCProtection=false` (guide `sbrwopenfirelauncher.bat`). That flag stops Java splitting TLS 1.0 CBC records into 1/n−1 pieces. Someone turned it off on purpose, which suggests the game's TLS stack mishandles split records. It matches the roadmap's `DONT_INSERT_EMPTY_FRAGMENTS`. nfswo serves TLS from node with `ciphers: "DEFAULT@SECLEVEL=0", minVersion: "TLSv1"`.
3. After TLS the game restarts the stream. The fork's features list **leaves out `<mechanisms>`** unless the stream's `from` contains `.engine.engine`, so the game falls back to `jabber:iq:auth` (fork `SASLAuthentication.getSASLMechanisms`). nfswo sends an empty `<stream:features/>`.

The reference container (below) completes a TLS 1.0 handshake with OpenSSL. Whether the real client does can't be checked without it. That is the roadmap's top risk.

## Logging in: `jabber:iq:auth` (XEP-0078)

- **`get`** returns the fields the server accepts; the container's exact replies are under The reference container. nonsasl `IQAuthHandler` answers `<query xmlns='jabber:iq:auth'><username>…</username><password/><digest/><resource/></query>`, echoing the username if the request had one; `password` and `digest` are there because Openfire's default auth provider can retrieve passwords. nfswo adds `<clientlock xmlns='http://www.jabber.com/schemas/clientlocking.xsd'/>` after `<resource/>`, presumably copied from EA's server. Openfire never sent it, so SBRW clients worked without it.
- **`set`** carries the username, resource and either a plaintext `password` or `digest = sha1(streamId + password)` in lowercase hex. A missing resource is `not-acceptable`, bad credentials are `not-authorized` (nonsasl). Success is an empty `result`.
- **A second login with the same resource** kicks the older session: with Openfire's default kick limit of 0 it gets a `conflict` stream error and is closed (nonsasl `login`).

## Rooms

- **Public rooms** are created when the core starts: `channel.<short_name>__<n>` for `n` in `1..=amount` of each `chat_room` row, persistent, with no user limit and presence broadcast to every role (core `OpenFireRestApiCli.createGeneralChatRoom`). The stock data has one row, `EN` with amount 2.
- **Group rooms** `group.channel.*` are created by the game for private lobbies. The core asks Openfire who is in the persona's group room (core `getAllPersonaByGroup`). sbrw-rs answers that from the hub's room state.
- The MUC service is Openfire's stock `conference.<domain>` (fork). Join, leave, presence fan-out, status 110 for self and groupchat relay are stock Openfire behaviour, which nothing in the sources pins down byte for byte. nfswo has no rooms; it answers every room presence with a 401.

## Pushes from the server

Every push the core sends goes through `OpenFireConnector.send(msg, personaId)`:

```xml
<message to='sbrw.<personaId>@<XMPP_IP>' id='JN_1234567'>
  <subject>1337733113377331</subject>
  <body><!-- msg, escaped --></body>
</message>
```

- The `to` is the bare JID, so Openfire delivers it to the persona's session and stamps `from` with the engine's full JID, `sbrw.engine.engine@<XMPP_IP>/EA_Chat`.
- `msg` is the JAXB-marshalled payload, or for admin replies `XmppChat.createSystemMessage` (core). The rig records `msg` and the recipient for every push as `xmpp` side effects, so the bodies are verified. Only this envelope and its delivery are not.
- **Announcements** (`SendAnnouncement`) go through the REST plugin's `messages/game`. It wraps the text in the same `ChatBroadcast` system message, sets the same subject and id, sets `from` to the server's own address, and broadcasts it to every session (restapi `MessageController.sendGameBroadcastMessage`). sbrw-rs's hub does the same for every logged-in client (`Hub::announce`), with the push's attribute order; the plugin's own order isn't recorded.
- **Subject:** the stock client checks that the subject is a 64-bit hash of `to` followed by the body. The ModNet patch turns that check off, so SBRW sent the constant above. The hash is nfswo's `calculateHashFromData.js`, ported as `sbrw_xmpp::subject::hash` and checked against `fixtures/vectors/xmpp_subject.json` (written by `reference/nfswo/subject_vectors.js`). sbrw-rs sends the constant unless configured for the stock client.
- **Welcome message:** SBRW has none on login. nfswo pushes a `ChatBroadcast` system message once after the first login.

## What the game sends that isn't covered yet

- nfswo rewrites a trailing `'>` into `'/>` before parsing each packet. That suggests the game sends some stanzas without their closing tag, and the framer may need the fallback the roadmap mentions.
- Presence to rooms, groupchat messages, one-to-one `chat` messages and any iq beyond auth have no recorded example from the game.

## The reference container

The fork is stock Openfire apart from the hidden mechanisms, so `reference/xmpp/` runs Openfire 4.5.0, the release nearest the fork, with the nonsasl (1.0.1) and REST API (1.3.10) plugins. It runs on Java 8: SBRW ran JDK 10, and 8 is the nearest current JDK that still bundles the JAXB the REST plugin needs.

```sh
docker build -t sbrw-openfire:4.5.0 reference/xmpp
docker run -d --name sbrw-openfire -p 15222:5222 -p 19090:9090 sbrw-openfire:4.5.0
python3 reference/xmpp/transcript.py 127.0.0.1 15222 19090 > fixtures/xmpp/openfire.txt
```

- **Setup** runs unattended at build time (`setup.sh`), then makes the guide's admin-console settings: in-band registration off, compression off, REST API on with a secret.
- **SCRAM-only passwords:** autosetup also turns on `user.scramHashedPasswordOnly`, which the web wizard leaves off. With it on, `get` drops `<password/><digest/>` and nonsasl refuses every login, because it only authenticates when passwords can be retrieved. SBRW's game logged in, so its install had it off, and `setup.sh` turns it off again.
- **TLS:** current JDKs disable TLS 1.0, so the image's `java.security` re-enables it. Openfire runs with `-Djsse.enableCBCProtection=false`, as SBRW's launcher did.
- **What `transcript.py` does:**
  - `setup_openfire()` creates the engine account, both public rooms and two players through REST, with the bodies the core sends.
  - `scenario()` acts as the game over TLS 1.0. It logs in, sends an unknown iq, then joins `channel.EN__1`, chats there, messages the other player and leaves. It creates a group room and unlocks it. The engine sends two pushes, a second login takes over the first's resource, a player rejoins the room to get its history, and another closes its stream, gets a push while offline, and logs in again to receive it.
- **Stability:** stream ids and generated stanza ids are numbered in order of appearance, and timestamps keep their format with the digits replaced. Two fresh containers give identical transcripts.

`fixtures/xmpp/openfire.txt` shows:

- **TLS** negotiates `ECDHE-RSA-AES256-SHA` at TLS 1.0.
- **Features after TLS:** SASL `mechanisms`, `ver`, `auth` (iq-auth) and `c`, with no `register` and no `compression`. The fork also drops `mechanisms` for the game.
- **iq:auth:**
  - `get` → `<iq type="result" id="…"><query xmlns="jabber:iq:auth"><username>sbrw.100</username><password/><digest/><resource/></query></iq>`.
  - `set` with the right password → `<iq type="result" id="…" to="sbrw.100@127.0.0.1/EA-Chat"/>`.
  - A wrong password → `type="error"` addressed to `<domain>/<stream id>`, echoing the query, with `<error code="401" type="auth"><not-authorized …/></error>`.
- **Initial presence** comes back to the sender from its own full JID. Some time after it, the server sends the client `<iq type="get" id="…" from="127.0.0.1"><query xmlns="jabber:iq:version"/></iq>`.
- **Room names are lowercased:** joining `channel.EN__1` gets presence from `channel.en__1@conference.127.0.0.1/<nick>`.
- **The public rooms are semi-anonymous** (`canAnyoneDiscoverJID` false), so items carry no `jid`. A joiner gets the existing occupants' presence (`affiliation="none" role="participant"`), then its own with status 110, then `<message type="groupchat" id="…"><subject/></message>`. Occupants already in the room get the new presence without 110.
- **Groupchat** is echoed to the sender and relayed to everyone from `room/<nick>`, with `<stanza-id xmlns="urn:xmpp:sid:0" id="…" by="room"/>` added.
- **Leaving:** the leaver gets `type="unavailable"` with `role="none"` and status 110; the others get the same without 110.
- **Pushes:** a message from the engine to a bare JID arrives with `to` unchanged, `id` and child order kept, and `from` stamped `sbrw.engine.engine@127.0.0.1/EA_Chat`.
- **Unknown iq** (`jabber:iq:nonsense` to the server) → `type="error"` from the domain, echoing the query as `<query xmlns="…"></query>`, with `<error code="501" type="cancel"><feature-not-implemented …/></error>`.
- **One-to-one chat** to a full JID arrives with `from` stamped as the sender's full JID. The sender gets nothing back.
- **Group rooms:** joining a `group.channel.*` room that doesn't exist creates it with the joiner as owner. The room is non-anonymous, so items carry `jid`, with statuses 110, 100 and 201. It stays **locked**: anyone else who joins gets a 404 `item-not-found` presence error until the owner submits an empty `jabber:x:data` form to `muc#owner` (an instant room, XEP-0045 §10.1.2). After that, joiners see the owner and themselves with status 100. Nothing shows whether the game unlocks its rooms that way; SBRW's private lobbies worked, so it must do something that unlocks them.
- **Push bodies** are re-serialised by Openfire: `"` and `'` come out unescaped, `<`, `>` and `&` escaped, and newlines and UTF-8 (including non-BMP) raw.
- **Same resource:** a second login with the same resource succeeds. The first stream gets `<stream:error><conflict xmlns="urn:ietf:params:xml:ns:xmpp-streams"/></stream:error></stream:stream>` and is closed, and leaves its rooms.
- **Room history:** a joiner gets the room's earlier groupchat after its own presence and before the subject. Each is the message as relayed, with `<delay xmlns="urn:xmpp:delay" stamp="2026-10-14T00:23:26.671Z" from="room/nick"/>` appended. Openfire keeps the last 25 by default; the recording has one.
- **Pushes to an offline player are stored**, as are pushes to one who hasn't sent initial presence yet. They are delivered right after the player's presence echo, with `<delay xmlns="urn:xmpp:delay" from="<domain>" stamp="…"/>` appended; note `from` before `stamp`, unlike room history. This matters because the game calls `SecureLoginPersona`, which pushes `AchievementsAwarded`, before it connects to XMPP.
- **Disconnects** (closing the stream, or being kicked by a conflict): the other occupants get a presence Openfire builds itself, so its attributes come in a different order: `<presence type="unavailable" from="room/nick" to="…">` with `role="none"` and no status. The client that closes gets `</stream:stream>` back.

Not covered: the REST plugin's SBRW-only endpoints (`chatrooms/forUser`, `messages/game`), which the official plugin lacks; and anything the game sends that the scenario doesn't, such as how it unlocks group rooms.

### Replaying it against sbrw-rs

Until sbrw-xmpp has a listener, its unit tests replay the fixture without the network. `stream::tests::logs_in_as_openfire_does` feeds client A's login through the framer and stream. `hub::tests::rooms_chat_and_pushes_as_openfire_does` runs every client from the room joins to the end: rooms, chat, pushes, history, the conflict and a disconnect. Both compare every stanza each client receives. Once the listener and TLS exist, the full replay works as follows.

The replay runs `transcript.py`'s `scenario()` against sbrw-rs, without `setup_openfire()`. In sbrw-rs the accounts are live sessions, so the replay first logs the personas in over HTTP with a token starting `0123456789abcdef`, and the rooms come from `chat_room`. It then compares the two transcripts:

- **Per client, stanza by stanza, not line by line.** Each `<` line is whatever arrived within 600 ms, so how stanzas are grouped into lines depends on timing.
- **Allowed differences:**
  - stream features: `<mechanisms>` (the fork hides it from the game) and the `<c>` caps hash, which sbrw-rs leaves out;
  - the server's `jabber:iq:version` query: Openfire sends it on a timer, and with no capture of how the game answers, sbrw-rs doesn't send it;
  - stream ids, generated stanza ids and the cipher name.
