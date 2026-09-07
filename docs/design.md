# herdr-raycast design (settled 2026-09-07)

Working name herdr-raycast, manifest id `erling.command-palette`, display name "Command Palette". Vocabulary is in `../CONTEXT.md`; decisions with trade-offs are in `adr/`.

## Stack

- Rust, ratatui, nucleo for fuzzy matching, crossterm for input.
- Direct Herdr socket client (ADR-0001). One connection per request. CLI kept only for scripts.
- Manifest `herdr-plugin.toml` with two `[[panes]]` entries (`palette` and `runner`, placement `popup`, which is the sized session-modal floating pane; `overlay` cannot be sized and returns a pane id, `popup` returns none and is tracked by a marker file) and actions `open` (All), `open-agents`, `open-workspaces`, `open-commands`. The user binds a key to `open` in their Herdr config (cmd+ø here, since ctrl+space is the WezTerm leader); keybindings never live in the manifest.
- `open` toggles: the UI writes an open marker in the state dir on start and clears it on exit; a second `open` calls `popup.close` when the marker exists. Herdr does not dispatch custom bindings while a popup has focus, so the UI also reads the `[[keys.command]]` entries bound to this plugin from the Herdr config and treats the `open` chord as close and the `open-<view>` chords as View switches. The UI pushes kitty keyboard flags so cmd and ctrl chords arrive unambiguously.
- Reference material on this machine: `~/projects/herdr-reviewr/docs/herdr-api-notes.md`, `~/.config/herdr/plugins/github/herdr.auto-title-*/docs/architecture/herdr-socket-api.md` (verified against 0.8.2, protocol 20), `herdr api schema --json`.

## Host facts that shape the design

- Herdr 0.8.2 on this host (munin, hostname `erling-test`). This is the primary install; the laptop links a clone later. Remote key delivery needs `--remote-keybindings server` on the client.
- Agent Status enum: `idle | working | blocked | done | unknown`. `done` = finished while unseen. No last-message API; `terminal_title_stripped` is the free signal, `agent.read --source recent-unwrapped` the paid one.
- `session.snapshot` returns the whole tree; events are not subscribed (ADR-0002).
- When the Palette is open it is the focused pane. Every action uses the Captured Context from `HERDR_PLUGIN_CONTEXT_JSON` at open, and `plugin.action.invoke` is called with that context explicitly.
- `plugin pane close` only knows in-memory panes; fall back to `pane.close`.
- Plugin commands run with a minimal PATH; the launcher shim prepends the usual bin dirs and `~/dotfiles/bin`.

## Layout

```
┌ [1] All  [2] Agents  [3] Workspaces  [4] Commands      ~/atlas/infra/terraform ┐
│ > query_                                                                         │
│ ◉ retire-atlas-upd   blocked   retire-atlas-upd   │ preview (when toggled)      │
│ ✓ reviewer           done      spain outreach     │ status, name, ws, cwd, title│
│   herdr-raycast                                   │ last 40 lines of output     │
│   New worktree                       command      │                             │
└──────────────────────────────────────────────────────────────────────────────────┘
```

Header shows the View tabs with their numbers and the Context Directory. Preview splits the overlay roughly 50/50 when on.

## Views

- **All**: three sections with a header line each: Agents, Workspaces (no tabs or panes), Commands. A query puts the three best matches first under a Top header, then ranks the rest within each section and hides empty sections.
- **Agents**: flat list, sorted blocked, done, working, idle, unknown, then Herdr order.
- **Workspaces**: the Herdr navigator tree. `▾` on workspace rows, `├──`/`└──` with `│` spines under children, a blank line between workspaces, `◆` on the captured workspace and pane. Tab rows appear only when a workspace has more than one tab. Pane rows are labeled by manual label, agent name, terminal title, agent kind, else "shell". A query orders workspaces by their best match, keeps tree order inside each, and shows ancestors of matches dimmed.
- **Commands**: all three classes, with class and source shown. With no query, the most used come first.

## Yeet

`Yeet` (Command, and an action on Agent rows) hands work off without interrupting the session. It takes the Claude agent in the palette's pane, or the only Claude agent in that tab, reads its session id from the Herdr snapshot, opens an unfocused tab labeled `yeet` in the same workspace and cwd, starts `claude --resume <id> --fork-session` there through `agent.start`, and prompts the fork to run `/yeet-task` with the optional instruction, without asking for confirmation. The fork closes its own tab when yeet-spawn has reported; on failure or a real need for input it renames the tab to `yeet: needs input` and shows a Herdr notification. A lingering `yeet` tab therefore means attention is needed. `/yeet-fork` is not offered: it relies on the spawned worker reporting back to the calling pane, which for a fork no longer exists.

## Ranking

Every item has search fields; the first is its title. A match is tiered: exact, whole word, prefix, substring, fuzzy, with any title match above any match in another field. Within a tier, items used more often (every Enter is counted in `usage.json` in the state dir) come first, then nucleo's score. Herdr's generated tab labels (`1 · dir › agent › title`) are not search fields because they fuzzy-match almost anything.

Status glyphs appear only on agents and on panes running an agent; workspace rows get a blue ▪ outside the tree (the tree caret already marks them there); workspace and tab rows carry an activity summary ("1 blocked · 2 working") in their meta instead. Meta is a list of colored segments: paths cyan, agent kind magenta, workspace blue, status in its status color, the rest dim. Paths show their last two components. The header shows the captured workspace and Context Directory in the same colors, bold.

Fuzzy match fields: workspace label and directory name; tab label plus workspace label; pane agent name or process plus tab and workspace; agent name, title, workspace; Command title, id, keywords, plugin id.

## Modes and keys

Opens in Insert. Escape enters Normal. Escape in Normal does nothing.

| Key | Mode | Action |
|---|---|---|
| typing | Insert | filter |
| Esc | Insert | to Normal |
| `/`, `i` | Normal | to Insert |
| `q` | Normal | close |
| ctrl+c | both | close |
| `1`-`4`, Tab, Shift+Tab | both (letters Normal only) | switch View |
| `j`/`k`, arrows, ctrl+n/ctrl+p, ctrl+j/ctrl+k | Normal / both | move |
| Enter | both | primary action |
| `p` | Normal | toggle Preview |
| ctrl+o | both | action menu for the Item |
| `N` | Normal | new workspace |
| `a` | Normal, pane or workspace Item | split a pane and start an agent there |
| `r` | Normal | rename workspace, tab or agent |
| `x` then `y` | Normal | close workspace, tab or pane |
| `e` | Normal, Command Item | edit the defining Command File |

Keys resolve by the Item kind under the cursor, so they work in All. The keymap is one table in code; no config layer in v1.

Primary actions: Workspace, Tab, Pane, Agent focus the target and close the Palette. Command prompts for inputs inside the Palette (text inputs reuse the search bar, picker inputs reuse the list), then runs and closes unless `keep_open`. The rename builtins keep the Palette open and report the result in the status line; typed agent names are sanitized to Herdr's rule (`[a-z][a-z0-9_-]*`, 32 chars) before the call, and New worktree branch names to a lowercase name git accepts (`/` kept, other invalid characters become `-`).

Preview: off by default, toggled with `p`, remembered in `HERDR_PLUGIN_STATE_DIR`. Agents: status, name, workspace, cwd, title, last 40 lines of recent output, refreshed for the selected row only. Commands: resolved `run`, Target, inputs, source file. Workspaces: tab list. Tabs: panes.

## Commands

Three classes: Built-in (native Rust), Plugin Action (every installed plugin's manifest actions that declare `contexts`, minus our own, invoked via `plugin.action.invoke` with the Captured Context), User-defined (Command Files).

### Command Files

`$HERDR_PLUGIN_CONFIG_DIR/commands/*.toml`. Never in a repo (ADR-0003).

```toml
scope = "/home/erling/atlas/atlas-workspace/worktrees/main/infra/terraform"  # omit for Global

[[commands]]
id = "new-worktree"
title = "New worktree"
keywords = ["wt", "branch"]
run = "herdr worktree create --branch {{input.branch}} --base main --focus"
in = "background"            # background | current-pane | split | tab | workspace | popup
cwd = "{{ctx.dir}}"          # default: Context Directory
label = "{{input.branch}}"   # tab or workspace label when `in` creates one
keep_open = false
confirm = false

[[commands.inputs]]
name = "branch"
prompt = "Branch name"
kind = "text"                # text | choice | workspace | tab | pane | agent | ref (git refs: origin default branch first, then local branches; typed refs accepted)
# choices = ["a", "b"]       # for kind = choice
# choices_command = "git branch --format=%(refname:short)"

[[commands]]
id = "persiyanov.reviewr.open"
disabled = true              # hides a Plugin Action, Built-in or Global Command in this Scope
```

Scope matching: the Context Directory lies inside `scope`, checked against both the real path and the path with the worktree checkout prefix replaced by the repo root. Narrowest Scope wins on id collision; Global lowest; Built-ins and Plugin Actions sit below Global.

Context Directory: focused pane's live cwd (process-info of the foreground process group), else pane launch cwd, else workspace cwd.

### Script contract

- `run` is a shell string or a path. Relative paths resolve against the Command File's directory.
- Templates: `{{input.<name>}}`, `{{ctx.dir}}`, `{{ctx.workspace_id}}`, `{{ctx.tab_id}}`, `{{ctx.pane_id}}`, `{{ctx.agent}}`, `{{item.id}}`.
- Env vars with the same values: `RAYCAST_DIR`, `RAYCAST_WORKSPACE_ID`, `RAYCAST_TAB_ID`, `RAYCAST_PANE_ID`, `RAYCAST_AGENT`, `RAYCAST_ITEM_ID`, `RAYCAST_INPUT_<NAME>`.
- `HERDR_BIN_PATH` and `HERDR_SOCKET_PATH` pass through. `herdr-raycast api <method> [json]` calls any socket method for scripts.
- The Palette spawns the process detached, then exits.

### Built-ins in v1

new workspace, new worktree, new tab, rename workspace, rename tab, rename agent, yeet, close workspace, close tab, close pane, start agent in new pane, run herdr-nvim pick-file, Edit Command Files, New Command File for this directory.

New Command File takes a name, sanitizes it to `[a-z0-9-]`, writes `commands/<name>.toml` with `scope` set to the Context Directory, then opens it. Editing uses `nvim-open <file>` when found on PATH, else `$EDITOR <file>` in a popup Target. An `examples/` directory ships shell equivalents of the Built-ins.

## Out of v1

Files View, Claude transcript reading, host built-in actions (split, zoom), frecency, theme inheritance, configurable in-Palette keys, a committed in-repo Command File source.
