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

Actions `open-agents`, `open-workspaces` and `open-commands` open straight into a View. Over `--remote`, custom keys need `--remote-keybindings server` on the client and `herdr server reload-config` on the server.

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
run = "terraform plan"
in = "split"              # background | current-pane | split | tab | workspace | popup

[[commands.inputs]]
name = "target"
prompt = "Target"
kind = "choice"           # text | choice | workspace | tab | pane | agent | ref
choices_command = "terraform state list"
```

`run` sees `{{input.<name>}}`, `{{ctx.dir}}`, `{{ctx.workspace_id}}`, `{{ctx.tab_id}}`, `{{ctx.pane_id}}`, `{{ctx.agent}}`, `{{item.id}}`, and the same values as `RAYCAST_*` env vars. A relative `run` path resolves next to the Command File. Scripts can call any socket method with `herdr-raycast api <method> [json]`.

"New command file for this directory" writes a scoped template and opens it; "Edit command files" picks one. Both use `nvim-open` when on PATH, else `$EDITOR` in a popup.

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
```

Logs: `~/.local/state/herdr/plugins/erling.command-palette/palette.log` and `commands.log`.
