use crate::commands::builtins::{self, Builtin};
use crate::commands::run::{self, ItemRef, Plan, Runtime};
use crate::commands::{resolve, Class, Command, Input, InputKind};
use crate::config::{self, Config};
use crate::context::Captured;
use crate::herdr::{AgentStatus, Client, Snapshot};
use crate::keys::{self, PaletteKey};
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
    Dim,
    Path,
    Agent,
    Workspace,
    Status(AgentStatus),
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

fn seg(text: impl Into<String>, tone: Tone) -> Seg {
    Seg(text.into(), tone)
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
    pub answers: HashMap<String, String>,
}

pub enum Flow {
    Menu { actions: Vec<(Command, Option<ItemRef>)>, cursor: usize, query: String },
    Input { draft: Draft, step: usize, text: String, options: Vec<(String, String)>, query: String, cursor: usize },
    Confirm { draft: Draft },
}

const TOP_MATCHES: usize = 3;

pub struct App {
    pub client: Client,
    pub cfg: Config,
    pub ctx: Captured,
    pub snapshot: Snapshot,
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
        let mut app = Self {
            client,
            cfg,
            ctx,
            snapshot,
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
        let ranked = self.search.rank(&self.query, hays.into_iter());
        self.context.clear();
        self.top = 0;
        if self.query.trim().is_empty() {
            self.filtered = ranked;
        } else if self.view == View::Workspaces {
            // Keep tree order; ancestors of a match stay as dimmed context.
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
            self.filtered = (0..self.items.len()).filter(|&i| keep[i]).collect();
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

    fn agent_items(&self) -> Vec<Item> {
        let mut agents: Vec<&crate::herdr::Agent> = self.snapshot.agents.iter().collect();
        agents.sort_by_key(|a| a.agent_status.priority());
        agents
            .into_iter()
            .map(|a| {
                let ws = self.snapshot.workspace_label(&a.workspace_id);
                let title = a.terminal_title_stripped.clone().or_else(|| a.title.clone()).unwrap_or_default();
                let kind = a.display_agent.clone().or_else(|| a.agent.clone()).unwrap_or_default();
                let mut meta = vec![];
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
                    id: format!("agent:{}", a.pane_id),
                    title: row_title,
                    meta,
                    status: Some(a.agent_status),
                    depth: 0,
                    current: self.ctx.pane_id.as_deref() == Some(a.pane_id.as_str()),
                    hay: vec![a.display_name(), title, ws, kind],
                    item_ref: Some(ItemRef::Agent { pane_id: a.pane_id.clone(), target: a.target().to_string(), workspace_id: a.workspace_id.clone() }),
                }
            })
            .collect()
    }

    fn workspace_dir(&self, w: &crate::herdr::Workspace) -> String {
        w.worktree
            .as_ref()
            .map(|wt| wt.checkout_path.clone())
            .or_else(|| w.cwd.clone())
            .or_else(|| self.snapshot.panes.iter().find(|p| p.workspace_id == w.workspace_id).and_then(|p| p.cwd.clone()))
            .unwrap_or_default()
    }

    fn workspace_row(&self, w: &crate::herdr::Workspace) -> Item {
        let dir = self.workspace_dir(w);
        let panes: Vec<&crate::herdr::Pane> = self.snapshot.panes.iter().filter(|p| p.workspace_id == w.workspace_id).collect();
        let mut meta = activity(panes.iter().filter(|p| self.snapshot.agent_in_pane(&p.pane_id).is_some()).map(|p| p.agent_status));
        if !dir.is_empty() {
            meta.push(seg(crate::context::short_path(&dir), Tone::Path));
        }
        Item {
            kind: Kind::Workspace,
            id: format!("ws:{}", w.workspace_id),
            title: format!("{} ({})", w.label, panes.len()),
            meta,
            status: None,
            depth: 0,
            current: self.ctx.workspace_id.as_deref() == Some(w.workspace_id.as_str()),
            hay: vec![w.label.clone(), dir.rsplit('/').next().unwrap_or("").to_string()],
            item_ref: Some(ItemRef::Workspace(w.workspace_id.clone())),
        }
    }

    fn workspace_items(&self) -> Vec<Item> {
        let mut out = vec![];
        for w in &self.snapshot.workspaces {
            out.push(self.workspace_row(w));
            let tabs: Vec<&crate::herdr::Tab> = self.snapshot.tabs.iter().filter(|t| t.workspace_id == w.workspace_id).collect();
            let show_tabs = tabs.len() > 1;
            for t in tabs {
                let panes: Vec<&crate::herdr::Pane> = self.snapshot.panes.iter().filter(|p| p.tab_id == t.tab_id).collect();
                if show_tabs {
                    let mut meta = vec![seg(format!("{} panes", panes.len()), Tone::Dim)];
                    meta.extend(activity(panes.iter().filter(|p| self.snapshot.agent_in_pane(&p.pane_id).is_some()).map(|p| p.agent_status)));
                    out.push(Item {
                        kind: Kind::Tab,
                        id: format!("tab:{}", t.tab_id),
                        title: t.label.clone(),
                        meta,
                        status: None,
                        depth: 1,
                        current: false,
                        hay: vec![t.label.clone(), w.label.clone()],
                        item_ref: Some(ItemRef::Tab { tab_id: t.tab_id.clone(), workspace_id: w.workspace_id.clone() }),
                    });
                }
                for p in panes {
                    let agent = self.snapshot.agent_in_pane(&p.pane_id);
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
                        id: format!("pane:{}", p.pane_id),
                        title: name.clone(),
                        meta,
                        status: agent.map(|_| p.agent_status),
                        depth: if show_tabs { 2 } else { 1 },
                        current: self.ctx.pane_id.as_deref() == Some(p.pane_id.as_str()),
                        hay: vec![name, t.label.clone(), w.label.clone(), cwd.rsplit('/').next().unwrap_or("").to_string()],
                        item_ref: Some(ItemRef::Pane { pane_id: p.pane_id.clone(), tab_id: t.tab_id.clone(), workspace_id: w.workspace_id.clone(), has_agent: agent.is_some() }),
                    });
                }
            }
        }
        out
    }

    fn command_items(&self) -> Vec<Item> {
        self.commands
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
            })
            .collect()
    }

    fn all_items(&self) -> Vec<Item> {
        let mut out = self.agent_items();
        out.extend(self.snapshot.workspaces.iter().map(|w| self.workspace_row(w)));
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
            'N' => self.start_builtin(Builtin::NewWorkspace, None),
            'a' => self.start_builtin(Builtin::StartAgent, self.selected().and_then(|i| i.item_ref.clone())),
            'r' => match self.selected().map(|i| (i.kind.clone(), i.item_ref.clone())) {
                Some((Kind::Workspace, r)) => self.start_builtin(Builtin::RenameWorkspace, r),
                Some((Kind::Tab, r)) => self.start_builtin(Builtin::RenameTab, r),
                Some((Kind::Pane, r)) => self.start_builtin(Builtin::RenameTab, r),
                Some((Kind::Agent, r)) => self.start_builtin(Builtin::RenameAgent, r),
                _ => {}
            },
            'x' => match self.selected().map(|i| (i.kind.clone(), i.item_ref.clone())) {
                Some((Kind::Workspace, r)) => self.start_builtin(Builtin::CloseWorkspace, r),
                Some((Kind::Tab, r)) => self.start_builtin(Builtin::CloseTab, r),
                Some((Kind::Pane, r)) | Some((Kind::Agent, r)) => self.start_builtin(Builtin::ClosePane, r),
                _ => {}
            },
            'e' => {
                if let Some(Kind::Command(i)) = self.selected().map(|i| i.kind.clone()) {
                    if let Class::User { file } = &self.commands[i].class {
                        let mut cmd = builtins::item_action(Builtin::EditCommandFiles, "Edit command file");
                        cmd.inputs.clear();
                        let draft = Draft { command: cmd, item: None, answers: HashMap::from([("file".to_string(), file.display().to_string())]) };
                        self.finish(draft);
                    }
                }
            }
            _ => {}
        }
    }

    fn primary(&mut self) {
        let Some(item) = self.selected().cloned() else { return };
        match item.kind {
            Kind::Command(i) => {
                let cmd = self.commands[i].clone();
                self.start_command(cmd, None);
            }
            Kind::Workspace => self.start_builtin(Builtin::FocusWorkspace, item.item_ref),
            Kind::Tab => self.start_builtin(Builtin::FocusTab, item.item_ref),
            Kind::Pane => self.start_builtin(Builtin::FocusPane, item.item_ref),
            Kind::Agent => self.start_builtin(Builtin::FocusAgent, item.item_ref),
        }
    }

    fn start_builtin(&mut self, b: Builtin, item: Option<ItemRef>) {
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
        self.start_command(cmd, item);
    }

    pub fn actions_for(&self, item: &Item) -> Vec<(Command, Option<ItemRef>)> {
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
                if let Class::User { file } = &c.class {
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
        self.flow = Some(Flow::Menu { actions, cursor: 0, query: String::new() });
    }

    fn start_command(&mut self, command: Command, item: Option<ItemRef>) {
        let draft = Draft { command, item, answers: HashMap::new() };
        self.advance(draft, 0);
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
        let plan = Plan { command, answers, ctx: self.ctx.clone(), item: draft.item, palette_pane: self.palette_pane.clone() };
        if plan.command.keep_open {
            let rt = Runtime { client: &self.client, cfg: &self.cfg, plugin_id: self.plugin_id.clone() };
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
            Flow::Menu { actions, mut cursor, mut query } => {
                let hays: Vec<Vec<String>> = actions.iter().map(|(c, _)| vec![c.title.clone()]).collect();
                let filtered = self.search.rank(&query, hays.iter().map(|v| v.as_slice()));
                match key.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter => {
                        if let Some(&i) = filtered.get(cursor) {
                            let (cmd, item) = actions[i].clone();
                            self.start_command(cmd, item);
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
                self.flow = Some(Flow::Menu { actions, cursor, query });
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
                        } else if !query.is_empty() && matches!(input.kind, InputKind::Choice) {
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

    fn update_preview(&mut self) {
        let Some(item) = self.selected() else { return };
        let pane_id = match &item.item_ref {
            Some(ItemRef::Agent { pane_id, .. }) => pane_id.clone(),
            Some(ItemRef::Pane { pane_id, .. }) => pane_id.clone(),
            _ => return,
        };
        let stale = match &self.preview_cache {
            Some((id, _, at)) => *id != pane_id || at.elapsed() > Duration::from_millis(1000),
            None => true,
        };
        if stale {
            let text = self.client.read_pane(&pane_id, self.cfg.preview_lines).unwrap_or_default();
            self.preview_cache = Some((pane_id, text, Instant::now()));
        }
    }

    /// Lines for the preview panel of the selected Item.
    pub fn preview_lines(&self) -> Vec<String> {
        let Some(item) = self.selected() else { return vec![] };
        let s = &self.snapshot;
        match (&item.kind, &item.item_ref) {
            (Kind::Agent, Some(ItemRef::Agent { pane_id, .. })) | (Kind::Pane, Some(ItemRef::Pane { pane_id, .. })) => {
                let mut lines = vec![];
                if let Some(a) = s.agent_in_pane(pane_id) {
                    lines.push(format!("{} {}  ({})", a.agent_status.glyph(), a.display_name(), a.status_text()));
                    lines.push(format!("workspace: {}", s.workspace_label(&a.workspace_id)));
                    if let Some(c) = a.foreground_cwd.as_ref().or(a.cwd.as_ref()) {
                        lines.push(format!("cwd: {}", crate::context::shorten_home(std::path::Path::new(c))));
                    }
                    if let Some(t) = &a.terminal_title_stripped {
                        lines.push(format!("title: {t}"));
                    }
                } else if let Some(p) = s.pane(pane_id) {
                    lines.push(format!("pane {}", p.pane_id));
                    if let Some(c) = p.foreground_cwd.as_ref().or(p.cwd.as_ref()) {
                        lines.push(format!("cwd: {}", crate::context::shorten_home(std::path::Path::new(c))));
                    }
                }
                lines.push(String::new());
                if let Some((id, text, _)) = &self.preview_cache {
                    if id == pane_id {
                        lines.extend(text.lines().map(|l| l.trim_end().to_string()));
                    }
                }
                lines
            }
            (Kind::Workspace, Some(ItemRef::Workspace(id))) => {
                let mut lines = vec![];
                if let Some(w) = s.workspace(id) {
                    lines.push(format!("{} {}", w.agent_status.glyph(), w.label));
                    if let Some(wt) = &w.worktree {
                        lines.push(format!("worktree: {}", crate::context::shorten_home(std::path::Path::new(&wt.checkout_path))));
                        lines.push(format!("repo: {}", crate::context::shorten_home(std::path::Path::new(&wt.repo_root))));
                    }
                    lines.push(String::new());
                    for t in s.tabs.iter().filter(|t| t.workspace_id == *id) {
                        lines.push(format!("{} {}  ({} panes)", t.agent_status.glyph(), t.label, t.pane_count));
                    }
                }
                lines
            }
            (Kind::Tab, Some(ItemRef::Tab { tab_id, .. })) => {
                let mut lines = vec![];
                for p in s.panes.iter().filter(|p| p.tab_id == *tab_id) {
                    let who = s.agent_in_pane(&p.pane_id).map(|a| a.display_name()).unwrap_or_else(|| "shell".into());
                    lines.push(format!("{} {}  {}", p.agent_status.glyph(), p.pane_id, who));
                }
                lines
            }
            (Kind::Command(i), _) => {
                let c = &self.commands[*i];
                let mut lines = vec![c.title.clone(), format!("id: {}", c.id), format!("class: {}", c.class.label()), format!("source: {}", c.source)];
                match &c.class {
                    Class::User { .. } => {
                        lines.push(format!("in: {}", c.target.label()));
                        if let Some(cwd) = &c.cwd {
                            lines.push(format!("cwd: {cwd}"));
                        }
                        lines.push(String::new());
                        lines.push(format!("$ {}", c.run));
                    }
                    Class::PluginAction { plugin_id, action_id } => lines.push(format!("invokes {plugin_id}.{action_id} with the captured context")),
                    Class::Builtin(_) => {}
                }
                if !c.inputs.is_empty() {
                    lines.push(String::new());
                    lines.push("inputs:".into());
                    for i in &c.inputs {
                        lines.push(format!("  {} ({:?})  {}", i.name, i.kind, i.prompt));
                    }
                }
                lines
            }
            _ => vec![],
        }
    }
}
