use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub type Env = std::collections::HashMap<String, String>;

#[derive(Clone, Debug)]
pub struct Client {
    path: PathBuf,
    timeout: Duration,
}

impl Client {
    pub fn from_env() -> Self {
        let path = std::env::var_os("HERDR_SOCKET_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
                home.join(".config/herdr/herdr.sock")
            });
        Self { path, timeout: Duration::from_secs(5) }
    }

    /// A forwarded socket of another Machine; a broken ssh forward must not stall the UI for long.
    pub fn at(path: PathBuf) -> Self {
        Self { path, timeout: Duration::from_millis(1500) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut stream = UnixStream::connect(&self.path)
            .with_context(|| format!("connect {}", self.path.display()))?;
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;
        let req = json!({"id": "herdr-raycast", "method": method, "params": params});
        stream.write_all(serde_json::to_string(&req)?.as_bytes())?;
        stream.write_all(b"\n")?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line)?;
        if line.is_empty() {
            bail!("{method}: empty reply from herdr");
        }
        let resp: Value = serde_json::from_str(&line).with_context(|| format!("{method}: bad json"))?;
        if let Some(err) = resp.get("error") {
            let code = err.get("code").and_then(Value::as_str).unwrap_or("error");
            let msg = err.get("message").and_then(Value::as_str).unwrap_or("");
            return Err(anyhow!("{method}: {code}: {msg}"));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let v = self.call("session.snapshot", json!({}))?;
        let snap = v.get("snapshot").cloned().unwrap_or(v);
        Ok(serde_json::from_value(snap)?)
    }

    pub fn focus_pane(&self, pane_id: &str) -> Result<()> {
        self.call("pane.focus", json!({"pane_id": pane_id})).map(|_| ())
    }
    pub fn focus_tab(&self, tab_id: &str) -> Result<()> {
        self.call("tab.focus", json!({"tab_id": tab_id})).map(|_| ())
    }
    pub fn focus_workspace(&self, id: &str) -> Result<()> {
        self.call("workspace.focus", json!({"workspace_id": id})).map(|_| ())
    }
    pub fn focus_agent(&self, target: &str) -> Result<()> {
        self.call("agent.focus", json!({"target": target})).map(|_| ())
    }

    pub fn process_info(&self, pane_id: &str) -> Result<ProcessInfo> {
        let v = self.call("pane.process_info", json!({"pane_id": pane_id}))?;
        Ok(serde_json::from_value(v.get("process_info").cloned().unwrap_or(v))?)
    }

    pub fn read_pane(&self, pane_id: &str, lines: u32) -> Result<String> {
        let v = self.call(
            "pane.read",
            json!({"pane_id": pane_id, "source": "recent_unwrapped", "lines": lines, "format": "text"}),
        )?;
        Ok(v.pointer("/read/text").and_then(Value::as_str).unwrap_or("").to_string())
    }

    pub fn plugin_actions(&self) -> Result<Vec<PluginAction>> {
        let v = self.call("plugin.action.list", json!({}))?;
        Ok(serde_json::from_value(v.get("actions").cloned().unwrap_or(Value::Array(vec![])))?)
    }

    pub fn invoke_action(&self, plugin_id: &str, action_id: &str, context: &Value) -> Result<()> {
        self.call(
            "plugin.action.invoke",
            json!({"plugin_id": plugin_id, "action_id": action_id, "context": context}),
        )
        .map(|_| ())
    }

    pub fn open_plugin_pane(&self, params: Value) -> Result<String> {
        let v = self.call("plugin.pane.open", params)?;
        v.pointer("/plugin_pane/pane/pane_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("plugin.pane.open: no pane id in reply"))
    }

    pub fn close_pane(&self, pane_id: &str) -> Result<()> {
        if self.call("plugin.pane.close", json!({"pane_id": pane_id})).is_ok() {
            return Ok(());
        }
        self.call("pane.close", json!({"pane_id": pane_id})).map(|_| ())
    }

    pub fn run_in_pane(&self, pane_id: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.call("pane.send_text", json!({"pane_id": pane_id, "text": text}))?;
        self.call("pane.send_keys", json!({"pane_id": pane_id, "keys": ["enter"]})).map(|_| ())
    }

    pub fn split(&self, target_pane: &str, direction: &str, cwd: &str, focus: bool, env: &Env) -> Result<String> {
        let v = self.call(
            "pane.split",
            json!({"target_pane_id": target_pane, "direction": direction, "cwd": cwd, "focus": focus, "env": env}),
        )?;
        pane_id_in(&v, "/pane/pane_id")
    }

    pub fn create_tab(&self, workspace_id: &str, cwd: &str, label: Option<&str>, focus: bool, env: &Env) -> Result<Created> {
        let v = self.call(
            "tab.create",
            json!({"workspace_id": workspace_id, "cwd": cwd, "label": label, "focus": focus, "env": env}),
        )?;
        Ok(Created {
            workspace_id: workspace_id.to_string(),
            tab_id: v.pointer("/tab/tab_id").and_then(Value::as_str).unwrap_or("").to_string(),
            pane_id: pane_id_in(&v, "/root_pane/pane_id")?,
        })
    }

    pub fn create_workspace(&self, cwd: &str, label: Option<&str>, focus: bool, env: &Env) -> Result<Created> {
        let v = self.call("workspace.create", json!({"cwd": cwd, "label": label, "focus": focus, "env": env}))?;
        created_from(&v)
    }

    pub fn create_worktree(&self, cwd: &str, branch: &str, base: Option<&str>, label: Option<&str>) -> Result<Created> {
        let v = self.call(
            "worktree.create",
            json!({"cwd": cwd, "branch": branch, "base": base, "label": label, "focus": true}),
        )?;
        created_from(&v)
    }

    pub fn rename_workspace(&self, id: &str, label: &str) -> Result<()> {
        self.call("workspace.rename", json!({"workspace_id": id, "label": label})).map(|_| ())
    }
    pub fn rename_tab(&self, id: &str, label: &str) -> Result<()> {
        self.call("tab.rename", json!({"tab_id": id, "label": label})).map(|_| ())
    }
    pub fn rename_agent(&self, target: &str, name: &str) -> Result<()> {
        self.call("agent.rename", json!({"target": target, "name": name})).map(|_| ())
    }
    pub fn close_workspace(&self, id: &str) -> Result<()> {
        self.call("workspace.close", json!({"workspace_id": id})).map(|_| ())
    }
    pub fn close_tab(&self, id: &str) -> Result<()> {
        self.call("tab.close", json!({"tab_id": id})).map(|_| ())
    }
    pub fn close_plain_pane(&self, id: &str) -> Result<()> {
        self.call("pane.close", json!({"pane_id": id})).map(|_| ())
    }
    pub fn start_agent(&self, name: &str, kind: &str, pane_id: &str) -> Result<()> {
        self.call("agent.start", json!({"name": name, "kind": kind, "pane_id": pane_id})).map(|_| ())
    }
    pub fn notify(&self, title: &str, body: Option<&str>) {
        let _ = self.call("notification.show", json!({"title": title, "body": body}));
    }
}

fn pane_id_in(v: &Value, ptr: &str) -> Result<String> {
    v.pointer(ptr)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no pane id at {ptr} in reply"))
}

fn created_from(v: &Value) -> Result<Created> {
    Ok(Created {
        workspace_id: v.pointer("/workspace/workspace_id").and_then(Value::as_str).unwrap_or("").to_string(),
        tab_id: v.pointer("/tab/tab_id").and_then(Value::as_str).unwrap_or("").to_string(),
        pane_id: pane_id_in(v, "/root_pane/pane_id")?,
    })
}

#[derive(Debug, Clone)]
pub struct Created {
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    #[default]
    Unknown,
}

impl AgentStatus {
    pub fn priority(self) -> u8 {
        match self {
            AgentStatus::Blocked => 0,
            AgentStatus::Done => 1,
            AgentStatus::Working => 2,
            AgentStatus::Idle => 3,
            AgentStatus::Unknown => 4,
        }
    }
    pub fn glyph(self) -> &'static str {
        match self {
            AgentStatus::Blocked => "◉",
            AgentStatus::Done => "✓",
            AgentStatus::Working => "◐",
            AgentStatus::Idle => "●",
            AgentStatus::Unknown => "○",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Working => "working",
            AgentStatus::Idle => "idle",
            AgentStatus::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct Worktree {
    #[serde(default)]
    pub checkout_path: String,
    #[serde(default)]
    pub repo_root: String,
    #[serde(default)]
    pub repo_name: String,
    #[serde(default)]
    pub is_linked_worktree: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub worktree: Option<Worktree>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub pane_count: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct Pane {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct Agent {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub display_agent: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    #[serde(default)]
    pub agent_status: AgentStatus,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub state_labels: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub agent_session: Option<AgentSession>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct AgentSession {
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

impl Agent {
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.display_agent.clone())
            .or_else(|| self.agent.clone())
            .unwrap_or_else(|| self.pane_id.clone())
    }
    pub fn target(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.pane_id)
    }
    pub fn status_text(&self) -> String {
        self.state_labels
            .get(self.agent_status.label())
            .cloned()
            .unwrap_or_else(|| self.agent_status.label().to_string())
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub struct Area {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Layout {
    pub tab_id: String,
    #[serde(default)]
    pub area: Area,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct Snapshot {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    #[serde(default)]
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub layouts: Vec<Layout>,
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
    #[serde(default)]
    pub focused_tab_id: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
}

impl Snapshot {
    pub fn focused_area(&self) -> Option<Area> {
        let tab = self.focused_tab_id.as_deref()?;
        self.layouts.iter().find(|l| l.tab_id == tab).map(|l| l.area)
    }
    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.workspace_id == id)
    }
    pub fn tab(&self, id: &str) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.tab_id == id)
    }
    pub fn pane(&self, id: &str) -> Option<&Pane> {
        self.panes.iter().find(|p| p.pane_id == id)
    }
    pub fn agent_in_pane(&self, pane_id: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.pane_id == pane_id)
    }
    pub fn workspace_label(&self, id: &str) -> String {
        self.workspace(id).map(|w| w.label.clone()).unwrap_or_else(|| id.to_string())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct ProcessInfo {
    #[serde(default)]
    pub foreground_process_group_id: Option<u32>,
    #[serde(default)]
    pub foreground_processes: Vec<ProcessEntry>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct ProcessEntry {
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub argv0: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

impl ProcessInfo {
    pub fn foreground_cwd(&self) -> Option<String> {
        let pgid = self.foreground_process_group_id?;
        self.foreground_processes
            .iter()
            .find(|p| p.pid == Some(pgid))
            .and_then(|p| p.cwd.clone())
            .or_else(|| self.foreground_processes.first().and_then(|p| p.cwd.clone()))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct PluginAction {
    pub plugin_id: String,
    pub action_id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub contexts: Vec<String>,
}
