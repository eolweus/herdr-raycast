use crate::commands::builtins::{self, Builtin};
use crate::commands::run::{self, ItemRef, Plan, Runtime};
use crate::commands::{resolve, Class, Command, Input, InputKind, ItemKind};
use crate::config::{self, Config};
use crate::context::Captured;
use crate::herdr::{AgentStatus, Client, Snapshot};
use crate::keys::{self, PaletteKey};
use crate::machines::Machines;
use crate::search::Search;
use crate::state;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    All,
    Agents,
    Workspaces,
    Commands,
}

impl View {
    pub const ALL: [View; 4] = [View::All, View::Agents, View::Workspaces, View::Commands];
    pub fn title(self) -> &'static str {
        match self {
            View::All => "All",
            View::Agents => "Agents",
            View::Workspaces => "Workspaces",
            View::Commands => "Commands",
        }
    }
    pub fn parse(s: &str) -> Option<View> {
        match s.to_lowercase().as_str() {
            "all" => Some(View::All),
            "agents" => Some(View::Agents),
            "workspaces" => Some(View::Workspaces),
            "commands" => Some(View::Commands),
            _ => None,
        }
    }
    pub fn index(self) -> usize {
        View::ALL.iter().position(|v| *v == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Insert,
    Normal,
}

#[derive(Clone, Debug)]
pub enum Kind {
    Workspace,
    Tab,
    Pane,
    Agent,
    Command(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Dim,
    Path,
    Agent,
    Workspace,
    Machine,
    Status(AgentStatus),
    Title,
    Heading,
    Strong,
    Em,
    Code,
    Rule,
}

#[derive(Clone, Debug)]
pub struct Seg(pub String, pub Tone);

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub meta: Vec<Seg>,
    pub status: Option<AgentStatus>,
    pub depth: u8,
    pub current: bool,
    pub hay: Vec<String>,
    pub item_ref: Option<ItemRef>,
    /// None for the Machine the Palette runs on.
    pub machine: Option<String>,
}

impl Item {
    // Names outlive pane and workspace ids across restarts, so usage keys on them.
    pub fn usage_key(&self) -> String {
        let key = match self.kind {
            Kind::Command(_) => return self.id.clone(),
            Kind::Workspace => format!("ws:{}", self.hay.first().cloned().unwrap_or_default()),
            Kind::Tab => format!("tab:{}", self.hay.first().cloned().unwrap_or_default()),
            Kind::Pane | Kind::Agent => format!("agent:{}", self.hay.first().cloned().unwrap_or_default()),
        };
        scoped(key, self.machine.as_deref())
    }
}

/// Ids are per server, so an Item from another Machine carries its name.
fn scoped(id: String, machine: Option<&str>) -> String {
    match machine {
        Some(m) => format!("{id}@{m}"),
        None => id,
    }
}

impl Kind {
    pub fn section(&self) -> (usize, &'static str) {
        match self {
            Kind::Agent => (0, "Agents"),
            Kind::Workspace | Kind::Tab | Kind::Pane => (1, "Workspaces"),
            Kind::Command(_) => (2, "Commands"),
        }
    }
}

// Herdr's generated tab labels ("1 · dir › agent › title") fuzzy-match almost any query.
fn item_kind(k: &Kind) -> Option<ItemKind> {
    match k {
        Kind::Workspace => Some(ItemKind::Workspace),
        Kind::Tab => Some(ItemKind::Tab),
        Kind::Pane => Some(ItemKind::Pane),
        Kind::Agent => Some(ItemKind::Agent),
        Kind::Command(_) => None,
    }
}

fn manual_tab_label(t: &crate::herdr::Tab) -> Option<String> {
    let auto = t.label.split_once(" · ").is_some_and(|(n, _)| n.parse::<u32>().is_ok());
    (!auto && !t.label.is_empty()).then(|| t.label.clone())
}

fn seg(text: impl Into<String>, tone: Tone) -> Seg {
    Seg(text.into(), tone)
}

fn machine_seg(machine: Option<&str>) -> Vec<Seg> {
    machine.map(|m| vec![seg(m, Tone::Machine)]).unwrap_or_default()
}

fn activity(statuses: impl Iterator<Item = AgentStatus>) -> Vec<Seg> {
    let (mut blocked, mut working, mut done) = (0, 0, 0);
    for s in statuses {
        match s {
            AgentStatus::Blocked => blocked += 1,
            AgentStatus::Working => working += 1,
            AgentStatus::Done => done += 1,
            _ => {}
        }
    }
    let mut out = vec![];
    if blocked > 0 { out.push(seg(format!("{blocked} blocked"), Tone::Status(AgentStatus::Blocked))); }
    if working > 0 { out.push(seg(format!("{working} working"), Tone::Status(AgentStatus::Working))); }
    if done > 0 { out.push(seg(format!("{done} done"), Tone::Status(AgentStatus::Done))); }
    out
}

#[derive(Clone, Debug)]
pub struct Draft {
    pub command: Command,
    pub item: Option<ItemRef>,
    pub machine: Option<String>,
    pub answers: HashMap<String, String>,
}

pub enum Flow {
    Menu { actions: Vec<(Command, Option<ItemRef>)>, machine: Option<String>, cursor: usize, query: String },
    Input { draft: Draft, step: usize, text: String, options: Vec<(String, String)>, query: String, cursor: usize },
    Confirm { draft: Draft },
}

const TOP_MATCHES: usize = 3;

pub struct App {
    pub client: Client,
    pub cfg: Config,
    pub ctx: Captured,
    pub snapshot: Snapshot,
    pub machines: Machines,
    pub commands: Vec<Command>,
    pub command_errors: Vec<String>,
    pub command_files: Vec<PathBuf>,
    pub view: View,
    pub mode: Mode,
    pub query: String,
    pub items: Vec<Item>,
    pub filtered: Vec<usize>,
    pub context: Vec<bool>,
    pub top: usize,
    pub cursor: usize,
    pub preview: bool,
    pub preview_cache: Option<(String, String, Instant)>,
    pub flow: Option<Flow>,
    pub status: Option<String>,
    pub quit: bool,
    pub edit_in_editor: bool,
    pub usage: state::Usage,
    search: Search,
    last_poll: Instant,
    plugin_id: String,
    palette_pane: Option<String>,
    palette_keys: Vec<PaletteKey>,
}

impl App {
    pub fn new(view: Option<View>) -> Result<Self> {
        let client = Client::from_env();
        let cfg = config::load();
        let ctx = Captured::from_env().unwrap_or_else(|| Captured::capture(&client));
        let plugin_id = run::plugin_id();
        let palette_keys = keys::palette_keys(&plugin_id);
        let snapshot = client.snapshot()?;
        let resolved = resolve::resolve(&client, &ctx, &plugin_id);
        let st = state::load();
        let machines = Machines::new(cfg.machines_dir(), client.path(), Duration::from_millis(cfg.remote_poll_ms.max(250)));
        let mut app = Self {
            client,
            cfg,
            ctx,
            snapshot,
            machines,
            commands: resolved.commands,
            command_errors: resolved.errors,
            command_files: resolved.files,
            view: view.unwrap_or(View::All),
            mode: Mode::Insert,
            query: String::new(),
            items: vec![],
            filtered: vec![],
            context: vec![],
            top: 0,
            cursor: 0,
            preview: st.preview,
            preview_cache: None,
            flow: None,
            status: None,
            quit: false,
            edit_in_editor: false,
            usage: state::Usage::load(),
            search: Search::default(),
            last_poll: Instant::now(),
            plugin_id,
            palette_keys,
            palette_pane: std::env::var("HERDR_PANE_ID").ok(),
        };
        if let Some(e) = app.command_errors.first() {
            app.status = Some(format!("command file error: {e}"));
        }
        app.rebuild();
        Ok(app)
    }

    pub fn refresh(&mut self) {
        if let Ok(s) = self.client.snapshot() {
            self.snapshot = s;
        }
        self.machines.tick();
        let keep = self.selected().map(|i| i.id.clone());
        self.rebuild();
        if let Some(id) = keep {
            if let Some(pos) = self.filtered.iter().position(|&i| self.items[i].id == id) {
                self.cursor = pos;
            }
        }
    }

    pub fn tick(&mut self) {
        if self.last_poll.elapsed() >= Duration::from_millis(self.cfg.poll_ms) {
            self.last_poll = Instant::now();
            self.refresh();
        }
        if self.preview {
            self.update_preview();
        }
    }

    fn rebuild(&mut self) {
        self.items = match self.view {
            View::All => self.all_items(),
            View::Agents => self.agent_items(),
            View::Workspaces => self.workspace_items(),
            View::Commands => self.command_items(),
        };
        self.apply_filter();
    }

    fn apply_filter(&mut self) {
        let hays: Vec<&[String]> = self.items.iter().map(|i| i.hay.as_slice()).collect();
        let usage: Vec<u32> = self.items.iter().map(|i| self.usage.count(&i.usage_key())).collect();
        let ranked = self.search.rank_used(&self.query, hays.into_iter(), &usage);
        self.context.clear();
        self.top = 0;
        if self.query.trim().is_empty() {
            self.filtered = ranked;
        } else if self.view == View::Workspaces {
            // Workspaces order by their best match; inside one the tree order holds and
            // ancestors of a match stay as dimmed context.
            let mut best = vec![usize::MAX; self.items.len()];
            for (pos, &i) in ranked.iter().enumerate() {
                let root = (0..=i).rev().find(|&j| self.items[j].depth == 0).unwrap_or(0);
                best[root] = best[root].min(pos);
            }
            let matched: std::collections::HashSet<usize> = ranked.into_iter().collect();
            let mut keep = vec![false; self.items.len()];
            for &i in &matched {
                keep[i] = true;
                let mut depth = self.items[i].depth;
                for j in (0..i).rev() {
                    if depth == 0 {
                        break;
                    }
                    if self.items[j].depth < depth {
                        keep[j] = true;
                        depth = self.items[j].depth;
                    }
                }
            }
            let mut groups: Vec<Vec<usize>> = vec![];
            for i in (0..self.items.len()).filter(|&i| keep[i]) {
                if self.items[i].depth == 0 || groups.is_empty() {
                    groups.push(vec![]);
                }
                groups.last_mut().unwrap().push(i);
            }
            groups.sort_by_key(|g| best[g[0]]);
            self.filtered = groups.concat();
            self.context = self.filtered.iter().map(|i| !matched.contains(i)).collect();
            if self.cursor == 0 {
                if let Some(p) = self.context.iter().position(|c| !c) {
                    self.cursor = p;
                }
            }
        } else if self.view == View::All {
            // Best matches lead regardless of section; the rest stay grouped.
            let mut v = ranked;
            self.top = v.len().min(TOP_MATCHES);
            v[self.top..].sort_by_key(|&i| self.items[i].kind.section().0);
            self.filtered = v;
        } else {
            self.filtered = ranked;
        }
        if self.cursor >= self.filtered.len() {
            self.cursor = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn selected(&self) -> Option<&Item> {
        self.filtered.get(self.cursor).map(|&i| &self.items[i])
    }

    pub fn snapshot_of(&self, machine: Option<&str>) -> &Snapshot {
        machine.and_then(|m| self.machines.get(m)).map(|m| &m.snapshot).unwrap_or(&self.snapshot)
    }

    pub fn client_of(&self, machine: Option<&str>) -> &Client {
        machine.and_then(|m| self.machines.get(m)).map(|m| &m.client).unwrap_or(&self.client)
    }

    /// The local snapshot first, then every connected Machine.
    fn sources(&self) -> Vec<(Option<&str>, &Snapshot)> {
        let mut v = vec![(None, &self.snapshot)];
        v.extend(self.machines.connected().map(|m| (Some(m.name.as_str()), &m.snapshot)));
        v
    }

    fn agent_items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self.sources().into_iter().flat_map(|(m, s)| self.agents_of(m, s)).collect();
        items.sort_by_key(|i| i.status.unwrap_or_default().priority());
        items
    }

    fn agents_of(&self, machine: Option<&str>, snap: &Snapshot) -> Vec<Item> {
        snap.agents
            .iter()
            .map(|a| {
                let ws = snap.workspace_label(&a.workspace_id);
                let title = a.terminal_title_stripped.clone().or_else(|| a.title.clone()).unwrap_or_default();
                let kind = a.display_agent.clone().or_else(|| a.agent.clone()).unwrap_or_default();
                let mut meta = machine_seg(machine);
                let row_title = match &a.name {
                    Some(n) => {
                        if !title.is_empty() && title != *n {
                            meta.push(seg(title.clone(), Tone::Dim));
                        }
                        n.clone()
                    }
                    None if !title.is_empty() => title.clone(),
                    None => kind.clone(),
                };
                if !kind.is_empty() {
                    meta.push(seg(kind.clone(), Tone::Agent));
                }
                meta.push(seg(a.agent_status.label(), Tone::Status(a.agent_status)));
                meta.push(seg(ws.clone(), Tone::Workspace));
                Item {
                    kind: Kind::Agent,
                    id: scoped(format!("agent:{}", a.pane_id), machine),
                    title: row_title,
                    meta,
                    status: Some(a.agent_status),
                    depth: 0,
                    current: machine.is_none() && self.ctx.pane_id.as_deref() == Some(a.pane_id.as_str()),
                    hay: [a.display_name(), title, ws, kind].into_iter().chain(machine.map(str::to_string)).collect(),
                    item_ref: Some(ItemRef::Agent { pane_id: a.pane_id.clone(), target: a.target().to_string(), workspace_id: a.workspace_id.clone() }),
                    machine: machine.map(str::to_string),
                }
            })
            .collect()
    }

    fn workspace_dir(snap: &Snapshot, w: &crate::herdr::Workspace) -> String {
        w.worktree
            .as_ref()
            .map(|wt| wt.checkout_path.clone())
            .or_else(|| w.cwd.clone())
            .or_else(|| snap.panes.iter().find(|p| p.workspace_id == w.workspace_id).and_then(|p| p.cwd.clone()))
            .unwrap_or_default()
    }

    fn workspace_row(&self, machine: Option<&str>, snap: &Snapshot, w: &crate::herdr::Workspace) -> Item {
        let dir = Self::workspace_dir(snap, w);
        let panes: Vec<&crate::herdr::Pane> = snap.panes.iter().filter(|p| p.workspace_id == w.workspace_id).collect();
        let mut meta = machine_seg(machine);
        meta.extend(activity(panes.iter().filter(|p| snap.agent_in_pane(&p.pane_id).is_some()).map(|p| p.agent_status)));
        if !dir.is_empty() {
            meta.push(seg(crate::context::short_path(&dir), Tone::Path));
        }
        Item {
            kind: Kind::Workspace,
            id: scoped(format!("ws:{}", w.workspace_id), machine),
            title: format!("{} ({})", w.label, panes.len()),
            meta,
            status: None,
            depth: 0,
            current: machine.is_none() && self.ctx.workspace_id.as_deref() == Some(w.workspace_id.as_str()),
            hay: [w.label.clone(), dir.rsplit('/').next().unwrap_or("").to_string()].into_iter().chain(machine.map(str::to_string)).collect(),
            item_ref: Some(ItemRef::Workspace(w.workspace_id.clone())),
            machine: machine.map(str::to_string),
        }
    }

    fn workspace_items(&self) -> Vec<Item> {
        self.sources().into_iter().flat_map(|(m, s)| self.tree_of(m, s)).collect()
    }

    fn tree_of(&self, machine: Option<&str>, snap: &Snapshot) -> Vec<Item> {
        let mut out = vec![];
        for w in &snap.workspaces {
            out.push(self.workspace_row(machine, snap, w));
            let tabs: Vec<&crate::herdr::Tab> = snap.tabs.iter().filter(|t| t.workspace_id == w.workspace_id).collect();
            let show_tabs = tabs.len() > 1;
            for t in tabs {
                let panes: Vec<&crate::herdr::Pane> = snap.panes.iter().filter(|p| p.tab_id == t.tab_id).collect();
                if show_tabs {
                    let mut meta = vec![seg(format!("{} panes", panes.len()), Tone::Dim)];
                    meta.extend(activity(panes.iter().filter(|p| snap.agent_in_pane(&p.pane_id).is_some()).map(|p| p.agent_status)));
                    out.push(Item {
                        kind: Kind::Tab,
                        id: scoped(format!("tab:{}", t.tab_id), machine),
                        title: t.label.clone(),
                        meta,
                        status: None,
                        depth: 1,
                        current: false,
                        hay: manual_tab_label(t).into_iter().chain([w.label.clone()]).chain(machine.map(str::to_string)).collect(),
                        item_ref: Some(ItemRef::Tab { tab_id: t.tab_id.clone(), workspace_id: w.workspace_id.clone() }),
                        machine: machine.map(str::to_string),
                    });
                }
                for p in panes {
                    let agent = snap.agent_in_pane(&p.pane_id);
                    let name = p
                        .label
                        .clone()
                        .or_else(|| agent.and_then(|a| a.name.clone()))
                        .or_else(|| p.terminal_title_stripped.clone().filter(|t| !t.is_empty()))
                        .or_else(|| agent.and_then(|a| a.display_agent.clone().or_else(|| a.agent.clone())))
                        .unwrap_or_else(|| "shell".into());
                    let cwd = p.foreground_cwd.clone().or_else(|| p.cwd.clone()).unwrap_or_default();
                    let mut meta = vec![];
                    if let Some(a) = agent {
                        if let Some(k) = a.display_agent.clone().or_else(|| a.agent.clone()) {
                            meta.push(seg(k, Tone::Agent));
                        }
                        meta.push(seg(p.agent_status.label(), Tone::Status(p.agent_status)));
                    }
                    if !cwd.is_empty() {
                        meta.push(seg(crate::context::short_path(&cwd), Tone::Path));
                    }
                    out.push(Item {
                        kind: Kind::Pane,
                        id: scoped(format!("pane:{}", p.pane_id), machine),
                        title: name.clone(),
                        meta,
                        status: agent.map(|_| p.agent_status),
                        depth: if show_tabs { 2 } else { 1 },
                        current: machine.is_none() && self.ctx.pane_id.as_deref() == Some(p.pane_id.as_str()),
                        hay: [name].into_iter().chain(manual_tab_label(t)).chain([w.label.clone(), cwd.rsplit('/').next().unwrap_or("").to_string()]).chain(machine.map(str::to_string)).collect(),
                        item_ref: Some(ItemRef::Pane { pane_id: p.pane_id.clone(), tab_id: t.tab_id.clone(), workspace_id: w.workspace_id.clone(), has_agent: agent.is_some() }),
                        machine: machine.map(str::to_string),
                    });
                }
            }
        }
        out
    }

    fn command_items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self
            .commands
            .iter()
            .enumerate()
            .map(|(i, c)| Item {
                kind: Kind::Command(i),
                id: format!("cmd:{}", c.id),
                title: c.title.clone(),
                meta: vec![seg(c.class.label(), Tone::Agent), seg(c.source.rsplit('/').next().unwrap_or(&c.source), Tone::Dim)],
                status: None,
                depth: 0,
                current: false,
                hay: c.search_text(),
                item_ref: None,
                machine: None,
            })
            .collect();
        items.sort_by_key(|i| std::cmp::Reverse(self.usage.count(&i.usage_key())));
        items
    }

    fn all_items(&self) -> Vec<Item> {
        let mut out = self.agent_items();
        for (m, s) in self.sources() {
            out.extend(s.workspaces.iter().map(|w| self.workspace_row(m, s, w)));
        }
        out.extend(self.command_items());
        out
    }

    pub fn set_view(&mut self, v: View) {
        if self.view != v {
            self.view = v;
            self.cursor = 0;
            self.rebuild();
        }
    }

    fn cycle_view(&mut self, delta: i32) {
        let n = View::ALL.len() as i32;
        let i = (self.view.index() as i32 + delta).rem_euclid(n) as usize;
        self.set_view(View::ALL[i]);
    }

    fn move_cursor(&mut self, delta: i32, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        ((self.cursor as i32 + delta).rem_euclid(len as i32)) as usize
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if let Some(k) = self.palette_keys.iter().find(|k| k.binding.matches(&key)) {
            match k.view {
                Some(v) if v != self.view => self.set_view(v),
                _ => self.quit = true,
            }
            return;
        }
        if self.flow.is_some() {
            self.handle_flow_key(key);
            return;
        }
        match (self.mode, key.code) {
            (_, KeyCode::Tab) => self.cycle_view(1),
            (_, KeyCode::BackTab) => self.cycle_view(-1),
            (_, KeyCode::Enter) => self.primary(),
            (_, KeyCode::Down) => self.cursor = self.move_cursor(1, self.filtered.len()),
            (_, KeyCode::Up) => self.cursor = self.move_cursor(-1, self.filtered.len()),
            (_, KeyCode::Char('n' | 'j')) if ctrl => self.cursor = self.move_cursor(1, self.filtered.len()),
            (_, KeyCode::Char('p' | 'k')) if ctrl => self.cursor = self.move_cursor(-1, self.filtered.len()),
            (_, KeyCode::Char('o')) if ctrl => self.open_menu(),
            (_, KeyCode::Char(c @ '1'..='4')) if key.modifiers.contains(KeyModifiers::SUPER) => {
                self.set_view(View::ALL[c as usize - '1' as usize]);
            }
            (_, KeyCode::Char('u')) if ctrl => {
                self.query.clear();
                self.cursor = 0;
                self.apply_filter();
            }
            (Mode::Insert, KeyCode::Esc) => self.mode = Mode::Normal,
            (Mode::Insert, KeyCode::Backspace) => {
                self.query.pop();
                self.cursor = 0;
                self.apply_filter();
            }
            (Mode::Insert, KeyCode::Char('w')) if ctrl => {
                let trimmed = self.query.trim_end().to_string();
                self.query = match trimmed.rfind(' ') {
                    Some(i) => trimmed[..=i].to_string(),
                    None => String::new(),
                };
                self.apply_filter();
            }
            (Mode::Insert, KeyCode::Char(c)) if !ctrl => {
                self.query.push(c);
                self.cursor = 0;
                self.apply_filter();
            }
            (Mode::Normal, KeyCode::Char(c)) if !ctrl => self.normal_key(c),
            _ => {}
        }
    }

    fn normal_key(&mut self, c: char) {
        match c {
            '1' => self.set_view(View::All),
            '2' => self.set_view(View::Agents),
            '3' => self.set_view(View::Workspaces),
            '4' => self.set_view(View::Commands),
            'A' => self.set_view(View::All),
            'a' => self.set_view(View::Agents),
            'w' => self.set_view(View::Workspaces),
            'c' => self.set_view(View::Commands),
            'j' => self.cursor = self.move_cursor(1, self.filtered.len()),
            'k' => self.cursor = self.move_cursor(-1, self.filtered.len()),
            'g' => self.cursor = 0,
            'G' => self.cursor = self.filtered.len().saturating_sub(1),
            '/' | 'i' => self.mode = Mode::Insert,
            'q' => self.quit = true,
            'p' => {
                self.preview = !self.preview;
                state::save(&state::State { preview: self.preview });
            }
            'N' => self.start_builtin(Builtin::NewWorkspace, None, None),
            'r' => match self.selected().map(|i| (i.kind.clone(), i.item_ref.clone(), i.machine.clone())) {
                Some((Kind::Workspace, r, m)) => self.start_builtin(Builtin::RenameWorkspace, r, m),
                Some((Kind::Tab, r, m)) => self.start_builtin(Builtin::RenameTab, r, m),
                Some((Kind::Pane, r, m)) => self.start_builtin(Builtin::RenameTab, r, m),
                Some((Kind::Agent, r, m)) => self.start_builtin(Builtin::RenameAgent, r, m),
                _ => {}
            },
            'x' => match self.selected().map(|i| (i.kind.clone(), i.item_ref.clone(), i.machine.clone())) {
                Some((Kind::Workspace, r, m)) => self.start_builtin(Builtin::CloseWorkspace, r, m),
                Some((Kind::Tab, r, m)) => self.start_builtin(Builtin::CloseTab, r, m),
                Some((Kind::Pane, r, m)) | Some((Kind::Agent, r, m)) => self.start_builtin(Builtin::ClosePane, r, m),
                _ => {}
            },
            'e' => {
                if let Some(Kind::Command(i)) = self.selected().map(|i| i.kind.clone()) {
                    if let Class::User { file, .. } = &self.commands[i].class {
                        let mut cmd = builtins::item_action(Builtin::EditCommandFiles, "Edit command file");
                        cmd.inputs.clear();
                        let draft = Draft { command: cmd, item: None, machine: None, answers: HashMap::from([("file".to_string(), file.display().to_string())]) };
                        self.finish(draft);
                    }
                }
            }
            _ => {}
        }
    }

    fn primary(&mut self) {
        let Some(item) = self.selected().cloned() else { return };
        self.usage.record(&item.usage_key());
        match item.kind {
            Kind::Command(i) => {
                let cmd = self.commands[i].clone();
                self.start_command(cmd, None, None);
            }
            Kind::Workspace => self.start_builtin(Builtin::FocusWorkspace, item.item_ref, item.machine),
            Kind::Tab => self.start_builtin(Builtin::FocusTab, item.item_ref, item.machine),
            Kind::Pane => self.start_builtin(Builtin::FocusPane, item.item_ref, item.machine),
            Kind::Agent => self.start_builtin(Builtin::FocusAgent, item.item_ref, item.machine),
        }
    }

    fn start_builtin(&mut self, b: Builtin, item: Option<ItemRef>, machine: Option<String>) {
        let title = match b {
            Builtin::FocusWorkspace | Builtin::FocusTab | Builtin::FocusPane | Builtin::FocusAgent => "Focus",
            Builtin::NewWorkspace => "New workspace",
            Builtin::NewTab => "New tab here",
            Builtin::StartAgent => "Start agent here",
            Builtin::RenameWorkspace => "Rename workspace",
            Builtin::RenameTab => "Rename tab",
            Builtin::RenameAgent => "Rename agent",
            Builtin::CloseWorkspace => "Close workspace",
            Builtin::CloseTab => "Close tab",
            Builtin::ClosePane => "Close pane",
            _ => "Run",
        };
        let cmd = builtins::item_action(b, title);
        self.start_command(cmd, item, machine);
    }

    pub fn actions_for(&self, item: &Item) -> Vec<(Command, Option<ItemRef>)> {
        let mut v = self.builtin_actions_for(item);
        if let Some(kind) = item_kind(&item.kind) {
            v.extend(self.commands.iter().filter(|c| c.on.contains(&kind)).map(|c| (c.clone(), item.item_ref.clone())));
        }
        v
    }

    fn builtin_actions_for(&self, item: &Item) -> Vec<(Command, Option<ItemRef>)> {
        use Builtin::*;
        let r = item.item_ref.clone();
        let mk = |b: Builtin, t: &str| (builtins::item_action(b, t), r.clone());
        match item.kind {
            Kind::Workspace => vec![mk(FocusWorkspace, "Focus"), mk(RenameWorkspace, "Rename"), mk(NewTab, "New tab here"), mk(StartAgent, "Start agent in new tab"), mk(CloseWorkspace, "Close workspace")],
            Kind::Tab => vec![mk(FocusTab, "Focus"), mk(RenameTab, "Rename"), mk(StartAgent, "Start agent in split"), mk(CloseTab, "Close tab")],
            Kind::Pane => {
                let free = matches!(r, Some(ItemRef::Pane { has_agent: false, .. }));
                let mut v = vec![mk(FocusPane, "Focus"), mk(RenameTab, "Rename tab")];
                v.push(mk(StartAgent, if free { "Start agent in this pane" } else { "Start agent in split" }));
                v.push(mk(ClosePane, "Close pane"));
                v
            }
            Kind::Agent => vec![mk(FocusAgent, "Focus"), mk(RenameAgent, "Rename"), mk(StartAgent, "Start agent in split"), mk(ClosePane, "Close pane")],
            Kind::Command(i) => {
                let c = &self.commands[i];
                let mut v = vec![(c.clone(), None)];
                if let Class::User { file, .. } = &c.class {
                    let mut e = builtins::item_action(EditCommandFiles, "Edit command file");
                    e.inputs.clear();
                    e.cwd = Some(file.display().to_string());
                    v.push((e, None));
                }
                v
            }
        }
    }

    fn open_menu(&mut self) {
        let Some(item) = self.selected().cloned() else { return };
        let actions = self.actions_for(&item);
        self.flow = Some(Flow::Menu { actions, machine: item.machine.clone(), cursor: 0, query: String::new() });
    }

    fn start_command(&mut self, command: Command, item: Option<ItemRef>, machine: Option<String>) {
        let item = match item {
            None if !command.on.is_empty() => match self.implicit_item(&command.on) {
                Some(i) => Some(i),
                None => {
                    let kinds: Vec<&str> = command.on.iter().map(|k| k.label()).collect();
                    self.status = Some(format!("error: no {} in the current context", kinds.join(" or ")));
                    return;
                }
            },
            other => other,
        };
        let draft = Draft { command, item, machine, answers: HashMap::new() };
        self.advance(draft, 0);
    }

    /// The Item an `on` Command acts on when run from the list: the captured pane's agent (or the
    /// tab's only agent), else the captured pane, tab or workspace, in the order `on` lists them.
    fn implicit_item(&self, on: &[ItemKind]) -> Option<ItemRef> {
        let s = &self.snapshot;
        let ctx = &self.ctx;
        let pane = ctx.pane_id.as_deref().and_then(|p| s.pane(p));
        on.iter().find_map(|k| match k {
            ItemKind::Agent => {
                let a = pane.and_then(|p| s.agent_in_pane(&p.pane_id)).or_else(|| {
                    let tab = pane.map(|p| p.tab_id.as_str())?;
                    let mut in_tab = s.agents.iter().filter(|a| a.tab_id == tab);
                    let one = in_tab.next()?;
                    in_tab.next().is_none().then_some(one)
                })?;
                Some(ItemRef::Agent { pane_id: a.pane_id.clone(), target: a.target().to_string(), workspace_id: a.workspace_id.clone() })
            }
            ItemKind::Pane => pane.map(|p| ItemRef::Pane { pane_id: p.pane_id.clone(), tab_id: p.tab_id.clone(), workspace_id: p.workspace_id.clone(), has_agent: s.agent_in_pane(&p.pane_id).is_some() }),
            ItemKind::Tab => pane.map(|p| ItemRef::Tab { tab_id: p.tab_id.clone(), workspace_id: p.workspace_id.clone() }),
            ItemKind::Workspace => ctx.workspace_id.clone().map(ItemRef::Workspace),
        })
    }

    fn advance(&mut self, draft: Draft, step: usize) {
        if let Some(input) = draft.command.inputs.get(step).cloned() {
            let options = self.options_for(&input);
            let text = input.default.clone().unwrap_or_default();
            self.flow = Some(Flow::Input { draft, step, text, options, query: String::new(), cursor: 0 });
            return;
        }
        if draft.command.confirm {
            self.flow = Some(Flow::Confirm { draft });
            return;
        }
        self.finish(draft);
    }

    fn finish(&mut self, draft: Draft) {
        self.flow = None;
        let mut answers = draft.answers;
        if let Class::Builtin(Builtin::EditCommandFiles) = draft.command.class {
            if let Some(f) = &draft.command.cwd {
                answers.entry("file".into()).or_insert(f.clone());
            }
        }
        let mut command = draft.command;
        if let Class::Builtin(Builtin::EditCommandFiles) = command.class {
            command.cwd = None;
        }
        let machine = draft.machine.as_deref();
        let mut item_vars = draft.item.as_ref().map(|i| run::item_vars(self.snapshot_of(machine), i)).unwrap_or_default();
        let socket = machine.map(|m| self.client_of(Some(m)).path().to_path_buf());
        if let (Some(m), Some(s)) = (machine, &socket) {
            item_vars.push(("item.machine".into(), m.to_string()));
            item_vars.push(("item.socket".into(), s.display().to_string()));
        }
        let plan = Plan { command, answers, ctx: self.ctx.clone(), item: draft.item, item_vars, palette_pane: self.palette_pane.clone(), machine: draft.machine.clone(), socket };
        // Focusing on another Machine cannot bring it on screen, so the Palette stays up to say so.
        let remote_focus = plan.machine.is_some() && matches!(plan.command.class, Class::Builtin(Builtin::FocusWorkspace | Builtin::FocusTab | Builtin::FocusPane | Builtin::FocusAgent));
        if plan.command.keep_open || remote_focus {
            let rt = Runtime { client: self.client_of(machine), cfg: &self.cfg, plugin_id: self.plugin_id.clone() };
            match run::execute(&plan, &rt) {
                Ok(msg) => self.status = Some(msg),
                Err(e) => self.status = Some(format!("error: {e:#}")),
            }
            self.refresh();
            return;
        }
        match run::defer(&plan) {
            Ok(()) => self.quit = true,
            Err(e) => self.status = Some(format!("error: {e:#}")),
        }
    }

    fn options_for(&self, input: &Input) -> Vec<(String, String)> {
        let s = &self.snapshot;
        match input.kind {
            InputKind::Text => vec![],
            InputKind::Choice if input.name == "file" && input.choices.is_empty() && input.choices_command.is_none() => {
                let mut v: Vec<(String, String)> = self
                    .command_files
                    .iter()
                    .map(|f| (f.display().to_string(), f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
                    .collect();
                if v.is_empty() {
                    let d = config::commands_dir();
                    v.push((d.display().to_string(), "commands/ (empty)".into()));
                }
                v
            }
            InputKind::Choice => {
                let mut v: Vec<(String, String)> = input.choices.iter().map(|c| (c.clone(), c.clone())).collect();
                if let Some(cmd) = &input.choices_command {
                    if let Ok(out) = std::process::Command::new("sh").arg("-c").arg(cmd).current_dir(&self.ctx.dir).output() {
                        v.extend(String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| (l.to_string(), l.to_string())));
                    }
                }
                v
            }
            InputKind::Ref => crate::git::base_refs(&self.ctx.dir).into_iter().map(|r| (r.clone(), r)).collect(),
            InputKind::Workspace => s.workspaces.iter().map(|w| (w.workspace_id.clone(), w.label.clone())).collect(),
            InputKind::Tab => s.tabs.iter().map(|t| (t.tab_id.clone(), format!("{} › {}", s.workspace_label(&t.workspace_id), t.label))).collect(),
            InputKind::Pane => s
                .panes
                .iter()
                .map(|p| {
                    let who = s.agent_in_pane(&p.pane_id).map(|a| a.display_name()).unwrap_or_else(|| "shell".into());
                    let tab = s.tab(&p.tab_id).map(|t| t.label.clone()).unwrap_or_default();
                    (p.pane_id.clone(), format!("{} › {tab} › {who}", s.workspace_label(&p.workspace_id)))
                })
                .collect(),
            InputKind::Agent => s
                .agents
                .iter()
                .map(|a| (a.target().to_string(), format!("{} ({})  ·  {}", a.display_name(), a.status_text(), s.workspace_label(&a.workspace_id))))
                .collect(),
        }
    }

    fn handle_flow_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(flow) = self.flow.take() else { return };
        match flow {
            Flow::Menu { actions, machine, mut cursor, mut query } => {
                let hays: Vec<Vec<String>> = actions.iter().map(|(c, _)| vec![c.title.clone()]).collect();
                let filtered = self.search.rank(&query, hays.iter().map(|v| v.as_slice()));
                match key.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter => {
                        if let Some(&i) = filtered.get(cursor) {
                            let (cmd, item) = actions[i].clone();
                            self.start_command(cmd, item, machine);
                        }
                        return;
                    }
                    KeyCode::Down => cursor = (cursor + 1) % filtered.len().max(1),
                    KeyCode::Up => cursor = (cursor + filtered.len().max(1) - 1) % filtered.len().max(1),
                    KeyCode::Char('n' | 'j') if ctrl => cursor = (cursor + 1) % filtered.len().max(1),
                    KeyCode::Char('p' | 'k') if ctrl => cursor = (cursor + filtered.len().max(1) - 1) % filtered.len().max(1),
                    KeyCode::Backspace => {
                        query.pop();
                        cursor = 0;
                    }
                    KeyCode::Char(c) if !ctrl => {
                        query.push(c);
                        cursor = 0;
                    }
                    _ => {}
                }
                self.flow = Some(Flow::Menu { actions, machine, cursor, query });
            }
            Flow::Input { mut draft, step, mut text, options, mut query, mut cursor } => {
                let input = draft.command.inputs[step].clone();
                let is_text = matches!(input.kind, InputKind::Text);
                let hays: Vec<Vec<String>> = options.iter().map(|(v, l)| vec![l.clone(), v.clone()]).collect();
                let filtered = self.search.rank(&query, hays.iter().map(|v| v.as_slice()));
                match key.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter => {
                        let value = if is_text {
                            text.clone()
                        } else if let Some(&i) = filtered.get(cursor) {
                            options[i].0.clone()
                        } else if !query.is_empty() && matches!(input.kind, InputKind::Choice | InputKind::Ref) {
                            query.clone()
                        } else {
                            self.flow = Some(Flow::Input { draft, step, text, options, query, cursor });
                            return;
                        };
                        if !is_text {
                            if let Some(&i) = filtered.get(cursor) {
                                draft.answers.insert(format!("{}_label", input.name), options[i].1.clone());
                            }
                        }
                        draft.answers.insert(input.name.clone(), value);
                        self.advance(draft, step + 1);
                        return;
                    }
                    KeyCode::Backspace => {
                        if is_text { text.pop(); } else { query.pop(); cursor = 0; }
                    }
                    KeyCode::Char('u') if ctrl => {
                        if is_text { text.clear(); } else { query.clear(); cursor = 0; }
                    }
                    KeyCode::Char('g') if ctrl && is_text => self.edit_in_editor = true,
                    KeyCode::Down => cursor = (cursor + 1) % filtered.len().max(1),
                    KeyCode::Up => cursor = (cursor + filtered.len().max(1) - 1) % filtered.len().max(1),
                    KeyCode::Char('n' | 'j') if ctrl => cursor = (cursor + 1) % filtered.len().max(1),
                    KeyCode::Char('p' | 'k') if ctrl => cursor = (cursor + filtered.len().max(1) - 1) % filtered.len().max(1),
                    KeyCode::Char(c) if !ctrl => {
                        if is_text { text.push(c); } else { query.push(c); cursor = 0; }
                    }
                    _ => {}
                }
                self.flow = Some(Flow::Input { draft, step, text, options, query, cursor });
            }
            Flow::Confirm { draft } => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.finish(draft),
                _ => {}
            },
        }
    }

    pub fn input_text(&self) -> Option<&str> {
        match &self.flow {
            Some(Flow::Input { draft, step, text, .. }) if matches!(draft.command.inputs[*step].kind, InputKind::Text) => Some(text),
            _ => None,
        }
    }

    pub fn set_input_text(&mut self, value: String) {
        if let Some(Flow::Input { text, .. }) = &mut self.flow {
            *text = value;
        }
    }

    fn update_preview(&mut self) {
        let Some(item) = self.selected() else { return };
        let pane_id = match &item.item_ref {
            Some(ItemRef::Agent { pane_id, .. }) => pane_id.clone(),
            Some(ItemRef::Pane { pane_id, .. }) => pane_id.clone(),
            _ => return,
        };
        let key = item.id.clone();
        let machine = item.machine.clone();
        let stale = match &self.preview_cache {
            Some((id, _, at)) => *id != key || at.elapsed() > Duration::from_millis(1000),
            None => true,
        };
        if stale {
            let text = self.client_of(machine.as_deref()).read_pane(&pane_id, self.cfg.preview_lines).unwrap_or_default();
            self.preview_cache = Some((key, text, Instant::now()));
        }
    }

    /// Styled lines for the preview panel of the selected Item.
    pub fn preview_lines(&self, width: usize) -> Vec<Vec<Seg>> {
        let Some(item) = self.selected() else { return vec![] };
        let s = self.snapshot_of(item.machine.as_deref());
        let path = |c: &str| seg(crate::context::shorten_home(std::path::Path::new(c)), Tone::Path);
        let machine_line = || item.machine.as_ref().map(|m| field("machine", seg(m, Tone::Machine)));
        match (&item.kind, &item.item_ref) {
            (Kind::Agent, Some(ItemRef::Agent { pane_id, .. })) | (Kind::Pane, Some(ItemRef::Pane { pane_id, .. })) => {
                let mut lines = vec![];
                if let Some(a) = s.agent_in_pane(pane_id) {
                    let st = Tone::Status(a.agent_status);
                    lines.push(vec![seg(a.agent_status.glyph(), st), seg(" ", Tone::Plain), seg(a.display_name(), Tone::Title), seg("  ", Tone::Plain), seg(a.status_text(), st)]);
                    lines.extend(machine_line());
                    lines.push(field("workspace", seg(s.workspace_label(&a.workspace_id), Tone::Workspace)));
                    if let Some(c) = a.foreground_cwd.as_ref().or(a.cwd.as_ref()) {
                        lines.push(field("cwd", path(c)));
                    }
                    if let Some(t) = &a.terminal_title_stripped {
                        lines.push(field("title", seg(t, Tone::Plain)));
                    }
                } else if let Some(p) = s.pane(pane_id) {
                    lines.push(vec![seg("pane ", Tone::Dim), seg(&p.pane_id, Tone::Title)]);
                    lines.extend(machine_line());
                    if let Some(c) = p.foreground_cwd.as_ref().or(p.cwd.as_ref()) {
                        lines.push(field("cwd", path(c)));
                    }
                }
                if let Some((id, text, _)) = &self.preview_cache {
                    if *id == item.id {
                        lines.push(rule(width));
                        lines.extend(text.lines().map(|l| vec![seg(l.trim_end(), Tone::Plain)]));
                    }
                }
                lines
            }
            (Kind::Workspace, Some(ItemRef::Workspace(id))) => {
                let mut lines = vec![];
                if let Some(w) = s.workspace(id) {
                    lines.push(vec![seg(w.agent_status.glyph(), Tone::Status(w.agent_status)), seg(" ", Tone::Plain), seg(&w.label, Tone::Title)]);
                    lines.extend(machine_line());
                    if let Some(wt) = &w.worktree {
                        lines.push(field("worktree", path(&wt.checkout_path)));
                        lines.push(field("repo", path(&wt.repo_root)));
                    }
                    lines.push(vec![]);
                    for t in s.tabs.iter().filter(|t| t.workspace_id == *id) {
                        lines.push(vec![
                            seg(t.agent_status.glyph(), Tone::Status(t.agent_status)),
                            seg(" ", Tone::Plain),
                            seg(&t.label, Tone::Plain),
                            seg(format!("  {} panes", t.pane_count), Tone::Dim),
                        ]);
                    }
                }
                lines
            }
            (Kind::Tab, Some(ItemRef::Tab { tab_id, .. })) => s
                .panes
                .iter()
                .filter(|p| p.tab_id == *tab_id)
                .map(|p| {
                    let who = s.agent_in_pane(&p.pane_id).map(|a| seg(a.display_name(), Tone::Agent)).unwrap_or_else(|| seg("shell", Tone::Dim));
                    vec![seg(p.agent_status.glyph(), Tone::Status(p.agent_status)), seg(" ", Tone::Plain), seg(&p.pane_id, Tone::Dim), seg("  ", Tone::Plain), who]
                })
                .collect(),
            (Kind::Command(i), _) => {
                let c = &self.commands[*i];
                let mut lines = vec![vec![seg(&c.title, Tone::Title)]];
                if !c.description.is_empty() {
                    lines.extend(crate::markdown::render(&c.description, width));
                }
                lines.push(vec![]);
                lines.push(field("id", seg(&c.id, Tone::Plain)));
                lines.push(field("class", seg(c.class.label(), Tone::Plain)));
                lines.push(field("source", seg(&c.source, Tone::Plain)));
                match &c.class {
                    Class::User { .. } => {
                        lines.push(field("in", seg(c.target.label(), Tone::Plain)));
                        if let Some(cwd) = &c.cwd {
                            lines.push(field("cwd", seg(cwd, Tone::Path)));
                        }
                        lines.push(vec![]);
                        lines.push(vec![seg("$ ", Tone::Dim), seg(&c.run, Tone::Code)]);
                    }
                    Class::PluginAction { plugin_id, action_id } => {
                        lines.push(vec![seg("invokes ", Tone::Dim), seg(format!("{plugin_id}.{action_id}"), Tone::Code), seg(" with the captured context", Tone::Dim)])
                    }
                    Class::Builtin(_) => {}
                }
                if !c.inputs.is_empty() {
                    lines.push(vec![]);
                    lines.push(vec![seg("inputs", Tone::Heading)]);
                    for i in &c.inputs {
                        lines.push(vec![seg("  ", Tone::Plain), seg(&i.name, Tone::Strong), seg(format!(" {:?}", i.kind).to_lowercase(), Tone::Dim), seg("  ", Tone::Plain), seg(&i.prompt, Tone::Plain)]);
                    }
                }
                lines
            }
            _ => vec![],
        }
    }
}

fn field(name: &str, value: Seg) -> Vec<Seg> {
    vec![seg(format!("{name:<10}"), Tone::Dim), value]
}

fn rule(width: usize) -> Vec<Seg> {
    vec![seg("─".repeat(width.max(1)), Tone::Rule)]
}
