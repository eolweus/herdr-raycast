# Command Palette

A Herdr plugin that opens a searchable overlay for jumping to workspaces, tabs, panes and agents, and for running user-defined and built-in commands.

## Language

**Palette**:
The overlay pane the plugin draws. One Palette is open at a time.
_Avoid_: popup, picker, launcher

**Machine**:
One Herdr server. The Palette runs on one Machine (its Items carry no name) and can read others whose sockets are forwarded into its machines directory; their Items carry the Machine's name. Ids are only unique within a Machine.
_Avoid_: host, remote, server, session

**View**:
One of the top-level lists the Palette can show: All, Agents, Workspaces, Commands. Selected with the tabs across the top of the Palette.
_Avoid_: tab (that word is reserved for Herdr tabs), page, filter

**Item**:
One row in a View. An Item is a Workspace, Tab, Pane, Agent or Command.
_Avoid_: entry, row, result

**Command**:
An Item the Palette can run. One of three classes: Built-in (native, shipped with the plugin), Plugin Action (declared by any installed Herdr plugin's manifest), or User-defined (declared in a Command File). A User-defined Command may declare `on` and then also serves as an Item action for those Item kinds.
_Avoid_: action on its own (Herdr's manifest concept is a Plugin Action here), quick action, task

**Target**:
Where a Command's `run` executes: background, current-pane, split, tab, workspace or popup. The Palette resolves Targets natively; the `run` itself can be any executable.
_Avoid_: placement, mode

**Captured Context**:
The workspace, tab, pane and Context Directory that were focused when the Palette opened. Every Command and Item action acts on the Captured Context, never on the Palette's own pane.
_Avoid_: focused pane, current pane

**Command File**:
A TOML file declaring User-defined Commands, kept outside any repo in the plugin's config directory.

**Scope**:
The directory subtree a Command File applies to. A Command is visible only when the Context Directory lies inside its Scope. A Command File with no Scope is Global.
_Avoid_: project, per-project, repo-local

**Context Directory**:
The directory the Palette treats as "here" when it opens, used to resolve Scopes and as the default working directory for Commands.
_Avoid_: cwd, project root

**Mode**:
The keyboard state of the Palette: Insert (typing filters the list) or Normal (keys are commands).

**Preview**:
The panel beside the list that shows detail for the selected Item, such as an Agent's status and recent output.
_Avoid_: side panel, split, summary

**Agent Status**:
Herdr's classification of an agent: idle, working, blocked, done or unknown. Done means the agent finished while its tab was not being watched.
_Avoid_: state, waiting-for-input
