use super::{Class, Command, Input, InputKind, Target};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Builtin {
    NewWorkspace,
    NewWorktree,
    NewTab,
    RenameWorkspace,
    RenameTab,
    RenameAgent,
    CloseWorkspace,
    CloseTab,
    ClosePane,
    StartAgent,
    EditCommandFiles,
    NewCommandFile,
    FocusWorkspace,
    FocusTab,
    FocusPane,
    FocusAgent,
}

fn cmd(b: Builtin, id: &str, title: &str, keywords: &[&str], inputs: Vec<Input>) -> Command {
    Command {
        id: id.into(),
        title: title.into(),
        keywords: keywords.iter().map(|s| s.to_string()).collect(),
        class: Class::Builtin(b),
        run: String::new(),
        target: Target::Background,
        cwd: None,
        label: None,
        keep_open: matches!(b, Builtin::RenameWorkspace | Builtin::RenameTab | Builtin::RenameAgent),
        confirm: matches!(b, Builtin::CloseWorkspace | Builtin::CloseTab | Builtin::ClosePane),
        inputs,
        disabled: false,
        depth: -2,
        source: "built-in".into(),
    }
}

/// Commands listed in the Commands view. Focus builtins are Item actions only.
pub fn all() -> Vec<Command> {
    use Builtin::*;
    vec![
        cmd(NewWorkspace, "new-workspace", "New workspace", &["ws", "create"], vec![Input::text("label", "Workspace label (empty = directory name)")]),
        cmd(NewWorktree, "new-worktree", "New worktree", &["wt", "branch", "git"], vec![Input::text("branch", "Branch name"), Input::text("base", "Base ref (empty = current)")]),
        cmd(NewTab, "new-tab", "New tab", &["tab"], vec![Input::text("label", "Tab label (optional)")]),
        cmd(RenameWorkspace, "rename-workspace", "Rename workspace", &["ws"], vec![Input::pick("workspace", "Workspace", InputKind::Workspace), Input::text("label", "New label")]),
        cmd(RenameTab, "rename-tab", "Rename tab", &["tab"], vec![Input::pick("tab", "Tab", InputKind::Tab), Input::text("label", "New label")]),
        cmd(RenameAgent, "rename-agent", "Rename agent", &["agent"], vec![Input::pick("agent", "Agent", InputKind::Agent), Input::text("name", "New name ([a-z][a-z0-9_-]*)")]),
        cmd(CloseWorkspace, "close-workspace", "Close workspace", &["ws", "kill"], vec![Input::pick("workspace", "Workspace", InputKind::Workspace)]),
        cmd(CloseTab, "close-tab", "Close tab", &["tab", "kill"], vec![Input::pick("tab", "Tab", InputKind::Tab)]),
        cmd(ClosePane, "close-pane", "Close pane", &["pane", "kill"], vec![Input::pick("pane", "Pane", InputKind::Pane)]),
        cmd(StartAgent, "start-agent", "Start agent in new pane", &["agent", "claude", "split"], vec![Input::text("name", "Agent name (optional)")]),
        cmd(EditCommandFiles, "edit-commands", "Edit command files", &["config", "palette"], vec![Input::pick("file", "Command file", InputKind::Choice)]),
        cmd(NewCommandFile, "new-command-file", "New command file for this directory", &["config", "palette", "scope"], vec![Input::text("name", "File name")]),
    ]
}

pub fn item_action(b: Builtin, title: &str) -> Command {
    let mut c = cmd(b, &format!("item.{}", title.to_lowercase().replace(' ', "-")), title, &[], vec![]);
    c.inputs = match b {
        Builtin::RenameWorkspace => vec![Input::text("label", "New label")],
        Builtin::RenameTab => vec![Input::text("label", "New label")],
        Builtin::RenameAgent => vec![Input::text("name", "New name")],
        Builtin::StartAgent => vec![Input::text("name", "Agent name (optional)")],
        Builtin::NewTab => vec![Input::text("label", "Tab label (optional)")],
        _ => vec![],
    };
    c
}
