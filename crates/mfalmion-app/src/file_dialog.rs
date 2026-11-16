//! Native dialogs and disk IO run only on explicit user action, on a worker thread.
use crate::notation::{self, GameRecord};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    process::Command,
    sync::mpsc::{self, Receiver},
};
pub enum FileReply {
    Imported(Vec<GameRecord>),
    Saved,
    Cancelled,
}
fn choose(save: bool) -> Result<Option<std::path::PathBuf>, String> {
    let mut command = Command::new("zenity");
    command.args(["--file-selection", "--file-filter=PGN files | *.pgn *.PGN"]);
    if save {
        command.args(["--save", "--filename=game.pgn"]);
    }
    let output = command
        .output()
        .map_err(|e| format!("Native file dialog unavailable: {e}. Use paste/copy PGN instead."))?;
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(format!(
            "Native file dialog failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let path = String::from_utf8(output.stdout).map_err(|_| "File path is not UTF-8")?;
    Ok(Some(path.trim_end_matches('\n').into()))
}
pub fn start(
    export: Option<String>,
    ctx: eframe::egui::Context,
) -> Receiver<Result<FileReply, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let Some(path) = choose(export.is_some())? else {
                return Ok(FileReply::Cancelled);
            };
            if let Some(text) = export {
                let overwrite = path.try_exists().map_err(|e| e.to_string())?;
                if overwrite {
                    let status = Command::new("zenity")
                        .args(["--question", "--title=Confirm PGN replacement", "--text"])
                        .arg(format!("Replace {}?", path.display()))
                        .status()
                        .map_err(|e| e.to_string())?;
                    if !status.success() {
                        return Ok(FileReply::Cancelled);
                    }
                }
                save_text(&path, &text, overwrite).map_err(|e| e.to_string())?;
                Ok(FileReply::Saved)
            } else {
                let file = fs::File::open(path).map_err(|e| e.to_string())?;
                let mut text = String::new();
                file.take(16 * 1024 * 1024 + 1)
                    .read_to_string(&mut text)
                    .map_err(|e| e.to_string())?;
                notation::parse_pgn(&text).map(FileReply::Imported)
            }
        })();
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    rx
}

pub fn parse_text(text: String, ctx: eframe::egui::Context) -> Receiver<Result<FileReply, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = notation::parse_pgn(&text).map(FileReply::Imported);
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    rx
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
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        if overwrite {
            fs::rename(&temporary, path)
        } else {
            fs::hard_link(&temporary, path)
        } // atomic no-clobber publication
    })();
    let _ = fs::remove_file(&temporary);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
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
        save_text(&path, "replacement", true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
        fs::remove_file(path).unwrap();
    }
}
