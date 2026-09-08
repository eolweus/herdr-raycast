# herdr-raycast

A Raycast-style command palette for [Herdr](https://herdr.dev). One popup to jump to any workspace, tab, pane or agent, and to run built-in, plugin and user-defined commands.

Design and vocabulary: [`CONTEXT.md`](CONTEXT.md), [`docs/design.md`](docs/design.md), [`docs/adr/`](docs/adr).

## Install

```sh
just link                       # cargo build --release, copies to bin/, herdr plugin link
```

Bind a key in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "cmd+shift+p"
type = "plugin_action"
command = "erling.command-palette.open"
```

Actions `open-agents`, `open-workspaces` and `open-commands` open straight into a View. With several machines in Herdr 0.9, link the plugin and bind the keys on every machine: the client applies its own keybindings, and a palette key only works while viewing a machine that has the plugin.

## Machines

Drop another machine's Herdr socket into `~/.herdr/machines/<name>.sock` and its agents, workspaces and panes show up next to the local ones, tagged with `<name>` and searchable by it. An ssh forward is all it takes; both directions for a laptop and a box called munin:

```
Host munin
  StreamLocalBindUnlink yes
  RemoteForward /home/erling/.herdr/machines/mac.sock /Users/me/.config/herdr/herdr.sock
  LocalForward  /Users/me/.herdr/machines/munin.sock /home/erling/.config/herdr/herdr.sock
```

munin needs `StreamLocalBindUnlink yes` in its sshd config too. Rename, close, start agent and `on` commands act on the other machine; `in = split|tab|workspace|current-pane` targets land there, `background` runs the script locally with `RAYCAST_ITEM_MACHINE` and `RAYCAST_ITEM_SOCKET` set. Focus can only be done on that machine's own server, so the palette stays open and tells you to switch machine in Herdr. A machine that stops answering is shown crossed out in the header until it is back.

## Keys

Opens in Insert mode. Escape enters Normal mode; Escape does nothing there.

| Key | Where | Does |
|---|---|---|
| type | Insert | filter |
| `i` `/` | Normal | back to Insert |
| `1`-`4`, Tab, Shift+Tab | both (digits Normal only) | switch View |
| `j` `k` ↑ ↓ ^n ^p ^j ^k | Normal / both | move |
| Enter | both | focus the Item, or run the Command |
| ^o | both | action menu for the Item |
| `p` | Normal | toggle Preview |
| `N` | Normal | new workspace |
| `a` | Normal | start an agent in a split next to the Item |
| `r` | Normal | rename workspace, tab or agent |
| `x` then `y` | Normal | close workspace, tab or pane |
| `e` | Normal | edit the Command File defining the Command |
| `q`, ^c, the key bound to `open` | Normal / both | close (the `open-<view>` keys switch View, or close when already there) |

## Commands

Three classes: built-ins, every installed plugin's actions, and your own Command Files.

Command Files are TOML under `$(herdr plugin config-dir erling.command-palette)/commands/`. A file with a `scope` path is only visible when the palette opens with a directory inside it. Deeper scopes override shallower ones by `id`; `disabled = true` hides any command. See [`examples/commands/`](examples/commands).

```toml
scope = "~/work/infra"

[[commands]]
id = "tf-plan"
title = "Terraform plan"
description = "Plan the stack in this directory"   # optional, shown in the preview; may use markdown (headings, lists, `code`, **bold**, fences)
run = "terraform plan"
in = "split"              # background | current-pane | split | tab | workspace | popup
# on = ["agent"]          # also an action on these rows: workspace | tab | pane | agent

[[commands.inputs]]
name = "target"
prompt = "Target"
kind = "choice"           # text | choice | workspace | tab | pane | agent | ref
choices_command = "terraform state list"
```

`run` sees `{{input.<name>}}`, `{{ctx.dir}}`, `{{ctx.workspace_id}}`, `{{ctx.tab_id}}`, `{{ctx.pane_id}}`, `{{ctx.agent}}`, the `{{item.*}}` vars below, and the same values as `RAYCAST_*` env vars. A relative `run` path resolves next to the Command File. Scripts can call any socket method with `herdr-raycast api <method> [json]`.

"New command file for this directory" writes a scoped template and opens it; "Edit command files" picks one. Both use `nvim-open` when on PATH, else `$EDITOR` in a popup.

## Item actions

A Command File command with `on = ["agent"]` (any of `workspace`, `tab`, `pane`, `agent`) also appears in the ctrl+o action menu of those rows. Run from the Commands list, it acts on the captured pane's agent (or the tab's only agent), pane, tab or workspace. The command learns about the item through `{{item.kind}}`, `{{item.id}}`, `{{item.workspace_id}}`, `{{item.tab_id}}`, `{{item.pane_id}}`, `{{item.label}}`, `{{item.cwd}}`, `{{item.agent}}`, `{{item.agent_kind}}` and `{{item.session_id}}` (the Claude session id), and the matching `RAYCAST_ITEM_*` env vars. That is enough to write a Claude conversation fork as a shell script; a `yeet` command that does so lives in the author's dotfiles.

## Ranking

Exact matches beat prefix and fuzzy ones, and a hit in the title beats a hit in the workspace or path. Every Enter is counted in `usage.json` in the state dir, and better-used items win ties. Delete that file to reset.

## Config

`$(herdr plugin config-dir erling.command-palette)/config.toml`, all optional:

```toml
width = "60%"
height = "60%"
editor = "auto"          # or a command; {{file}} is substituted
poll_ms = 500
preview_lines = 40
debug_keys = false  # append every key event to palette.log in the state dir
agent_kind = "claude"
machines_dir = "~/.herdr/machines"   # other machines' forwarded sockets, <name>.sock
remote_poll_ms = 2000                # how often each of them is polled
```

Logs: `~/.local/state/herdr/plugins/erling.command-palette/palette.log` and `commands.log`.
