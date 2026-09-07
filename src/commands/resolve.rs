use super::{builtins, files, Class, Command};
use crate::context::Captured;
use crate::herdr::{Client, PluginAction};
use std::collections::HashMap;

pub struct Resolved {
    pub commands: Vec<Command>,
    pub errors: Vec<String>,
    pub files: Vec<std::path::PathBuf>,
}

pub fn resolve(client: &Client, ctx: &Captured, own_plugin_id: &str) -> Resolved {
    let mut all: Vec<Command> = builtins::all();
    let actions = client.plugin_actions().unwrap_or_default();
    all.extend(plugin_commands(&actions, own_plugin_id));
    let loaded = files::load(&crate::config::commands_dir(), ctx);
    all.extend(loaded.commands);
    Resolved { commands: merge(all), errors: loaded.errors, files: loaded.files }
}

fn plugin_commands(actions: &[PluginAction], own: &str) -> Vec<Command> {
    actions
        .iter()
        .filter(|a| !a.contexts.is_empty() && a.plugin_id != own)
        .map(|a| Command {
            id: format!("{}.{}", a.plugin_id, a.action_id),
            title: a.title.clone(),
            keywords: a.description.iter().cloned().collect(),
            class: Class::PluginAction { plugin_id: a.plugin_id.clone(), action_id: a.action_id.clone() },
            run: String::new(),
            target: Default::default(),
            cwd: None,
            label: None,
            keep_open: false,
            confirm: false,
            inputs: vec![],
            disabled: false,
            depth: -1,
            source: a.plugin_id.clone(),
        })
        .collect()
}

/// Deeper scopes replace shallower ones by id; `disabled` removes the id.
pub fn merge(mut all: Vec<Command>) -> Vec<Command> {
    all.sort_by_key(|c| c.depth);
    let mut by_id: HashMap<String, Command> = HashMap::new();
    let mut order: Vec<String> = vec![];
    for c in all {
        if c.disabled {
            by_id.remove(&c.id);
            continue;
        }
        if !by_id.contains_key(&c.id) {
            order.push(c.id.clone());
        }
        by_id.insert(c.id.clone(), c);
    }
    let mut out: Vec<Command> = order.into_iter().filter_map(|id| by_id.remove(&id)).collect();
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Target;

    fn user(id: &str, title: &str, depth: i32, disabled: bool) -> Command {
        Command {
            id: id.into(),
            title: title.into(),
            keywords: vec![],
            class: Class::User { file: "x.toml".into() },
            run: String::new(),
            target: Target::Background,
            cwd: None,
            label: None,
            keep_open: false,
            confirm: false,
            inputs: vec![],
            disabled,
            depth,
            source: String::new(),
        }
    }

    #[test]
    fn deeper_scope_wins() {
        let out = merge(vec![user("a", "deep", 3, false), user("a", "global", 0, false)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "deep");
    }

    #[test]
    fn disabled_hides_shallower() {
        let mut all = builtins::all();
        all.push(user("new-worktree", "", 2, true));
        assert!(merge(all).iter().all(|c| c.id != "new-worktree"));
    }

    #[test]
    fn disabled_does_not_hide_deeper() {
        let out = merge(vec![user("a", "deep", 3, false), user("a", "", 1, true)]);
        assert_eq!(out.len(), 1);
    }
}
