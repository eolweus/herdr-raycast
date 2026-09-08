use crate::herdr::{Client, Snapshot};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const RESCAN: Duration = Duration::from_secs(5);

/// Another Herdr server whose socket is forwarded into the machines directory.
/// Polled from its own thread so a stalled forward never blocks the UI.
pub struct Machine {
    pub name: String,
    pub client: Client,
    pub snapshot: Snapshot,
    pub ok: bool,
    pub error: Option<String>,
    rx: Receiver<Result<Snapshot, String>>,
}

impl Machine {
    fn spawn(name: String, path: PathBuf, every: Duration) -> Self {
        let client = Client::at(path);
        let (tx, rx) = mpsc::channel();
        let poller = client.clone();
        std::thread::spawn(move || loop {
            let r = poller.snapshot().map_err(|e| format!("{e:#}"));
            if tx.send(r).is_err() {
                break;
            }
            std::thread::sleep(every);
        });
        Self { name, client, snapshot: Snapshot::default(), ok: false, error: None, rx }
    }

    /// Takes every poll result that arrived; true when the visible state moved.
    fn drain(&mut self) -> bool {
        let mut changed = false;
        while let Ok(r) = self.rx.try_recv() {
            match r {
                Ok(s) => {
                    self.snapshot = s;
                    self.ok = true;
                    self.error = None;
                    changed = true;
                }
                Err(e) => {
                    changed |= self.ok;
                    self.ok = false;
                    self.error = Some(e);
                    self.snapshot = Snapshot::default();
                }
            }
        }
        changed
    }
}

pub struct Machines {
    dir: PathBuf,
    local: Option<PathBuf>,
    every: Duration,
    pub list: Vec<Machine>,
    last_scan: Instant,
}

impl Machines {
    pub fn new(dir: PathBuf, local_socket: &Path, every: Duration) -> Self {
        let mut m = Self { dir, local: local_socket.canonicalize().ok(), every, list: vec![], last_scan: Instant::now() };
        m.scan();
        m
    }

    /// `<dir>/<name>.sock` names a Machine; the entry for this server's own socket is skipped.
    fn scan(&mut self) {
        self.last_scan = Instant::now();
        let mut found: Vec<(String, PathBuf)> = vec![];
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let path = e.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".sock")) else { continue };
                let is_sock = std::fs::metadata(&path).map(|m| std::os::unix::fs::FileTypeExt::is_socket(&m.file_type())).unwrap_or(false);
                if !is_sock || name.is_empty() {
                    continue;
                }
                if self.local.is_some() && path.canonicalize().ok() == self.local {
                    continue;
                }
                found.push((name.to_string(), path));
            }
        }
        found.sort();
        self.list.retain(|m| found.iter().any(|(n, _)| *n == m.name));
        for (name, path) in found {
            if !self.list.iter().any(|m| m.name == name) {
                self.list.push(Machine::spawn(name, path, self.every));
            }
        }
        self.list.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Rescans now and then and drains poll results; true when any Machine changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        if self.last_scan.elapsed() >= RESCAN {
            let before: Vec<String> = self.list.iter().map(|m| m.name.clone()).collect();
            self.scan();
            changed = before != self.list.iter().map(|m| m.name.clone()).collect::<Vec<_>>();
        }
        for m in &mut self.list {
            changed |= m.drain();
        }
        changed
    }

    pub fn get(&self, name: &str) -> Option<&Machine> {
        self.list.iter().find(|m| m.name == name)
    }

    pub fn connected(&self) -> impl Iterator<Item = &Machine> {
        self.list.iter().filter(|m| m.ok)
    }
}
