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
- **Announcements** (`SendAnnouncement`) go through the REST plugin's `messages/game`. It wraps the text in the same `ChatBroadcast` system message, sets the same subject and id, sets `from` to the server's own address, and broadcasts it to every session (restapi `MessageController.sendGameBroadcastMessage`).
- **Subject:** the stock client checks that the subject is a 64-bit hash of `to` followed by the body. The ModNet patch turns that check off, so SBRW sent the constant above. The hash is nfswo's `calculateHashFromData.js`, ported as `sbrw_xmpp::subject::hash` and checked against `fixtures/vectors/xmpp_subject.json` (written by `reference/nfswo/subject_vectors.js`). sbrw-rs sends the constant unless configured for the stock client.
- **Welcome message:** SBRW has none on login. nfswo pushes a `ChatBroadcast` system message once after the first login.

## What the game sends that isn't covered yet

- nfswo rewrites a trailing `'>` into `'/>` before parsing each packet. That suggests the game sends some stanzas without their closing tag, and the framer may need the fallback the roadmap mentions.
- Presence to rooms, groupchat messages, one-to-one `chat` messages and any iq beyond auth have no recorded example from the game.

## The reference container

The fork is stock Openfire apart from the hidden mechanisms, so `reference/xmpp/` runs Openfire 4.5.0, the release nearest the fork, with nonsasl 1.0.1:

```sh
docker build -t sbrw-openfire:4.5.0 reference/xmpp
docker run -d --name sbrw-openfire -p 15222:5222 sbrw-openfire:4.5.0
python3 reference/xmpp/transcript.py 127.0.0.1 15222
```

- Setup runs unattended at build time (`setup.sh`). Autosetup takes the wizard's steps but also turns on `user.scramHashedPasswordOnly`, which the wizard leaves off. With it on, `get` drops `<password/><digest/>` and nonsasl refuses every login, because it only authenticates when passwords can be retrieved. SBRW's game logged in, so its install had it off; `setup.sh` turns it off again.
- JDK 11 disables TLS 1.0, so the image's `java.security` re-enables it. Openfire runs with `-Djsse.enableCBCProtection=false`, as SBRW's launcher did.
- Accounts are made with in-band registration, where the core used the REST plugin; the accounts end up the same.

What it has shown so far (`transcript.py`, client pinned to TLS 1.0 at `SECLEVEL=0`):

- TLS 1.0 negotiates `ECDHE-RSA-AES256-SHA`.
- Stream features before TLS: `starttls`, SASL `mechanisms`, `compression`, `ver`, `auth` (iq-auth), `register`, `c`. After TLS the same without `starttls`. The fork drops `mechanisms` for the game, and SBRW's guide turns compression off.
- iq:auth `get` → `<iq type="result" id="…"><query xmlns="jabber:iq:auth"><username>sbrw.100</username><password/><digest/><resource/></query></iq>`.
- iq:auth `set` with the right password → `<iq type="result" id="…" to="sbrw.100@127.0.0.1/EA-Chat"/>`. A wrong one → `type="error"` echoing the query, with `<error code="401" type="auth"><not-authorized …/></error>`. Once the stream is logged in, another `set` gets `result` whatever the password.
- `<presence/>` comes back to the sender from its own full JID.
- Room names are lowercased: joining `channel.EN__1` gets presence `from="channel.en__1@conference.127.0.0.1/sbrw.100"`. A first joiner of a missing room becomes its owner with statuses 110, 100 and 201 and an empty `<subject/>` message.

Still to do: create the public rooms as the core does (persistent, before anyone joins), script a second client for presence fan-out, groupchat, leaving and a resource conflict, send a push from the engine account, and commit the transcripts under `fixtures/xmpp/` for the replay. Until then MUC fan-out and conflict handling are from the sources above, not checked.
