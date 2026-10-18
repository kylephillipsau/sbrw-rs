# Running sbrw-rs on Unraid

Unraid runs Docker Engine on Linux, so the server behaves as it does on a
VPS ([vps.md](vps.md)). This page covers what differs: the template, the
data folder's owner, and hosting from home. Like the VPS guide, it hasn't
been tried with the real game yet.

## Install

In the Unraid terminal:

```sh
wget -qO /boot/config/plugins/dockerMan/templates-user/my-sbrw-rs.xml \
  https://raw.githubusercontent.com/kylephillipsau/sbrw-rs/main/unraid/sbrw-rs.xml
mkdir -p /mnt/user/appdata/sbrw-rs && chown 99:100 /mnt/user/appdata/sbrw-rs
```

The first command adds the template. The second makes the data folder,
owned by `nobody:users`, the user the template runs the container as
(`--user 99:100`). Docker would otherwise create the folder owned by root,
and the server would stop at start with "the data directory /data ...
isn't writable".

Then, in the Docker tab, choose Add Container, pick `sbrw-rs` under
Template, fill in Public IP and Apply. The container shows as healthy once
`/health` answers. Each player adds `http://<public IP>:8680/Engine.svc`
as a server in the SBRW launcher.

## Hosting from home

- Forward 8680/tcp, 5222/tcp, 9999/udp and 9998/udp on your router to the
  Unraid server, keeping the same port numbers.
- Public IP is your internet address; `curl ifconfig.me` in the terminal
  shows it. If your ISP changes it, update the template to match.
- If the WAN address your router shows differs from that, your ISP puts you
  behind carrier-grade NAT and forwarded ports can't reach you. Ask the ISP
  for a public IP, or use a VPS.
- The server gives every player the same address, so players on your home
  network connect through the public IP too. That needs your router to
  support NAT loopback (hairpinning), which most do.

## Running it

The container's Console button doesn't work, because the image has no
shell. Run commands from the Unraid terminal instead:

```sh
docker exec sbrw-rs sbrw admin set-admin you@example.com
docker exec sbrw-rs sbrw admin announce Restarting in 5 minutes
```

[vps.md](vps.md) has the other commands, backups and troubleshooting; its
`/data` is `/mnt/user/appdata/sbrw-rs` here, so backups are in
`/mnt/user/appdata/sbrw-rs/backups` and settings go in
`/mnt/user/appdata/sbrw-rs/sbrw.toml`. To record a session, set Capture
(under Show more settings) to `http,xmpp,udp-freeroam,udp-race`; the
captures land in `/mnt/user/appdata/sbrw-rs/captures`.

To update, edit the container and Apply without changes: Unraid pulls the
image again and recreates the container.
