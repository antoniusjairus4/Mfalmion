//! Bounded scheduler, one SQLite connection and one engine worker per active job.
use super::*;
use crate::storage::{
    Store,
    batch::{Coverage, Selection},
    jobs::Status,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
    mpsc::{self, SyncSender},
};
#[derive(Clone, Debug)]
pub struct State {
    pub revision: u64,
    pub profile: i64,
    pub config: Config,
    pub coverage: Coverage,
    pub job: Option<i64>,
    pub status: String,
    pub current_game: Option<i64>,
    pub position: usize,
    pub positions: usize,
    pub completed: u32,
    pub total: u32,
    pub message: String,
    pub review: Option<(i64, i64, String, Arc<Summary>)>,
    pub statistics: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            revision: 0,
            profile: 0,
            config: Config::default(),
            coverage: Coverage::default(),
            job: None,
            status: "idle".into(),
            current_game: None,
            position: 0,
            positions: 0,
            completed: 0,
            total: 0,
            message: String::new(),
            review: None,
            statistics: String::new(),
        }
    }
}
pub enum Command {
    Inspect(i64, Option<i64>),
    Start(i64, Config, Selection),
    Resume(i64),
    Cancel(i64),
    Save(Config),
}
pub struct Client {
    tx: SyncSender<Command>,
    pub state: Arc<Mutex<State>>,
    pub control: Arc<AtomicU8>,
    pub active: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    pub done: Arc<AtomicBool>,
}
impl Client {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        Self::with_path(crate::storage::configured_path()?, wake)
    }
    pub(crate) fn with_path(
        path: std::path::PathBuf,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(2);
        let state = Arc::new(Mutex::new(State::default()));
        let control = Arc::new(AtomicU8::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let running = active.clone();
        let done = Arc::new(AtomicBool::new(false));
        let (s, c, q, d) = (state.clone(), control.clone(), stop.clone(), done.clone());
        std::thread::Builder::new()
            .name("mfalmion-batch".into())
            .spawn(move || {
                let result = (|| -> Result<(), String> {
                    let mut store = Store::open(&path)?;
                    {
                        let mut state = s.lock().unwrap_or_else(|p| p.into_inner());
                        match store.batch_config() {
                            Ok(config) => state.config = config,
                            Err(e) => state.message = e,
                        }
                    }
                    while !q.load(Ordering::Acquire) {
                        let command = match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                            Ok(v) => v,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(_) => break,
                        };
                        let is_run = matches!(command, Command::Start(..) | Command::Resume(..));
                        let result = match command {
                            Command::Inspect(profile, game) => inspect(&store, &s, profile, game),
                            Command::Save(config) => (|| {
                                store.save_batch_config(&config)?;
                                let profile = s.lock().unwrap_or_else(|p| p.into_inner()).profile;
                                inspect(&store, &s, profile, None)
                            })(),
                            Command::Cancel(profile) => (|| {
                                if let Some((job, _)) = store.resumable_batch(profile)? {
                                    store.stop_batch(job, true)?;
                                }
                                inspect(&store, &s, profile, None)
                            })(),
                            Command::Start(profile, config, selection) => (|| {
                                let job = store.start_batch(profile, &config, selection)?;
                                execute(
                                    &mut store,
                                    &s,
                                    &c,
                                    &wake,
                                    profile,
                                    job,
                                    &config,
                                    !matches!(selection, Selection::Reanalyze(_)),
                                )
                            })(
                            ),
                            Command::Resume(profile) => (|| {
                                let (job, config) = store
                                    .resumable_batch(profile)?
                                    .ok_or("No resumable account job")?;
                                let reuse = store.batch_reuse(job)?;
                                execute(&mut store, &s, &c, &wake, profile, job, &config, reuse)
                            })(),
                        };
                        if let Err(e) = result {
                            s.lock().unwrap_or_else(|p| p.into_inner()).message = e;
                        }
                        if is_run {
                            running.store(false, Ordering::Release);
                        }
                        s.lock().unwrap_or_else(|p| p.into_inner()).revision += 1;
                        wake();
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    s.lock().unwrap_or_else(|p| p.into_inner()).message =
                        format!("Batch worker unavailable: {e}");
                }
                running.store(false, Ordering::Release);
                d.store(true, Ordering::Release);
                wake();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx,
            state,
            control,
            active,
            stop,
            done,
        })
    }
    pub fn send(&self, command: Command) -> Result<(), String> {
        let run = matches!(command, Command::Start(..) | Command::Resume(..));
        if run {
            self.active
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| "An account job is already queued or running".to_string())?;
            self.control.store(0, Ordering::Release);
        }
        if let Err(e) = self.tx.try_send(command) {
            if run {
                self.active.store(false, Ordering::Release);
            }
            return Err(format!("Batch command queue: {e}"));
        }
        Ok(())
    }
    pub fn snapshot(&self) -> State {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    pub fn interrupt(&self, cancel: bool) {
        self.control
            .store(if cancel { 2 } else { 1 }, Ordering::Release);
    }
    pub fn shutdown(&self) {
        self.interrupt(false);
        self.stop.store(true, Ordering::Release);
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn inspect(
    store: &Store,
    state: &Mutex<State>,
    profile: i64,
    game: Option<i64>,
) -> Result<(), String> {
    let config = store.batch_config()?;
    let coverage = store.batch_coverage(profile, &config)?;
    let resumable = store.resumable_batch(profile)?;
    let review = game
        .map(|id| {
            store.completed_review(profile, id, &config).and_then(|r| {
                r.map(|r| {
                    store.load_game(profile, id).map(|record| {
                        (
                            profile,
                            id,
                            crate::storage::games::fingerprint(&record),
                            Arc::new(r),
                        )
                    })
                })
                .transpose()
            })
        })
        .transpose()?
        .flatten();
    let statistics = store.account_statistics(profile, &config)?;
    let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
    if s.profile != profile {
        s.status = "idle".into();
        s.total = 0;
        s.completed = 0;
        s.current_game = None;
        s.position = 0;
        s.positions = 0;
    }
    s.profile = profile;
    s.coverage = coverage;
    s.config = config;
    s.review = review;
    if let Some((job, _)) = resumable {
        let j = store.job(job)?;
        s.job = Some(job);
        s.status = j.status.name().into();
        s.total = j.total;
        s.completed = j.completed;
    } else {
        s.job = None;
        if matches!(s.status.as_str(), "running" | "paused" | "pending") {
            s.status = "idle".into();
        }
    }
    s.statistics = format!(
        "Compatible completed games: {} · account estimated accuracy: {} · eligible moves {} / {}",
        statistics.games,
        statistics
            .average_accuracy()
            .map_or_else(|| "—".into(), |v| format!("{v:.1}%")),
        statistics.eligible,
        statistics.total_moves
    );
    let rates = statistics.classification_rates();
    s.statistics.push_str(&format!(
        " · blunder rate {:.1}%",
        100.0 * rates.get("Blunder").copied().unwrap_or(0.0)
    ));
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn execute(
    store: &mut Store,
    state: &Mutex<State>,
    control: &AtomicU8,
    wake: &Arc<dyn Fn() + Send + Sync>,
    profile: i64,
    job: i64,
    config: &Config,
    reuse: bool,
) -> Result<(), String> {
    store.transition_job(job, Status::Running, None)?;
    store.batch_progress(job)?;
    {
        let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
        s.profile = profile;
        s.config = config.clone();
        s.job = Some(job);
        s.status = "running".into();
        s.total = store.job(job)?.total;
        s.message.clear();
        s.review = None;
    }
    wake();
    let result = (|| -> Result<(), String> {
        let mut analyzer = runner::UciAnalyzer::new()?;
        while let Some(pending) = store.next_batch_game(job)? {
            if control.load(Ordering::Acquire) != 0 {
                break;
            }
            {
                let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
                s.current_game = Some(pending.game);
                s.position = 0;
                s.positions = 0;
            }
            let r = runner::analyze_game(
                store,
                &pending,
                profile,
                config,
                reuse,
                &mut analyzer,
                control,
                |position, total| {
                    let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
                    s.position = position;
                    s.positions = total;
                    drop(s);
                    wake();
                },
            );
            if control.load(Ordering::Acquire) != 0 {
                break;
            }
            if let Err(e) = r {
                store.mark_batch_run(pending.run, "failed", Some(&e))?;
                state.lock().unwrap_or_else(|p| p.into_inner()).message =
                    format!("Game {} failed: {e}", pending.game);
            }
            let (done, _) = store.batch_progress(job)?;
            state.lock().unwrap_or_else(|p| p.into_inner()).completed = done;
            wake();
        }
        Ok(())
    })();
    let interrupted = control.load(Ordering::Acquire);
    let status = if interrupted != 0 {
        store.stop_batch(job, interrupted == 2)?;
        if interrupted == 2 {
            "cancelled"
        } else {
            "paused"
        }
    } else if let Err(e) = result {
        store.db.execute("UPDATE batch_runs SET status='failed',last_error=?2 WHERE job_id=?1 AND status IN('pending','running','paused')",rusqlite::params![job,e]).map_err(|e|e.to_string())?;
        store.batch_progress(job)?;
        store.transition_job(job, Status::Failed, Some(&e))?;
        state.lock().unwrap_or_else(|p| p.into_inner()).message = e;
        "failed"
    } else {
        let (_, failed) = store.batch_progress(job)?;
        store.transition_job(
            job,
            if failed > 0 {
                Status::Failed
            } else {
                Status::Completed
            },
            if failed > 0 {
                Some("One or more games failed; retry failed games")
            } else {
                None
            },
        )?;
        if failed > 0 { "failed" } else { "completed" }
    };
    {
        let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
        s.status = status.into();
        s.current_game = None;
        s.coverage = store.batch_coverage(profile, config)?;
    }
    inspect(store, state, profile, None)?;
    // Drop of analyzer has already initiated quit/reaping. The engine lease remains held
    // by its worker until reaping completes, so an overlapping Start fails safely.
    Ok(())
}
