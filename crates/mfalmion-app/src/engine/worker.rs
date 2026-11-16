use super::{
    process::{ChildSlot, EngineProcess},
    protocol::{EngineOption, Message, Snapshots},
    *,
};
use shakmaty::{Color, Position, uci::UciMove};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
const POLL: Duration = Duration::from_millis(10);
#[derive(Clone, Copy)]
pub(super) struct Timeouts {
    pub startup: Duration,
    pub ready: Duration,
    pub cancel: Duration,
    pub search: Duration,
}
impl Default for Timeouts {
    fn default() -> Self {
        Self {
            startup: Duration::from_secs(5),
            ready: Duration::from_secs(3),
            cancel: Duration::from_secs(2),
            search: Duration::from_secs(35),
        }
    }
}

pub struct EngineClient {
    mailbox: Arc<Mailbox>,
    results: Arc<Results>,
    child: ChildSlot,
    done: Receiver<()>,
}
impl EngineClient {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        Self::build(Timeouts::default(), Some(wake))
    }
    #[cfg(test)]
    pub(super) fn with_timeouts(timeouts: Timeouts) -> Result<Self, String> {
        Self::build(timeouts, None)
    }
    fn build(
        timeouts: Timeouts,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Self, String> {
        #[cfg(not(test))]
        let lease = Lease::acquire()?;
        let mailbox = Arc::new(Mailbox::default());
        let results = Arc::new(Results {
            wake,
            ..Results::default()
        });
        let child = Arc::new(Mutex::new(None));
        let (finished, done) = completion();
        let mut worker = Worker {
            mailbox: mailbox.clone(),
            results: results.clone(),
            child: child.clone(),
            engine: None,
            timeouts,
            last_id: 0,
        };
        std::thread::Builder::new()
            .name("mfalmion-uci".into())
            .spawn(move || {
                #[cfg(not(test))]
                let _lease = lease;
                worker.run();
                drop(worker);
                let _ = finished.send(());
            })
            .map_err(|e| format!("Cannot start engine worker: {e}"))?;
        Ok(Self {
            mailbox,
            results,
            child,
            done,
        })
    }
    pub fn analyze(&self, request: Request) {
        self.mailbox.put(Work::Analyze(request));
    }
    pub fn stop(&self, id: u64) {
        self.mailbox.put(Work::Stop(id));
    }
    pub fn drain(&self) -> Vec<Event> {
        self.results.drain()
    }
    pub fn shutdown(&self) {
        self.mailbox.shutdown.store(true, Ordering::Release);
    }
    pub fn is_shutdown(&self) -> bool {
        !matches!(self.done.try_recv(), Err(mpsc::TryRecvError::Empty))
    }
    #[cfg(test)]
    pub fn pid(&self) -> Option<u32> {
        self.child
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|child| child.id())
    }
}
impl Drop for EngineClient {
    fn drop(&mut self) {
        self.shutdown();
        // Normal close uses asynchronous completion first. Emergency teardown signals kill
        // without waiting on the GUI thread; the worker remains responsible for wait/reap.
        if let Some(child) = self
            .child
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_mut()
        {
            let _ = child.kill();
        }
    }
}
struct Connected {
    process: EngineProcess,
    options: BTreeMap<String, EngineOption>,
    path: String,
    name: String,
    session: Option<u64>,
    applied: Option<Settings>,
}
struct Worker {
    mailbox: Arc<Mailbox>,
    results: Arc<Results>,
    child: ChildSlot,
    engine: Option<Connected>,
    timeouts: Timeouts,
    last_id: u64,
}
impl Worker {
    fn status(&self, id: u64, status: Status) {
        self.results.send(Event::Status(id, status));
    }
    fn wait(
        &self,
        engine: &EngineProcess,
        duration: Duration,
        mut accept: impl FnMut(Message) -> bool,
    ) -> Result<(), String> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if self.mailbox.interrupted() {
                return Err("Analysis cancelled".into());
            }
            if let Some(line) = engine.receive(POLL)?
                && let Ok(message) = protocol::parse(&line, Color::White)
                && accept(message)
            {
                return Ok(());
            }
        }
        Err("Engine response timed out".into())
    }
    fn connect(&mut self, request: &Request) -> Result<(), String> {
        request.settings.validate()?;
        if self
            .engine
            .as_ref()
            .is_some_and(|e| e.path != request.settings.path)
        {
            self.engine.take();
        }
        if self.engine.is_none() {
            self.status(request.id, Status::Starting);
            let process = EngineProcess::launch(&request.settings.path, self.child.clone())?;
            process.send("uci")?;
            let mut options = BTreeMap::new();
            let mut name = "UCI engine".into();
            self.wait(&process, self.timeouts.startup, |message| {
                match message {
                    Message::UciOk => return true,
                    Message::Name(value) => name = value,
                    Message::Option(key, value) if options.len() < 256 => {
                        options.insert(key, value);
                    }
                    _ => {}
                }
                false
            })
            .map_err(|e| format!("UCI startup: {e}"))?;
            self.results.send(Event::Identity(request.id, name.clone()));
            self.engine = Some(Connected {
                process,
                options,
                path: request.settings.path.clone(),
                name,
                session: None,
                applied: None,
            });
        }
        let engine = self
            .engine
            .as_ref()
            .ok_or("Engine connection unavailable")?;
        for (name, value) in [
            ("Threads", request.settings.threads),
            ("Hash", request.settings.hash_mib),
            ("MultiPV", request.settings.multipv as u32),
        ] {
            let changed = engine.applied.as_ref().is_none_or(|settings| match name {
                "Threads" => settings.threads != value,
                "Hash" => settings.hash_mib != value,
                _ => settings.multipv != value as usize,
            });
            if !changed {
                continue;
            }
            match engine.options.get(name) {
                Some(EngineOption::Spin(option)) if (option.min..=option.max).contains(&value) => {
                    engine
                        .process
                        .send(&format!("setoption name {name} value {value}"))?
                }
                _ => return Err(format!("Engine does not support {name}={value}")),
            }
        }
        if engine.applied.is_none()
            && engine.options.get("UCI_AnalyseMode") == Some(&EngineOption::Check)
        {
            engine
                .process
                .send("setoption name UCI_AnalyseMode value true")?;
        }
        if request.history.is_some() {
            if !engine.options.contains_key("Clear Hash") {
                return Err("Batch analysis requires Clear Hash support".into());
            }
            engine.process.send("ucinewgame")?;
            engine.process.send("setoption name Clear Hash")?;
        }
        if engine.session != Some(request.session) {
            engine.process.send("ucinewgame")?;
        }
        engine.process.send("isready")?;
        self.wait(&engine.process, self.timeouts.ready, |message| {
            message == Message::ReadyOk
        })
        .map_err(|e| format!("Engine readiness: {e}"))?;
        if let Some(engine) = &mut self.engine {
            engine.session = Some(request.session);
            engine.applied = Some(request.settings.clone());
        }
        self.status(request.id, Status::Ready);
        Ok(())
    }
    fn cancel(&mut self, id: u64) -> Result<(), String> {
        let Some(engine) = &self.engine else {
            return Ok(());
        };
        self.status(id, Status::Stopping);
        engine.process.send("stop")?;
        let deadline = Instant::now() + self.timeouts.cancel;
        while Instant::now() < deadline {
            if self.mailbox.shutdown.load(Ordering::Acquire) {
                return Ok(());
            }
            if let Some(line) = engine.process.receive(POLL)?
                && matches!(
                    protocol::parse(&line, Color::White),
                    Ok(Message::BestMove(_))
                )
            {
                return Ok(());
            }
        }
        Err("Engine did not acknowledge stop before cancellation timeout".into())
    }
    fn error(&mut self, id: u64, error: String) {
        self.engine.take();
        if !(self.mailbox.interrupted() && error.contains("Analysis cancelled")) {
            log::warn!("event=engine_error request_id={id} error={error}");
            self.results.send(Event::Error(id, error));
        }
    }
    fn run(&mut self) {
        let mut active: Option<(Request, Snapshots, Instant)> = None;
        while !self.mailbox.shutdown.load(Ordering::Acquire) {
            if let Some(work) = self.mailbox.take() {
                let id = match &work {
                    Work::Analyze(r) => r.id,
                    Work::Stop(id) => *id,
                };
                self.last_id = id;
                if active.take().is_some()
                    && let Err(error) = self.cancel(id)
                {
                    self.error(id, error);
                }
                let work = self.mailbox.take().unwrap_or(work); // latest navigation wins while stop drains
                match work {
                    Work::Stop(id) => {
                        self.last_id = id;
                        self.status(
                            id,
                            if self.engine.is_some() {
                                Status::Ready
                            } else {
                                Status::Offline
                            },
                        );
                    }
                    Work::Analyze(mut request) => {
                        self.last_id = request.id;
                        if let Err(error) = self.connect(&request) {
                            self.error(request.id, error);
                            continue;
                        }
                        // Handshake/readiness may span several UI changes; never search the old one.
                        if let Some(latest) = self.mailbox.take() {
                            match latest {
                                Work::Stop(id) => {
                                    self.last_id = id;
                                    self.status(id, Status::Ready);
                                    continue;
                                }
                                Work::Analyze(next) => {
                                    if request.settings != next.settings
                                        || request.session != next.session
                                    {
                                        self.mailbox.put(Work::Analyze(next));
                                        continue;
                                    }
                                    request = next;
                                }
                            }
                        }
                        if self.mailbox.interrupted() {
                            continue;
                        }
                        let Some(engine) = &self.engine else {
                            continue;
                        };
                        let command = match &request.history {
                            Some(history) => match history.command(&request.position) {
                                Ok(command) => command,
                                Err(e) => {
                                    self.error(request.id, e);
                                    continue;
                                }
                            },
                            None => {
                                format!("position fen {}", crate::notation::fen(&request.position))
                            }
                        };
                        let sent = engine.process.send(&command).and_then(|()| {
                            engine.process.send(&format!(
                                "go depth {} movetime 30000",
                                request.settings.depth
                            ))
                        });
                        if let Err(error) = sent {
                            self.error(request.id, error);
                            continue;
                        }
                        self.last_id = request.id;
                        self.results
                            .send(Event::Identity(request.id, engine.name.clone()));
                        self.status(request.id, Status::Analyzing);
                        let legal = request.position.legal_moves().len();
                        let snapshots =
                            Snapshots::new(request.settings.multipv.min(legal).max(1), legal == 0);
                        active = Some((request, snapshots, Instant::now()));
                    }
                }
            }
            if let Some((request, snapshots, started)) = &mut active {
                let id = request.id;
                if started.elapsed() > self.timeouts.search {
                    active = None;
                    self.error(
                        id,
                        "Engine search exceeded the bounded search watchdog".into(),
                    );
                    continue;
                }
                let response = self
                    .engine
                    .as_ref()
                    .ok_or_else(|| "Engine disconnected".into())
                    .and_then(|engine| engine.process.receive(POLL));
                match response {
                    Ok(Some(line)) => match protocol::parse(&line, request.position.turn()) {
                        Ok(Message::Info(info)) => {
                            if let Some(snapshot) = snapshots.push(info) {
                                self.results.send(Event::Snapshot(id, snapshot));
                            }
                        }
                        Ok(Message::BestMove(mv)) => {
                            let san = match mv {
                                None if request.position.legal_moves().is_empty() => Ok(None),
                                Some(text) => text
                                    .parse::<UciMove>()
                                    .ok()
                                    .and_then(|uci| uci.to_move(&request.position).ok())
                                    .map(|_| {
                                        Some(protocol::decode_pv(&request.position, &[text]).text)
                                    })
                                    .ok_or("Illegal bestmove from engine"),
                                None => Err("Engine returned no move for a playable position"),
                            };
                            active = None;
                            match san {
                                Ok(san) => {
                                    self.results.send(Event::BestMove(id, san));
                                    self.status(id, Status::Ready);
                                }
                                Err(e) => self.error(id, e.into()),
                            }
                        }
                        _ => {} // Unknown/partial/malformed info cannot corrupt a snapshot.
                    },
                    Err(error) => {
                        active = None;
                        self.error(id, error);
                    }
                    Ok(None) => {}
                }
            } else if let Some(engine) = &self.engine {
                if let Err(error) = engine.process.receive(POLL) {
                    self.error(self.last_id, error);
                }
            } else {
                std::thread::sleep(POLL);
            }
        }
        if let Some(engine) = &self.engine {
            let _ = engine.process.send("stop");
        }
        self.engine.take(); // bounded quit, then kill/reap on this worker, never on egui.
    }
}

#[cfg(not(test))]
static OWNED: AtomicBool = AtomicBool::new(false);
#[cfg(not(test))]
struct Lease;
#[cfg(not(test))]
impl Lease {
    fn acquire() -> Result<Self, String> {
        OWNED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Another analysis worker owns Stockfish; stop it first".to_string())?;
        Ok(Self)
    }
}
#[cfg(not(test))]
impl Drop for Lease {
    fn drop(&mut self) {
        OWNED.store(false, Ordering::Release);
    }
}
