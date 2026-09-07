---
status: accepted
---

# Poll the session snapshot instead of subscribing to events

Herdr 0.8.2 replays a backlog of roughly the last 95 revisions of every pane on `events.subscribe`, paced at about ten per second, so a live change can arrive more than ten seconds late and there is no way to skip the replay. The Palette instead calls `session.snapshot` on a half-second interval while open, which returns the whole workspace, tab, pane and agent tree in under a millisecond for a typical session. The UI reconciles by identity (ids), never by row index, so a refreshed snapshot moves the cursor with its Item rather than leaving it on a position.

## Consequences

- Do not reintroduce a subscription without measuring the backlog again on the installed Herdr version.
- Preview content that needs a per-pane read (recent output) is fetched only for the selected Item, not for every row.
