---
status: accepted
---

# One Palette per Machine, other Machines read through forwarded sockets

Herdr 0.9 gives the client several machines, each with its own server, socket and id space. The question was where the Palette runs and how it sees the rest.

Options considered:

1. One Palette on the laptop's Local server, driving every machine. Rejected: a popup is a per-server session resource, so that Palette is invisible while the client views another machine, which is most of the time; there is no socket method to switch the client's selected machine; and every Command that touches a repo would run on the wrong host unless proxied over ssh.
2. `herdr machine` registry plus ssh from the Palette to run `herdr` remotely. Rejected: an ssh round trip per poll is too slow for a half-second refresh, needs keys and agents in the plugin's environment, and the registry is client-side data the server-side plugin cannot read.
3. Each Machine runs its own Palette; other Machines' sockets are forwarded into a directory the Palette scans. Chosen.

A directory of sockets (`~/.herdr/machines/<name>.sock`) is the interface. Anything that can forward a Unix socket, today the `Host` entry in the laptop's ssh config, can add a Machine; the Palette has no ssh, hostnames or credentials of its own, and gains a Machine at the next rescan. The same plugin checkout is linked on every Machine, so behaviour and keybindings match wherever the client is looking.

## Consequences

- Remote reads are cheap (a snapshot over a forwarded socket is milliseconds) but each Machine costs one poller thread and one socket round trip per interval; keep `remote_poll_ms` well above `poll_ms`.
- Focus across Machines cannot be made to work from a plugin; it stays best effort until Herdr exposes machine switching.
- The socket directory is a contract shared with the dotfiles ssh config; renaming it or its files breaks discovery on both ends at once.
- Without `StreamLocalBindUnlink yes` a stale socket file survives a disconnect and the next forward fails silently; the Palette then shows that Machine crossed out.
