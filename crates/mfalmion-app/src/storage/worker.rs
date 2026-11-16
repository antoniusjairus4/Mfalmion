//! One connection, bounded queues, explicit UI polling/wake; no per-frame queries.
use super::{
    analysis::{Key, Line},
    games::{GameRow, ImportReport, Query},
    settings::Preferences,
    *,
};
use crate::{engine::protocol::Snapshot, notation::GameRecord};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
pub enum Command {
    Refresh(i64, Query),
    Select(String),
    DeleteProfile(i64, bool),
    Import(i64, String),
    ImportRecords(i64, Vec<GameRecord>),
    Open(i64, i64),
    Delete(i64, i64, bool),
    Preferences(Preferences),
    SyncPreferences(crate::sync::Preferences),
    Integrity,
    Backup(PathBuf),
    Restore(PathBuf, PathBuf, bool),
    Cache(Key, Snapshot),
    Lookup(Key),
}
pub enum Reply {
    State {
        profiles: Vec<(i64, String, bool)>,
        active: i64,
        preferences: Preferences,
        sync_preferences: crate::sync::Preferences,
        warning: Option<String>,
        bytes: u64,
    },
    Games(Vec<GameRow>),
    Opened(i64, i64, Box<GameRecord>),
    Imported(ImportReport),
    Cached(Key, Option<Vec<Line>>),
    Done(String),
    Error(String),
}
pub struct Client {
    sender: SyncSender<Command>,
    pub receiver: Receiver<Reply>,
    stop: Arc<AtomicBool>,
    pub done: Arc<AtomicBool>,
}
impl Client {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        Self::start(wake, None)
    }
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Result<Self> {
        Self::start(Arc::new(|| {}), Some(path))
    }
    fn start(wake: Arc<dyn Fn() + Send + Sync>, path: Option<PathBuf>) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel(4);
        let (out, receiver) = mpsc::sync_channel(4);
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let finished = done.clone();
        std::thread::Builder::new()
            .name("mfalmion-storage".into())
            .spawn(move || {
                let send = |reply| return_send(reply, &out, &stopping, &wake);
                let result = path
                    .map_or_else(configured_path, Ok)
                    .and_then(|path| Store::open(&path));
                match result {
                    Err(e) => send(Reply::Error(e)),
                    Ok(mut store) => {
                        let initial = store
                            .recover_jobs()
                            .and_then(|_| store.recover_sync())
                            .and_then(|_| state(&store));
                        match initial {
                            Ok(r) => send(r),
                            Err(e) => send(Reply::Error(e)),
                        };
                        while !stopping.load(Ordering::Acquire) {
                            match rx.recv_timeout(Duration::from_millis(50)) {
                                Ok(command) => match execute(&mut store, command) {
                                    Ok(reply) => send(reply),
                                    Err(e) => send(Reply::Error(e)),
                                },
                                Err(mpsc::RecvTimeoutError::Timeout) => {}
                                Err(_) => break,
                            }
                        }
                        // No work is running automatically. Explicitly pause any future running job.
                        let _ = store.recover_jobs();
                        let _ = store.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
                    }
                }
                finished.store(true, Ordering::Release);
                wake();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            sender: tx,
            receiver,
            stop,
            done,
        })
    }
    pub fn send(&self, command: Command) -> Result<()> {
        self.sender
            .try_send(command)
            .map_err(|e| format!("Storage worker busy or unavailable: {e}"))
    }
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn return_send(
    mut value: Reply,
    out: &SyncSender<Reply>,
    stop: &AtomicBool,
    wake: &Arc<dyn Fn() + Send + Sync>,
) {
    while !stop.load(Ordering::Acquire) {
        match out.try_send(value) {
            Ok(()) => {
                wake();
                return;
            }
            Err(mpsc::TrySendError::Full(v)) => {
                value = v;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return,
        }
    }
}
fn state(store: &Store) -> Result<Reply> {
    let (preferences, mut warning) = store.preferences()?;
    let (sync_preferences, sync_warning) = store.sync_preferences()?;
    if warning.is_none() {
        warning = sync_warning;
    }
    Ok(Reply::State {
        profiles: store.profiles()?,
        active: store.active_profile()?,
        preferences,
        sync_preferences,
        warning,
        bytes: store.size()?,
    })
}
fn execute(store: &mut Store, command: Command) -> Result<Reply> {
    Ok(match command {
        Command::Refresh(profile, q) => Reply::Games(store.list_games(profile, &q)?),
        Command::Select(name) => {
            store.select_profile(&name)?;
            state(store)?
        }
        Command::DeleteProfile(id, confirmation) => {
            store.delete_profile(id, confirmation)?;
            state(store)?
        }
        Command::Import(profile, text) => Reply::Imported(store.import_pgn(profile, &text)?),
        Command::ImportRecords(profile, records) => {
            let mut report = ImportReport::default();
            for record in records {
                match store.insert_game(profile, &record, record.original_pgn.as_deref()) {
                    Ok(true) => report.imported += 1,
                    Ok(false) => report.skipped += 1,
                    Err(e) => report.errors.push(e),
                }
            }
            Reply::Imported(report)
        }
        Command::Open(profile, id) => {
            Reply::Opened(profile, id, Box::new(store.load_game(profile, id)?))
        }
        Command::Delete(profile, id, confirmed) => {
            store.delete_game(profile, id, confirmed)?;
            Reply::Done("Game deleted".into())
        }
        Command::Preferences(p) => {
            store.save_preferences(&p)?;
            Reply::Done("Settings saved".into())
        }
        Command::SyncPreferences(p) => {
            store.save_sync_preferences(&p)?;
            Reply::Done("Sync preferences saved".into())
        }
        Command::Integrity => Reply::Done(format!(
            "Database integrity: {}; size {} bytes",
            store.integrity()?,
            store.size()?
        )),
        Command::Backup(path) => {
            store.backup(&path)?;
            Reply::Done("Backup saved (existing paths never overwritten)".into())
        }
        Command::Restore(path, safety, confirmed) => {
            store.restore(&path, &safety, confirmed)?;
            state(store)?
        }
        Command::Cache(key, snapshot) => {
            store.cache_analysis(&key, &snapshot, true)?;
            Reply::Done("Completed analysis cached".into())
        }
        Command::Lookup(key) => {
            let lines = store.cached_analysis(&key)?;
            Reply::Cached(key, lines)
        }
    })
}
