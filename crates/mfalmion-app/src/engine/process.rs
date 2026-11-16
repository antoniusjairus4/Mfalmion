//! One protocol owner; bounded pipe-reader/writer threads never touch egui.
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub type ChildSlot = Arc<Mutex<Option<Child>>>;
pub enum Output {
    Line(String),
    Failure(String),
}
pub struct EngineProcess {
    pub child: ChildSlot,
    input: Option<SyncSender<String>>,
    output: Option<Receiver<Output>>,
    readers: Vec<JoinHandle<()>>,
}
impl EngineProcess {
    pub fn launch(path: &str, child: ChildSlot) -> Result<Self, String> {
        let mut process = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Cannot start engine {path}: {e}"))?;
        let Some(mut stdin) = process.stdin.take() else {
            let _ = process.kill();
            let _ = process.wait();
            return Err("Engine stdin unavailable".into());
        };
        let Some(stdout) = process.stdout.take() else {
            let _ = process.kill();
            let _ = process.wait();
            return Err("Engine stdout unavailable".into());
        };
        *child.lock().unwrap_or_else(|p| p.into_inner()) = Some(process);
        let (tx, output) = mpsc::sync_channel(256);
        let (input, writes) = mpsc::sync_channel::<String>(16);
        let reader = thread::Builder::new()
            .name("uci-stdout".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = Vec::new();
                loop {
                    // fill_buf handles partial messages without an unbounded read_line allocation.
                    let buffer = match reader.fill_buf() {
                        Ok(buffer) => buffer,
                        Err(e) => {
                            let _ = tx.send(Output::Failure(format!("Engine output: {e}")));
                            break;
                        }
                    };
                    if buffer.is_empty() {
                        let _ = tx.send(Output::Failure("Engine stdout EOF".into()));
                        break;
                    }
                    let end = buffer
                        .iter()
                        .position(|b| *b == b'\n')
                        .map_or(buffer.len(), |n| n + 1);
                    if line.len() + end > 65_536 {
                        let _ =
                            tx.send(Output::Failure("Engine output line exceeds 64 KiB".into()));
                        break;
                    }
                    let complete = buffer[end - 1] == b'\n';
                    line.extend_from_slice(&buffer[..end]);
                    reader.consume(end);
                    if complete {
                        match String::from_utf8(std::mem::take(&mut line)) {
                            Ok(line) => {
                                if tx.send(Output::Line(line)).is_err() {
                                    break;
                                }
                            }
                            Err(_) => {
                                let _ =
                                    tx.send(Output::Failure("Engine output is not UTF-8".into()));
                                break;
                            }
                        }
                    }
                }
            });
        let reader = match reader {
            Ok(reader) => reader,
            Err(e) => {
                kill(&child);
                return Err(format!("Cannot start engine reader: {e}"));
            }
        };
        let writer = thread::Builder::new()
            .name("uci-stdin".into())
            .spawn(move || {
                while let Ok(command) = writes.recv() {
                    if writeln!(stdin, "{command}")
                        .and_then(|()| stdin.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            });
        let writer = match writer {
            Ok(writer) => writer,
            Err(e) => {
                drop(output);
                kill(&child);
                if reader.is_finished() {
                    let _ = reader.join();
                }
                return Err(format!("Cannot start engine writer: {e}"));
            }
        };
        Ok(Self {
            child,
            input: Some(input),
            output: Some(output),
            readers: vec![reader, writer],
        })
    }
    pub fn send(&self, command: &str) -> Result<(), String> {
        self.input
            .as_ref()
            .ok_or("Engine transport closed")?
            .try_send(command.into())
            .map_err(|e| format!("Engine command transport: {e}"))
    }
    pub fn receive(&self, timeout: Duration) -> Result<Option<String>, String> {
        match self
            .output
            .as_ref()
            .ok_or("Engine transport closed")?
            .recv_timeout(timeout)
        {
            Ok(Output::Line(line)) => Ok(Some(line)),
            Ok(Output::Failure(e)) => Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(_) => Err("Engine output channel closed".into()),
        }
    }
    pub fn shutdown(&mut self) {
        let _ = self.send("quit");
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let exited = self
                .child
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
                .is_none_or(|child| child.try_wait().ok().flatten().is_some());
            if exited || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        self.input.take();
        self.output.take();
        kill(&self.child);
        let deadline = Instant::now() + Duration::from_millis(100);
        while self.readers.iter().any(|h| !h.is_finished()) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        for reader in self.readers.drain(..) {
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
    }
}
pub fn kill(slot: &ChildSlot) {
    let owned = slot.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(mut child) = owned {
        let _ = child.kill();
        let _ = child.wait();
    }
}
impl Drop for EngineProcess {
    fn drop(&mut self) {
        self.shutdown();
    }
}
