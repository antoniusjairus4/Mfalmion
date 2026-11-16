//! Single-owner SQLite persistence. No UI or chess-rule authority lives here.
pub mod analysis;
pub mod games;
pub mod jobs;
mod schema;
pub mod settings;
pub mod sync;
#[cfg(test)]
mod tests;
pub mod worker;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub type Result<T> = std::result::Result<T, String>;
pub struct Store {
    pub(crate) db: Connection,
    pub path: PathBuf,
}
pub fn digest(value: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
/// Explicit overrides require an existing parent: an absent portable drive is an error.
pub fn configured_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("MFALMION_DATABASE") {
        return startup_path(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("MFALMION_DATA_DIR") {
        return startup_path(PathBuf::from(path).join("mfalmion.sqlite3"));
    }
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .ok_or("No application data directory; set MFALMION_DATABASE")?;
    let root = root.join("mfalmion");
    std::fs::create_dir_all(&root).map_err(|e| format!("Cannot create data directory: {e}"))?;
    startup_path(root.join("mfalmion.sqlite3"))
}
fn startup_path(path: PathBuf) -> Result<PathBuf> {
    if std::env::var_os("MFALMION_REQUIRE_DATABASE").is_some_and(|v| v == "1") && !path.is_file() {
        return Err(
            "Expected database is absent; reconnect the configured drive. No database created"
                .into(),
        );
    }
    Ok(path)
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if path.as_os_str().is_empty()
            || !path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .is_dir()
        {
            return Err("Database parent directory is missing; check the configured drive".into());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let absolute = if path.exists() {
            std::fs::canonicalize(path)
        } else {
            std::fs::canonicalize(parent).map(|p| p.join(path.file_name().unwrap_or_default()))
        }
        .map_err(|e| e.to_string())?;
        if absolute
            .ancestors()
            .skip(1)
            .any(|parent| parent.join(".git").exists())
        {
            return Err(
                "Database must be outside Git repositories and their target directories".into(),
            );
        }
        let mut db = Connection::open(path).map_err(|e| format!("Cannot open database: {e}"))?;
        db.busy_timeout(Duration::from_millis(750))
            .map_err(|e| e.to_string())?;
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > schema::VERSION {
            return Err("Database was created by a newer application; no changes applied".into());
        }
        let application: i64 = db
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 0 && application != 0x4d46414c {
            return Err("Not a Mfalmion database".into());
        }
        if version == 0 {
            let tables:i64=db.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",[],|r|r.get(0)).map_err(|e|e.to_string())?;
            if tables != 0 {
                return Err("Refusing to initialize an unrelated nonempty database".into());
            }
        }
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=256; PRAGMA journal_size_limit=4194304;").map_err(|e| format!("Database initialization: {e}"))?;
        if version == 0 {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            tx.execute_batch(schema::SQL).map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO schema_migrations(version) VALUES(?1)", [1_i64])
                .map_err(|e| e.to_string())?;
            tx.pragma_update(None, "application_id", 0x4d46414c_i64)
                .map_err(|e| e.to_string())?;
            tx.pragma_update(None, "user_version", 1_i64)
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        migrate_sync(&mut db, version)?;
        let mut store = Self {
            db,
            path: path.to_owned(),
        };
        // Recovery is explicit per worker startup, not on each connection or backup.
        if store.profiles()?.is_empty() {
            store.select_profile("Antonius-Jairus-R")?;
        }
        Ok(store)
    }
    pub fn profiles(&self) -> Result<Vec<(i64, String, bool)>> {
        self.db
            .prepare("SELECT id,username,active FROM profiles ORDER BY username COLLATE NOCASE")
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect()
            })
            .map_err(|e| e.to_string())
    }
    pub fn active_profile(&self) -> Result<i64> {
        self.db
            .query_row("SELECT id FROM profiles WHERE active=1", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    }
    pub fn select_profile(&mut self, username: &str) -> Result<i64> {
        if username.is_empty()
            || username.len() > 100
            || !username
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(
                "Username must contain 1–100 letters, digits, underscores or hyphens".into(),
            );
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO profiles(username) VALUES(?1) ON CONFLICT(username) DO NOTHING",
            [username],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("UPDATE profiles SET active=0 WHERE active=1", [])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE profiles SET active=1,updated_at=unixepoch() WHERE username=?1 COLLATE NOCASE",
            [username],
        )
        .map_err(|e| e.to_string())?;
        let id = tx
            .query_row("SELECT id FROM profiles WHERE active=1", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub fn delete_profile(&mut self, id: i64, confirmed: bool) -> Result<()> {
        if !confirmed {
            return Err("Explicit profile deletion confirmation required".into());
        }
        let count: i64 = self
            .db
            .query_row("SELECT count(*) FROM profiles", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if count <= 1 {
            return Err("Create/select another profile before deleting the last profile".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM profiles WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        let next: Option<i64> = tx
            .query_row(
                "SELECT id FROM profiles ORDER BY active DESC,id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(next) = next {
            tx.execute("UPDATE profiles SET active=1 WHERE id=?1", [next])
                .map_err(|e| e.to_string())?;
        }
        tx.execute("DELETE FROM documents WHERE id NOT IN(SELECT document_id FROM games WHERE document_id IS NOT NULL)",[]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        if next.is_none() {
            self.select_profile("Antonius-Jairus-R")?;
        }
        Ok(())
    }
    pub fn integrity(&self) -> Result<String> {
        let mut statement = self
            .db
            .prepare("PRAGMA integrity_check")
            .map_err(|e| e.to_string())?;
        let rows: Vec<String> = statement
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;
        if rows != ["ok"] {
            return Err(format!("Integrity check: {}", rows.join("; ")));
        }
        let violations: i64 = self
            .db
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        if violations != 0 {
            return Err("Foreign-key integrity check failed".into());
        }
        Ok("ok".into())
    }
    pub fn size(&self) -> Result<u64> {
        let mut bytes = 0;
        for path in [
            self.path.clone(),
            sidecar(&self.path, "-wal"),
            sidecar(&self.path, "-shm"),
        ] {
            match std::fs::metadata(path) {
                Ok(m) => bytes += m.len(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(bytes)
    }
    /// Always creates a new file. Existing backups are never overwritten.
    pub fn backup(&self, path: &Path) -> Result<()> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("Backup destination (must be new): {e}"))?;
        drop(file);
        let mut destination = Connection::open(path).map_err(|e| e.to_string())?;
        let backup =
            rusqlite::backup::Backup::new(&self.db, &mut destination).map_err(|e| e.to_string())?;
        copy_database(&backup)
            .map_err(|e| format!("Backup failed; partial destination retained: {e}"))
    }
    /// Validates before replacement and requires a fresh safety backup of the live DB.
    pub fn restore(&mut self, source: &Path, safety: &Path, confirmed: bool) -> Result<()> {
        if !confirmed {
            return Err("Explicit restore confirmation required".into());
        }
        if std::fs::canonicalize(source).ok() == std::fs::canonicalize(&self.path).ok() {
            return Err("Restore source must differ from the active database".into());
        }
        let source_db =
            Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| e.to_string())?;
        let probe = Self {
            db: source_db,
            path: source.to_owned(),
        };
        probe.integrity()?;
        let version: i64 = probe
            .db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if !(1..=schema::VERSION).contains(&version) {
            return Err("Restore requires the current Mfalmion schema".into());
        }
        let application: i64 = probe
            .db
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if application != 0x4d46414c {
            return Err("Not a Mfalmion backup".into());
        }
        probe.profiles()?;
        for table in [
            "schema_migrations",
            "settings",
            "documents",
            "games",
            "game_moves",
            "analysis",
            "jobs",
        ] {
            // Table names are a fixed internal whitelist, never user input.
            probe
                .db
                .prepare(&format!("SELECT * FROM {table} LIMIT 0"))
                .map_err(|e| e.to_string())?;
        }
        if version >= 2 {
            for table in [
                "remote_games",
                "archive_sync",
                "sync_http",
                "sync_runs",
                "sync_failures",
            ] {
                probe
                    .db
                    .prepare(&format!("SELECT * FROM {table} LIMIT 0"))
                    .map_err(|e| e.to_string())?;
            }
        }
        self.backup(safety)?;
        let backup =
            rusqlite::backup::Backup::new(&probe.db, &mut self.db).map_err(|e| e.to_string())?;
        copy_database(&backup)?;
        drop(backup);
        migrate_sync(&mut self.db, version)?;
        self.recover_sync()?;
        self.recover_jobs()?;
        self.integrity()?;
        Ok(())
    }
}

fn copy_database(backup: &rusqlite::backup::Backup<'_, '_>) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(
                "Database copy exceeded 30-second deadline; original transaction retained".into(),
            );
        }
        match backup.step(128).map_err(|e| e.to_string())? {
            rusqlite::backup::StepResult::Done => return Ok(()),
            rusqlite::backup::StepResult::More => {}
            rusqlite::backup::StepResult::Busy | rusqlite::backup::StepResult::Locked => {
                return Err("Database copy is locked; retry after other writers finish".into());
            }
            _ => return Err("Unknown SQLite backup status".into()),
        }
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    name.into()
}

fn migrate_sync(db: &mut Connection, version: i64) -> Result<()> {
    if version < 2 {
        let tx = db.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(sync::MIGRATION)
            .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO schema_migrations(version) VALUES(?1)", [2])
            .map_err(|e| e.to_string())?;
        tx.pragma_update(None, "user_version", 2)
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
pub fn schema_for_test() -> &'static str {
    schema::SQL
}
