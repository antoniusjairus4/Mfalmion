//! Explicit native dialogs and bounded file operations. No shell interpolation.
use crate::notation::{self, GameRecord};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
};
pub enum FileReply {
    Imported(Vec<GameRecord>),
    Saved,
    Cancelled,
}
#[derive(Default)]
struct DialogControl {
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}
pub struct FileTask {
    receiver: Receiver<Result<FileReply, String>>,
    control: Arc<DialogControl>,
}
impl FileTask {
    pub fn try_recv(&self) -> Result<Result<FileReply, String>, TryRecvError> {
        self.receiver.try_recv()
    }
}
impl Drop for FileTask {
    fn drop(&mut self) {
        self.control.cancelled.store(true, Ordering::Release);
        let mut slot = self
            .control
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn run_dialog(mut command: Command, control: &DialogControl) -> Result<Output, String> {
    {
        let mut slot = control
            .child
            .lock()
            .map_err(|_| "Native dialog state unavailable")?;
        if control.cancelled.load(Ordering::Acquire) {
            return Err("File operation cancelled".into());
        }
        *slot = Some(
            command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    format!("Native file dialog unavailable: {e}. Use paste/copy PGN instead.")
                })?,
        );
    }
    loop {
        if control.cancelled.load(Ordering::Acquire) {
            return Err("File operation cancelled".into());
        }
        let mut slot = control
            .child
            .lock()
            .map_err(|_| "Native dialog state unavailable")?;
        let child = slot.as_mut().ok_or("Native dialog cancelled")?;
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            let child = slot.take().ok_or("Native dialog cancelled")?;
            drop(slot);
            return child.wait_with_output().map_err(|e| e.to_string());
        }
        drop(slot);
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
fn dialog_success(output: &Output) -> Result<bool, String> {
    if output.status.success() {
        Ok(true)
    } else if output.status.code() == Some(1) {
        Ok(false)
    } else {
        Err(format!(
            "Native file dialog failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
fn selected_path(mut bytes: Vec<u8>) -> Result<PathBuf, String> {
    // zenity appends exactly one LF. Preserve any LF that belongs to the filename.
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return Err("File dialog returned an empty or invalid path".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(bytes).into())
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes)
            .map(PathBuf::from)
            .map_err(|_| "File path is not UTF-8".into())
    }
}
fn choose(save: bool, control: &DialogControl) -> Result<Option<PathBuf>, String> {
    let mut command = Command::new("zenity");
    command.args(["--file-selection", "--file-filter=PGN files | *.pgn *.PGN"]);
    if save {
        command.args(["--save", "--filename=game.pgn"]);
    }
    let output = run_dialog(command, control)?;
    if !dialog_success(&output)? {
        return Ok(None);
    }
    selected_path(output.stdout).map(Some)
}
fn read_pgn(path: &Path) -> Result<Vec<GameRecord>, String> {
    let metadata = fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > notation::MAX_PGN_BYTES as u64 {
        return Err("PGN exceeds the 16 MiB import limit".into());
    }
    if !metadata.is_file() {
        return Err("Select a regular PGN file".into());
    }
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(notation::MAX_PGN_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("Cannot read UTF-8 PGN: {e}"))?;
    notation::parse_pgn(&text)
}
fn worker(
    ctx: eframe::egui::Context,
    job: impl FnOnce(&DialogControl) -> Result<FileReply, String> + Send + 'static,
) -> FileTask {
    let (tx, receiver) = mpsc::channel();
    let control = Arc::new(DialogControl::default());
    let background = control.clone();
    let failure_sender = tx.clone();
    let failure_context = ctx.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("mfalmion-file".into())
        .spawn(move || {
            let result = job(&background);
            let _ = tx.send(result);
            ctx.request_repaint();
        })
    {
        let _ = failure_sender.send(Err(format!("Cannot start file worker: {error}")));
        failure_context.request_repaint();
    }
    FileTask { receiver, control }
}
pub fn start(export: Option<String>, ctx: eframe::egui::Context) -> FileTask {
    worker(ctx, move |control| {
        let Some(path) = choose(export.is_some(), control)? else {
            return Ok(FileReply::Cancelled);
        };
        if let Some(text) = export {
            let overwrite = path.try_exists().map_err(|e| e.to_string())?;
            if overwrite {
                let mut command = Command::new("zenity");
                command
                    .args([
                        "--question",
                        "--no-markup",
                        "--title=Confirm PGN replacement",
                        "--text",
                    ])
                    .arg(format!("Replace {}?", path.display()));
                if !dialog_success(&run_dialog(command, control)?)? {
                    return Ok(FileReply::Cancelled);
                }
            }
            if control.cancelled.load(Ordering::Acquire) {
                return Ok(FileReply::Cancelled);
            }
            save_text(&path, &text, overwrite).map_err(|e| e.to_string())?;
            Ok(FileReply::Saved)
        } else {
            read_pgn(&path).map(FileReply::Imported)
        }
    })
}
pub fn parse_text(text: String, ctx: eframe::egui::Context) -> FileTask {
    worker(ctx, move |_| {
        notation::parse_pgn(&text).map(FileReply::Imported)
    })
}

fn save_text(path: &std::path::Path, text: &str, overwrite: bool) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".mfalmion-{}-{unique}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        if overwrite {
            file.set_permissions(fs::metadata(path)?.permissions())?;
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        if overwrite {
            fs::rename(&temporary, path)
        } else {
            fs::hard_link(&temporary, path)
        } // atomic no-clobber publication
    })();
    if let Err(error) = fs::remove_file(&temporary)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("event=pgn_temporary_cleanup_failed error={error}");
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audit_path_decode_preserves_newline_and_non_utf8_names() {
        assert_eq!(
            selected_path(b"/tmp/game.pgn\n\n".to_vec()).unwrap(),
            PathBuf::from("/tmp/game.pgn\n")
        );
        assert!(selected_path(b"\n".to_vec()).is_err());
        assert!(selected_path(b"/tmp/a\0b\n".to_vec()).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            assert_eq!(
                selected_path(b"/tmp/\xff.pgn\n".to_vec())
                    .unwrap()
                    .as_os_str()
                    .as_bytes(),
                b"/tmp/\xff.pgn"
            );
        }
    }
    #[test]
    fn audit_invalid_utf8_and_non_regular_import_do_not_parse() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mfalmion-utf8-test-{unique}.pgn"));
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(read_pgn(&path).unwrap_err().contains("UTF-8"));
        fs::remove_file(path).unwrap();
        assert!(
            read_pgn(&std::env::temp_dir())
                .unwrap_err()
                .contains("regular")
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn audit_dropping_file_task_reaps_its_child_without_gui() {
        let child = Command::new("sleep").arg("10").spawn().unwrap();
        let pid = child.id();
        let control = Arc::new(DialogControl::default());
        *control.child.lock().unwrap() = Some(child);
        let (_sender, receiver) = mpsc::channel();
        drop(FileTask {
            receiver,
            control: control.clone(),
        });
        assert!(control.cancelled.load(Ordering::Acquire));
        assert!(control.child.lock().unwrap().is_none());
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }

    #[test]
    fn export_requires_confirmation_before_replacement() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mfalmion-export-test-{unique}.pgn"));
        save_text(&path, "original", false).unwrap();
        assert!(save_text(&path, "replacement", false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        save_text(&path, "replacement", true).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
        fs::remove_file(path).unwrap();
    }
}
