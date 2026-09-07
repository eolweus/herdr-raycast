pub mod builtins;
pub mod files;
pub mod resolve;
pub mod run;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    #[default]
    Background,
    CurrentPane,
    Split,
    Tab,
    Workspace,
    Popup,
}

impl Target {
    pub fn label(self) -> &'static str {
        match self {
            Target::Background => "background",
            Target::CurrentPane => "current-pane",
            Target::Split => "split",
            Target::Tab => "tab",
            Target::Workspace => "workspace",
            Target::Popup => "popup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    #[default]
    Text,
    Choice,
    Workspace,
    Tab,
    Pane,
    Agent,
    Ref,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Input {
    pub name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub kind: InputKind,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub choices_command: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
}

impl Input {
    pub fn text(name: &str, prompt: &str) -> Self {
        Self { name: name.into(), prompt: prompt.into(), kind: InputKind::Text, ..Default::default() }
    }
    pub fn pick(name: &str, prompt: &str, kind: InputKind) -> Self {
        Self { name: name.into(), prompt: prompt.into(), kind, ..Default::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Class {
    Builtin(builtins::Builtin),
    PluginAction { plugin_id: String, action_id: String },
    User { file: PathBuf },
}

impl Class {
    pub fn label(&self) -> &'static str {
        match self {
            Class::Builtin(_) => "built-in",
            Class::PluginAction { .. } => "plugin",
            Class::User { .. } => "user",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub class: Class,
    #[serde(default)]
    pub run: String,
    #[serde(default)]
    pub target: Target,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub keep_open: bool,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub inputs: Vec<Input>,
    #[serde(default)]
    pub disabled: bool,
    /// Scope precedence: higher wins. Built-ins -2, plugin actions -1, global 0, scoped = path depth.
    #[serde(default)]
    pub depth: i32,
    #[serde(default)]
    pub source: String,
}

impl Command {
    pub fn search_text(&self) -> Vec<String> {
        let mut v = vec![self.title.clone(), self.id.clone()];
        v.extend(self.keywords.iter().cloned());
        if let Class::PluginAction { plugin_id, .. } = &self.class {
            v.push(plugin_id.clone());
        }
        v
    }
}
