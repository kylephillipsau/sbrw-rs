# Running sbrw-rs on a VPS

How to run the server for friends on another network: a small Linux VPS,
the image built from this repository, and four open ports. None of this has been tried
with a real game client yet (the roadmap's M8 check, racing a friend on
another network, is still to do).

## What you need

- A Linux VPS, amd64 or arm64, with a public IPv4 address. The server is
  light: 1 vCPU and 1 GB of memory are plenty for a group of friends.
- Docker Engine with the compose plugin (`docker compose version`).
  Use Docker Engine on Linux, not Docker Desktop: on Linux, Docker forwards
  published UDP ports with DNAT, which keeps each player's real address and
  port. Free-roam and the race relay tell players apart by them.
- These ports open to the internet:

  | Port | Protocol | For |
  |---|---|---|
  | 8680 | TCP | the launcher and game API |
  | 5222 | TCP | XMPP: chat and server pushes |
  | 9999 | UDP | free-roam |
  | 9998 | UDP | the race relay |

  With `ufw`:

  ```sh
  sudo ufw allow 8680/tcp && sudo ufw allow 5222/tcp
  sudo ufw allow 9999/udp && sudo ufw allow 9998/udp
  ```

  Your provider may have a firewall of its own in its control panel; open
  the same ports there.

## Start it

There is no published image yet, so build it on the VPS from the
repository. The compiler peaks at about 650 MB, so on a 1 GB VPS add swap
first; the build takes about 5 minutes on one fast core, longer on a VPS.

```sh
sudo fallocate -l 2G /swapfile && sudo chmod 600 /swapfile
sudo mkswap /swapfile && sudo swapon /swapfile
```

Then:

```sh
mkdir ~/sbrw && cd ~/sbrw
git clone https://github.com/kylephillipsau/sbrw-rs.git
```

Put this in `~/sbrw/compose.yaml`, with your server's public IP:

```yaml
services:
  sbrw:
    build: ./sbrw-rs
    image: sbrw-rs:local
    container_name: sbrw
    environment:
      SBRW__SERVER__PUBLIC_HOST: 203.0.113.7    # your public IP
      SBRW__LAUNCHER__SERVER_NAME: My Server
    ports:
      - "8680:8680/tcp"
      - "5222:5222/tcp"
      - "9999:9999/udp"
      - "9998:9998/udp"
    volumes:
      - sbrw-data:/data
    restart: unless-stopped

volumes:
  sbrw-data:
```

```sh
docker compose up -d --build
docker compose logs -f      # "HTTP listening", "XMPP listening", ...
curl http://203.0.113.7:8680/health
```

`docker ps` shows the container as `healthy` once `/health` answers. Then
each player adds `http://203.0.113.7:8680/Engine.svc` as a server in the
SBRW launcher, registers and plays. The launcher installs the ModNet
patches the first time.

More settings go in `/data/sbrw.toml` inside the volume, or as
`SBRW__SECTION__KEY` variables; see the README's Configuration section.
To edit the file:

```sh
docker run --rm -it -v sbrw_sbrw-data:/data alpine vi /data/sbrw.toml
docker compose restart
```

(`docker volume ls` shows the volume's full name.)

## Running it

Commands for the running server go through `docker exec`:

```sh
docker exec sbrw sbrw admin set-admin you@example.com   # in-game /kick, /ban, /unban
docker exec sbrw sbrw admin announce Restarting in 5 minutes
docker exec sbrw sbrw admin ban DRIVER --for 1d --reason "wall riding"
docker exec sbrw sbrw admin tokens
```

**Invite only.** Set a ticket token and registration needs a ticket:

```yaml
    environment:
      SBRW_PARAMETER_TICKET_TOKEN: some-secret
```

Then `docker exec sbrw sbrw admin ticket alice` makes Alice's ticket, which
she enters when registering in the launcher.

**Forgotten passwords.** `echo 'new password' | docker exec -i sbrw sbrw
admin set-password player@example.com`. For players to reset their own
from `/forgotPasswd.jsp`, set up mail (`[mail] smtp_url` and the
`EMAIL_FROM` parameter); with Gmail, use an app password.

## Backups

Every night at 04:00 UTC the server copies its database to
`/data/backups/`, keeping a week of copies, and before any update that
changes the database it keeps a copy too. `sbrw admin backup` makes one
now. They're on the same disk as the server, so copy them off it now and
then:

```sh
docker cp sbrw:/data/backups ./sbrw-backups
```

To restore one: stop the server, copy the backup over `/data/world.db`,
and start it again.

```sh
docker compose stop
docker run --rm -v sbrw_sbrw-data:/data alpine \
  sh -c 'cp /data/backups/world-20261018T040000Z-nightly.db /data/world.db && rm -f /data/world.db-wal /data/world.db-shm'
docker compose start
```

## Updating

```sh
git -C sbrw-rs pull && docker compose up -d --build
```

The new version migrates the database if it needs to, after backing it up.

## If the ModNet CDN goes away

The launcher fetches its patches from `cdn.soapboxrace.world`, an address
built into it, and won't launch the game if that fails. Before that
happens, keep a copy: `tools/mirror-modnet.sh` downloads the files and
checks them against `assets/mirrors/modnet/modules.json`.

If it does go, this should work, though it is untested: turn ModNet off
on the server (`SBRW__MODDING__ENABLED: "false"`), so the launcher skips
the download and launches the game as it is, and have each player copy
the mirrored files into their game folder by hand. Failing that, the
roadmap's M9 (the unpatched client) is the way out.

## When something's wrong

- **The launcher shows the server offline.** Check
  `curl http://<ip>:8680/Engine.svc/GetServerInformation` from another
  machine, then the firewalls.
- **Players log in but see no one in free-roam, or races don't start.**
  UDP 9998 and 9999 aren't getting through, or something between is
  rewriting source ports. `SBRW_CAPTURE=udp-freeroam,udp-race` records
  what arrives into `/data/captures/`.
- **No chat or invites.** TCP 5222.
- **"Too many failed attempts from your address."** Ten failed logins
  from one address in five minutes; it clears itself
  (`[http] auth_failures` changes the limit).
