use crate::herdr::{Client, Worktree};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const CONTEXT_ENV: &str = "RAYCAST_CONTEXT";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Captured {
    pub workspace_id: Option<String>,
    pub workspace_label: Option<String>,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
    pub agent: Option<String>,
    pub dir: PathBuf,
    pub alt_dir: Option<PathBuf>,
    #[serde(default)]
    pub raw: Value,
}

impl Captured {
    pub fn from_env() -> Option<Self> {
        let s = std::env::var(CONTEXT_ENV).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn capture(client: &Client) -> Self {
        let raw: Value = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let field = |k: &str| raw.get(k).and_then(Value::as_str).map(str::to_string);
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());

        let workspace_id = field("workspace_id").or_else(|| env("HERDR_WORKSPACE_ID"));
        let tab_id = field("tab_id").or_else(|| env("HERDR_TAB_ID"));
        let pane_id = field("focused_pane_id").or_else(|| env("HERDR_PANE_ID"));
        let worktree: Option<Worktree> = raw.get("worktree").cloned().and_then(|v| serde_json::from_value(v).ok());

        let mut dir: Option<PathBuf> = None;
        if let Some(p) = &pane_id {
            if let Ok(info) = client.process_info(p) {
                dir = info.foreground_cwd().map(PathBuf::from);
            }
        }
        if dir.is_none() {
            if let (Some(p), Ok(snap)) = (&pane_id, client.snapshot()) {
                dir = snap.pane(p).and_then(|pane| pane.foreground_cwd.clone().or(pane.cwd.clone())).map(PathBuf::from);
            }
        }
        let dir = dir
            .or_else(|| field("focused_pane_cwd").map(PathBuf::from))
            .or_else(|| field("workspace_cwd").map(PathBuf::from))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));

        let alt_dir = worktree.as_ref().and_then(|wt| translate_worktree(&dir, wt));

        Self {
            workspace_id,
            workspace_label: field("workspace_label"),
            tab_id,
            pane_id,
            agent: field("focused_pane_agent"),
            dir,
            alt_dir,
            raw,
        }
    }

    pub fn to_env_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn invocation_context(&self) -> Value {
        if self.raw.is_object() {
            return self.raw.clone();
        }
        serde_json::json!({
            "workspace_id": self.workspace_id,
            "tab_id": self.tab_id,
            "focused_pane_id": self.pane_id,
            "focused_pane_cwd": self.dir,
        })
    }

    pub fn dir_str(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }
}

pub fn translate_worktree(dir: &Path, wt: &Worktree) -> Option<PathBuf> {
    if !wt.is_linked_worktree || wt.checkout_path.is_empty() || wt.repo_root.is_empty() {
        return None;
    }
    let rel = dir.strip_prefix(&wt.checkout_path).ok()?;
    Some(Path::new(&wt.repo_root).join(rel))
}

pub fn shorten_home(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(home) = std::env::var_os("HOME") {
        let home = home.to_string_lossy();
        if let Some(rest) = s.strip_prefix(home.as_ref()) {
            return format!("~{rest}");
        }
    }
    s.into_owned()
}

// Last two components, "…/" when more were dropped; short paths stay whole.
pub fn short_path(s: &str) -> String {
    let s = shorten_home(Path::new(s));
    let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 3 {
        return s;
    }
    format!("…/{}", parts[parts.len() - 2..].join("/"))
}
