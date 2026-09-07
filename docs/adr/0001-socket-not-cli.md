---
status: accepted
---

# Talk to the Herdr socket directly instead of shelling out to the CLI

The sibling plugins (herdr-nvim, herdr-reviewr) drive Herdr by spawning the `herdr` CLI and parsing its JSON. We open the Unix socket ourselves and send newline-delimited JSON requests, one connection per request. The CLI has no focus-by-id, runs plugin commands with a minimal PATH, and costs a process spawn per call, while a Palette that polls twice a second and jumps to panes by id needs all three fixed. The CLI remains available to User-defined Command scripts, and `herdr-raycast api <method> [json]` exposes the socket to them too.

## Consequences

- The plugin depends on the socket protocol version. Record the verified Herdr version whenever the client is changed.
- Anything the schema exposes is reachable without waiting for a CLI wrapper, including `plugin.action.invoke` with an explicit context.
