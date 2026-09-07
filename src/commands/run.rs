use super::builtins::Builtin;
use super::{files, Class, Command, Target};
use crate::config::{self, Config};
use crate::context::Captured;
use crate::herdr::{Client, Env};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ItemRef {
    Workspace(String),
    Tab { tab_id: String, workspace_id: String },
    Pane { pane_id: String, tab_id: String, workspace_id: String, has_agent: bool },
    Agent { pane_id: String, target: String, workspace_id: String },
}

impl ItemRef {
    pub fn id(&self) -> &str {
        match self {
            ItemRef::Workspace(id) => id,
            ItemRef::Tab { tab_id, .. } => tab_id,
            ItemRef::Pane { pane_id, .. } => pane_id,
            ItemRef::Agent { pane_id, .. } => pane_id,
        }
    }
    pub fn workspace_id(&self) -> &str {
        match self {
            ItemRef::Workspace(id) => id,
            ItemRef::Tab { workspace_id, .. } | ItemRef::Pane { workspace_id, .. } | ItemRef::Agent { workspace_id, .. } => workspace_id,
        }
    }
    pub fn pane_id(&self) -> Option<&str> {
        match self {
            ItemRef::Pane { pane_id, .. } | ItemRef::Agent { pane_id, .. } => Some(pane_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub command: Command,
    pub answers: HashMap<String, String>,
    pub ctx: Captured,
    pub item: Option<ItemRef>,
    pub palette_pane: Option<String>,
}

pub struct Runtime<'a> {
    pub client: &'a Client,
    pub cfg: &'a Config,
    pub plugin_id: String,
}

impl Plan {
    fn get(&self, key: &str) -> Option<String> {
        self.answers.get(key).cloned().filter(|s| !s.is_empty())
    }

    pub fn vars(&self) -> Vec<(String, String)> {
        let mut v = vec![
            ("ctx.dir".into(), self.ctx.dir_str()),
            ("ctx.workspace_id".into(), self.ctx.workspace_id.clone().unwrap_or_default()),
            ("ctx.tab_id".into(), self.ctx.tab_id.clone().unwrap_or_default()),
            ("ctx.pane_id".into(), self.ctx.pane_id.clone().unwrap_or_default()),
            ("ctx.agent".into(), self.ctx.agent.clone().unwrap_or_default()),
            ("item.id".into(), self.item.as_ref().map(|i| i.id().to_string()).unwrap_or_default()),
        ];
        for (k, val) in &self.answers {
            v.push((format!("input.{k}"), val.clone()));
        }
        v
    }

    pub fn render(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (k, v) in self.vars() {
            out = out.replace(&format!("{{{{{k}}}}}"), &v);
        }
        out
    }

    pub fn env(&self) -> Env {
        let mut env = Env::new();
        for (k, v) in self.vars() {
            let key = format!("RAYCAST_{}", k.replace('.', "_").to_uppercase());
            env.insert(key, v);
        }
        if let Some(ws) = &self.ctx.workspace_id {
            env.insert("HERDR_WORKSPACE_ID".into(), ws.clone());
        }
        if let Some(t) = &self.ctx.tab_id {
            env.insert("HERDR_TAB_ID".into(), t.clone());
        }
        if let Some(p) = &self.ctx.pane_id {
            env.insert("HERDR_PANE_ID".into(), p.clone());
        }
        env
    }

    pub fn cwd(&self) -> String {
        self.command.cwd.as_deref().map(|c| files::expand_home(&self.render(c))).unwrap_or_else(|| self.ctx.dir_str())
    }

    pub fn resolved_run(&self) -> String {
        self.render(&self.command.run)
    }

    pub fn label(&self) -> Option<String> {
        self.command.label.as_deref().map(|l| self.render(l)).filter(|l| !l.is_empty())
    }
}

/// Runs the plan now. Returns a one-line status message.
pub fn execute(plan: &Plan, rt: &Runtime) -> Result<String> {
    match &plan.command.class {
        Class::Builtin(b) => builtin(*b, plan, rt),
        Class::PluginAction { plugin_id, action_id } => {
            rt.client.invoke_action(plugin_id, action_id, &plan.ctx.invocation_context())?;
            Ok(format!("ran {plugin_id}.{action_id}"))
        }
        Class::User { .. } => run_shell(plan, rt, &plan.resolved_run(), plan.command.target),
    }
}

fn run_shell(plan: &Plan, rt: &Runtime, run: &str, target: Target) -> Result<String> {
    let cwd = plan.cwd();
    let env = plan.env();
    let ws = plan.item.as_ref().map(|i| i.workspace_id().to_string()).or_else(|| plan.ctx.workspace_id.clone());
    let pane = plan.item.as_ref().and_then(|i| i.pane_id().map(str::to_string)).or_else(|| plan.ctx.pane_id.clone());
    match target {
        Target::Background => {
            spawn_detached(run, &cwd, &env)?;
            Ok(format!("started: {run}"))
        }
        Target::CurrentPane => {
            let pane = pane.ok_or_else(|| anyhow!("no pane to run in"))?;
            rt.client.run_in_pane(&pane, run)?;
            Ok(format!("ran in {pane}"))
        }
        Target::Split => {
            let pane = pane.ok_or_else(|| anyhow!("no pane to split"))?;
            let new = rt.client.split(&pane, "right", &cwd, true, &env)?;
            rt.client.run_in_pane(&new, run)?;
            Ok(format!("split {new}"))
        }
        Target::Tab => {
            let ws = ws.ok_or_else(|| anyhow!("no workspace"))?;
            let created = rt.client.create_tab(&ws, &cwd, plan.label().as_deref(), true, &env)?;
            rt.client.run_in_pane(&created.pane_id, run)?;
            Ok(format!("opened tab {}", created.tab_id))
        }
        Target::Workspace => {
            let created = rt.client.create_workspace(&cwd, plan.label().as_deref(), true, &env)?;
            rt.client.run_in_pane(&created.pane_id, run)?;
            Ok(format!("opened workspace {}", created.workspace_id))
        }
        Target::Popup => {
            let mut env = env;
            env.insert("RAYCAST_RUN".into(), run.to_string());
            rt.client.open_plugin_pane(serde_json::json!({
                "plugin_id": rt.plugin_id, "entrypoint": "runner", "placement": "popup",
                "cwd": cwd, "env": env, "focus": true,
                "width": "80%", "height": "80%",
            }))?;
            Ok("opened popup".into())
        }
    }
}

pub fn spawn_detached(run: &str, cwd: &str, env: &Env) -> Result<()> {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(config::state_dir().join("commands.log"))
        .ok();
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(run).current_dir(cwd).envs(env).stdin(Stdio::null());
    match log {
        Some(f) => {
            cmd.stdout(Stdio::from(f.try_clone()?)).stderr(Stdio::from(f));
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    cmd.process_group(0);
    cmd.spawn().with_context(|| format!("spawn {run}"))?;
    Ok(())
}

fn builtin(b: Builtin, plan: &Plan, rt: &Runtime) -> Result<String> {
    let c = rt.client;
    let item = plan.item.as_ref();
    let ws = || item.map(|i| i.workspace_id().to_string()).or_else(|| plan.get("workspace")).or_else(|| plan.ctx.workspace_id.clone());
    match b {
        Builtin::FocusWorkspace => {
            let id = ws().ok_or_else(|| anyhow!("no workspace"))?;
            c.focus_workspace(&id)?;
            Ok(format!("focused {id}"))
        }
        Builtin::FocusTab => {
            let id = match item {
                Some(ItemRef::Tab { tab_id, .. }) => tab_id.clone(),
                _ => plan.get("tab").ok_or_else(|| anyhow!("no tab"))?,
            };
            c.focus_tab(&id)?;
            Ok(format!("focused {id}"))
        }
        Builtin::FocusPane => {
            let id = item.and_then(|i| i.pane_id().map(str::to_string)).or_else(|| plan.get("pane")).ok_or_else(|| anyhow!("no pane"))?;
            c.focus_pane(&id)?;
            Ok(format!("focused {id}"))
        }
        Builtin::FocusAgent => {
            let target = match item {
                Some(ItemRef::Agent { target, .. }) => target.clone(),
                _ => plan.get("agent").ok_or_else(|| anyhow!("no agent"))?,
            };
            c.focus_agent(&target)?;
            Ok(format!("focused {target}"))
        }
        Builtin::NewWorkspace => {
            let created = c.create_workspace(&plan.ctx.dir_str(), plan.get("label").as_deref(), true, &plan.env())?;
            Ok(format!("workspace {}", created.workspace_id))
        }
        Builtin::NewWorktree => {
            let branch = plan.get("branch").ok_or_else(|| anyhow!("branch name required"))?;
            let created = c.create_worktree(&plan.ctx.dir_str(), &branch, plan.get("base").as_deref(), Some(&branch))?;
            Ok(format!("worktree workspace {}", created.workspace_id))
        }
        Builtin::NewTab => {
            let ws = ws().ok_or_else(|| anyhow!("no workspace"))?;
            let created = c.create_tab(&ws, &plan.ctx.dir_str(), plan.get("label").as_deref(), true, &plan.env())?;
            Ok(format!("tab {}", created.tab_id))
        }
        Builtin::RenameWorkspace => {
            let id = ws().ok_or_else(|| anyhow!("no workspace"))?;
            c.rename_workspace(&id, &plan.get("label").unwrap_or_default())?;
            Ok("renamed".into())
        }
        Builtin::RenameTab => {
            let id = match item {
                Some(ItemRef::Tab { tab_id, .. }) => tab_id.clone(),
                Some(ItemRef::Pane { tab_id, .. }) => tab_id.clone(),
                _ => plan.get("tab").or_else(|| plan.ctx.tab_id.clone()).ok_or_else(|| anyhow!("no tab"))?,
            };
            c.rename_tab(&id, &plan.get("label").unwrap_or_default())?;
            Ok("renamed".into())
        }
        Builtin::RenameAgent => {
            let target = match item {
                Some(ItemRef::Agent { target, .. }) => target.clone(),
                _ => plan.get("agent").ok_or_else(|| anyhow!("no agent"))?,
            };
            let name = agent_slug(&plan.get("name").unwrap_or_default());
            if name.is_empty() {
                bail!("name required");
            }
            c.rename_agent(&target, &name)?;
            Ok(format!("renamed to {name}"))
        }
        Builtin::CloseWorkspace => {
            let id = ws().ok_or_else(|| anyhow!("no workspace"))?;
            c.close_workspace(&id)?;
            Ok(format!("closed {id}"))
        }
        Builtin::CloseTab => {
            let id = match item {
                Some(ItemRef::Tab { tab_id, .. }) => tab_id.clone(),
                _ => plan.get("tab").ok_or_else(|| anyhow!("no tab"))?,
            };
            c.close_tab(&id)?;
            Ok(format!("closed {id}"))
        }
        Builtin::ClosePane => {
            let id = item.and_then(|i| i.pane_id().map(str::to_string)).or_else(|| plan.get("pane")).ok_or_else(|| anyhow!("no pane"))?;
            c.close_plain_pane(&id)?;
            Ok(format!("closed {id}"))
        }
        Builtin::StartAgent => {
            let name = plan.get("name").map(|n| agent_name(&n)).unwrap_or_else(|| agent_name("agent"));
            let pane = match item {
                Some(ItemRef::Pane { pane_id, has_agent: false, .. }) => pane_id.clone(),
                Some(ItemRef::Workspace(id)) => {
                    let created = c.create_tab(id, &plan.ctx.dir_str(), Some(&name), true, &Env::new())?;
                    created.pane_id
                }
                Some(other) => {
                    let base = other.pane_id().map(str::to_string).or_else(|| plan.ctx.pane_id.clone()).ok_or_else(|| anyhow!("no pane"))?;
                    c.split(&base, "right", &plan.ctx.dir_str(), true, &Env::new())?
                }
                None => {
                    let base = plan.ctx.pane_id.clone().ok_or_else(|| anyhow!("no pane"))?;
                    c.split(&base, "right", &plan.ctx.dir_str(), true, &Env::new())?
                }
            };
            c.start_agent(&name, &rt.cfg.agent_kind, &pane)?;
            Ok(format!("started {name} in {pane}"))
        }
        Builtin::EditCommandFiles => {
            let file = plan.get("file").map(PathBuf::from).unwrap_or_else(config::commands_dir);
            std::fs::create_dir_all(config::commands_dir())?;
            open_editor(plan, rt, &file)
        }
        Builtin::NewCommandFile => {
            let name = files::sanitize_name(&plan.get("name").unwrap_or_default());
            let dir = config::commands_dir();
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{name}.toml"));
            if path.exists() {
                bail!("{} already exists", path.display());
            }
            std::fs::write(&path, files::template(&name, &plan.ctx.dir))?;
            open_editor(plan, rt, &path)
        }
    }
}

fn open_editor(plan: &Plan, rt: &Runtime, file: &Path) -> Result<String> {
    let file_s = file.display().to_string();
    let quoted = shell_quote(&file_s);
    if rt.cfg.editor == "auto" {
        if let Some(nvim_open) = which("nvim-open") {
            spawn_detached(&format!("{} {quoted}", nvim_open.display()), &plan.ctx.dir_str(), &plan.env())?;
            return Ok(format!("opened {file_s} in nvim"));
        }
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
        return run_shell(plan, rt, &format!("{editor} {quoted}"), Target::Popup);
    }
    let cmd = if rt.cfg.editor.contains("{{file}}") {
        rt.cfg.editor.replace("{{file}}", &quoted)
    } else {
        format!("{} {quoted}", rt.cfg.editor)
    };
    run_shell(plan, rt, &cmd, Target::Popup)
}

pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|p| p.join(bin)).find(|p| p.is_file())
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Herdr agent names: `[a-z][a-z0-9_-]*`, at most 32 chars. Empty when nothing usable remains.
pub fn agent_slug(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    let mut out = if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_lowercase()) { out } else { format!("a{out}") };
    out.truncate(32);
    out
}

pub fn agent_name(raw: &str) -> String {
    let mut out = agent_slug(raw);
    if out.is_empty() {
        out.push('a');
    }
    out.truncate(26);
    let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() % 10000).unwrap_or(0);
    format!("{out}-{suffix:04}")
}

/// Runs the plan after the Palette pane has closed, from a detached child process.
pub fn defer(plan: &Plan) -> Result<()> {
    let dir = config::state_dir().join("plans");
    std::fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
    let path = dir.join(format!("{stamp}.json"));
    std::fs::write(&path, serde_json::to_string(plan)?)?;
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("exec").arg(&path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.process_group(0);
    cmd.spawn()?;
    Ok(())
}

pub fn exec_deferred(path: &Path) -> Result<()> {
    let plan: Plan = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let _ = std::fs::remove_file(path);
    let client = Client::from_env();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) && crate::state::is_open() {
        std::thread::sleep(Duration::from_millis(40));
    }
    if let Some(pane) = plan.palette_pane.as_deref().filter(|p| *p != crate::state::POPUP) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match client.snapshot() {
                Ok(s) if s.pane(pane).is_none() => break,
                Ok(_) => std::thread::sleep(Duration::from_millis(40)),
                Err(_) => break,
            }
        }
    }
    std::thread::sleep(Duration::from_millis(60));
    let cfg = config::load();
    let rt = Runtime { client: &client, cfg: &cfg, plugin_id: plugin_id() };
    match execute(&plan, &rt) {
        Ok(msg) => log(&format!("ok {}: {msg}", plan.command.id)),
        Err(e) => {
            log(&format!("error {}: {e:#}", plan.command.id));
            client.notify(&format!("{} failed", plan.command.title), Some(&format!("{e:#}")));
        }
    }
    Ok(())
}

pub fn plugin_id() -> String {
    std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "erling.command-palette".into())
}

pub fn log(line: &str) {
    let _ = std::fs::create_dir_all(config::state_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(config::state_dir().join("palette.log")) {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::builtins;

    #[test]
    fn renders_templates_and_env() {
        let mut plan = Plan {
            command: builtins::all().remove(0),
            answers: HashMap::from([("branch".to_string(), "feat".to_string())]),
            ctx: Captured { dir: PathBuf::from("/tmp/x"), workspace_id: Some("w1".into()), ..Default::default() },
            item: None,
            palette_pane: None,
        };
        plan.command.run = "git switch {{input.branch}} # {{ctx.dir}} {{ctx.workspace_id}}".into();
        assert_eq!(plan.resolved_run(), "git switch feat # /tmp/x w1");
        let env = plan.env();
        assert_eq!(env.get("RAYCAST_INPUT_BRANCH").map(String::as_str), Some("feat"));
        assert_eq!(env.get("RAYCAST_CTX_DIR").map(String::as_str), Some("/tmp/x"));
    }

    #[test]
    fn agent_names_are_valid() {
        let n = agent_name("My Agent!");
        assert!(n.starts_with("my-agent-"));
        assert!(n.len() <= 32);
        assert!(agent_name("9x").starts_with("a9x"));
        assert_eq!(agent_slug("Spain Outreach"), "spain-outreach");
        assert_eq!(agent_slug("!!"), "");
    }
}
