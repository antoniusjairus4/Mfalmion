//! One selected-account sync at a time; one reusable connection to the SAME SQLite file.
use super::*;
use crate::storage::Store;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    time::Duration,
};
#[derive(Clone)]
pub struct Update {
    pub progress: Progress,
    pub failure: Option<Failure>,
    pub finished: bool,
}
enum Command {
    Start(i64, bool),
    Info(i64),
}
pub struct Client {
    sender: SyncSender<Command>,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    pub busy: Arc<AtomicBool>,
    pub done: Arc<AtomicBool>,
    updates: Arc<Mutex<Option<Update>>>,
}
impl Client {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> std::result::Result<Self, String> {
        Self::spawn(None, Box::new(client::Http::new()), wake)
    }
    #[cfg(test)]
    pub fn at(
        path: PathBuf,
        transport: Box<dyn client::Transport>,
    ) -> std::result::Result<Self, String> {
        Self::spawn(Some(path), transport, Arc::new(|| {}))
    }
    fn spawn(
        path: Option<PathBuf>,
        transport: Box<dyn client::Transport>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::result::Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let updates = Arc::new(Mutex::new(None));
        let (c, s, b, d, u) = (
            cancel.clone(),
            stop.clone(),
            busy.clone(),
            done.clone(),
            updates.clone(),
        );
        std::thread::Builder::new()
            .name("mfalmion-chesscom".into())
            .spawn(move || {
                let emit = |update| {
                    *u.lock().unwrap_or_else(|p| p.into_inner()) = Some(update);
                    wake();
                };
                let opened = path
                    .map_or_else(crate::storage::configured_path, Ok)
                    .and_then(|p| Store::open(&p));
                match opened {
                    Err(e) => emit(Update {
                        progress: Progress {
                            status: "error".into(),
                            error: Some(e.clone()),
                            ..Default::default()
                        },
                        failure: Some(e.into()),
                        finished: true,
                    }),
                    Ok(mut store) => {
                        let mut runner = runner::Runner { transport };
                        while !s.load(Ordering::Acquire) {
                            match rx.recv_timeout(Duration::from_millis(50)) {
                                Ok(Command::Start(profile, full)) => {
                                    let (p, f) = runner.run(
                                        &mut store,
                                        profile,
                                        full,
                                        &c,
                                        now(),
                                        |progress| {
                                            emit(Update {
                                                progress,
                                                failure: None,
                                                finished: false,
                                            })
                                        },
                                    );
                                    emit(Update {
                                        progress: p,
                                        failure: f,
                                        finished: true,
                                    });
                                    b.store(false, Ordering::Release);
                                    wake();
                                }
                                Ok(Command::Info(profile)) => {
                                    match store.sync_info(profile) {
                                        Ok(progress) => emit(Update {
                                            progress,
                                            failure: None,
                                            finished: true,
                                        }),
                                        Err(e) => emit(Update {
                                            progress: Progress {
                                                profile,
                                                status: "error".into(),
                                                error: Some(e.clone()),
                                                ..Default::default()
                                            },
                                            failure: Some(e.into()),
                                            finished: true,
                                        }),
                                    }
                                    b.store(false, Ordering::Release);
                                    wake();
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => {}
                                Err(_) => break,
                            }
                        }
                        let _ = store.db.execute_batch("PRAGMA wal_checkpoint(PASSIVE)");
                    }
                }
                b.store(false, Ordering::Release);
                d.store(true, Ordering::Release);
                wake();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            sender: tx,
            cancel,
            stop,
            busy,
            done,
            updates,
        })
    }
    fn send(&self, command: Command) -> std::result::Result<(), String> {
        if self.done.load(Ordering::Acquire) || self.stop.load(Ordering::Acquire) {
            return Err("Chess.com worker has stopped".into());
        }
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "A Chess.com operation is already active")?;
        self.cancel.store(false, Ordering::Release);
        if let Err(e) = self.sender.try_send(command) {
            self.busy.store(false, Ordering::Release);
            return Err(format!("Cannot queue sync: {e}"));
        }
        Ok(())
    }
    pub fn start(&self, profile: i64, full: bool) -> std::result::Result<(), String> {
        self.send(Command::Start(profile, full))
    }
    pub fn info(&self, profile: i64) -> std::result::Result<(), String> {
        self.send(Command::Info(profile))
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        self.cancel();
    }
    pub fn take(&self) -> Option<Update> {
        self.updates
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}
impl client::Transport for Box<dyn client::Transport> {
    fn get(
        &mut self,
        url: &str,
        cache: &crate::storage::sync::HttpCache,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<client::Response> {
        (**self).get(url, cache, limit, cancel)
    }
}
