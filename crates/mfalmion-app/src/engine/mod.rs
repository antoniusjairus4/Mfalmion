//! Asynchronous native UCI analysis. No GUI or filesystem persistence dependency.
mod process;
pub mod protocol;
#[cfg(test)]
mod tests;
mod worker;

use shakmaty::Chess;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
pub use worker::EngineClient;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub path: String,
    pub depth: u16,
    pub multipv: usize,
    pub threads: u32,
    pub hash_mib: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            path: "/usr/games/stockfish".into(),
            depth: 16,
            multipv: 3,
            threads: 2,
            hash_mib: 64,
        }
    }
}
impl Settings {
    pub fn max_threads() -> u32 {
        std::thread::available_parallelism().map_or(2, |n| n.get().min(8) as u32)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.path.trim().is_empty()
            || self.path.len() > 4096
            || self.path.contains(['\0', '\n', '\r'])
        {
            return Err("Specify a valid engine executable path".into());
        }
        if !(1..=32).contains(&self.depth) {
            return Err("Depth must be 1–32".into());
        }
        if !(1..=3).contains(&self.multipv) {
            return Err("MultiPV must be 1–3".into());
        }
        if self.threads == 0 || self.threads > Self::max_threads() {
            return Err(format!("Threads must be 1–{}", Self::max_threads()));
        }
        if !(16..=512).contains(&self.hash_mib) {
            return Err("Hash must be 16–512 MiB".into());
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Request {
    pub id: u64,
    pub session: u64,
    pub position: Chess,
    pub settings: Settings,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Offline,
    Starting,
    Ready,
    Analyzing,
    Stopping,
    Error,
}
#[derive(Debug)]
pub enum Event {
    Status(u64, Status),
    Identity(u64, String),
    Snapshot(u64, protocol::Snapshot),
    BestMove(u64, Option<String>),
    Error(u64, String),
}
impl Event {
    pub fn id(&self) -> u64 {
        match self {
            Self::Status(id, _)
            | Self::Identity(id, _)
            | Self::Snapshot(id, _)
            | Self::BestMove(id, _)
            | Self::Error(id, _) => *id,
        }
    }
}
pub enum Work {
    Analyze(Request),
    Stop(u64),
}
pub(super) struct Mailbox {
    tx: mpsc::SyncSender<Work>,
    rx: Mutex<Receiver<Work>>,
    shutdown: AtomicBool,
    stopped: AtomicBool,
}
impl Default for Mailbox {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        Self {
            tx,
            rx: Mutex::new(rx),
            shutdown: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        }
    }
}
impl Mailbox {
    fn put(&self, work: Work) {
        let rx = self.rx.lock().unwrap_or_else(|p| p.into_inner());
        let id = |work: &Work| match work {
            Work::Analyze(request) => request.id,
            Work::Stop(id) => *id,
        };
        let work = match rx.try_recv() {
            Ok(queued) if id(&queued) > id(&work) => queued,
            _ => work,
        };
        self.stopped
            .store(matches!(work, Work::Stop(_)), Ordering::Release);
        let _ = self.tx.try_send(work);
    }
    fn take(&self) -> Option<Work> {
        self.rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_recv()
            .ok()
    }
    fn interrupted(&self) -> bool {
        self.shutdown.load(Ordering::Acquire) || self.stopped.load(Ordering::Acquire)
    }
}
pub(super) struct Results {
    tx: mpsc::SyncSender<Event>,
    rx: Mutex<Receiver<Event>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Default for Results {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel(5);
        Self {
            tx,
            rx: Mutex::new(rx),
            wake: None,
        }
    }
}
impl Results {
    fn drain(&self) -> Vec<Event> {
        self.rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .try_iter()
            .collect()
    }
    fn send(&self, event: Event) {
        // Serialize replacement with the UI receiver. At most one event of each kind
        // is retained; a hidden panel cannot create an unbounded result backlog.
        let rx = self.rx.lock().unwrap_or_else(|p| p.into_inner());
        let kind = std::mem::discriminant(&event);
        let mut pending: Vec<_> = rx
            .try_iter()
            .filter(|old| std::mem::discriminant(old) != kind)
            .collect();
        pending.push(event);
        for event in pending {
            let _ = self.tx.try_send(event);
        }
        drop(rx);
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}
// Completion channel remains separate so normal GUI close waits asynchronously for child reaping.
pub(super) fn completion() -> (mpsc::Sender<()>, Receiver<()>) {
    mpsc::channel()
}
