//! Schema v2 additions and account-scoped synchronization checkpoints.
use super::*;
pub const MIGRATION: &str = r#"
ALTER TABLE profiles ADD COLUMN chesscom_id TEXT;
ALTER TABLE profiles ADD COLUMN sync_error TEXT;
ALTER TABLE profiles ADD COLUMN sync_retry_at INTEGER NOT NULL DEFAULT 0;
CREATE UNIQUE INDEX game_profile_identity ON games(profile_id,id);
CREATE TABLE remote_games(profile_id INTEGER NOT NULL, external_id TEXT NOT NULL, game_id INTEGER NOT NULL, white_rating INTEGER, black_rating INTEGER, end_time INTEGER, rated INTEGER, white_result TEXT, black_result TEXT, eco_url TEXT, opening TEXT, pgn_fingerprint TEXT NOT NULL, PRIMARY KEY(profile_id,external_id), UNIQUE(profile_id,game_id), FOREIGN KEY(profile_id,game_id) REFERENCES games(profile_id,id) ON DELETE CASCADE);
CREATE TABLE archive_sync(profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, month TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', imported INTEGER NOT NULL DEFAULT 0, skipped INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0, checked_at INTEGER, PRIMARY KEY(profile_id,month));
CREATE TABLE sync_http(profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, endpoint TEXT NOT NULL, etag TEXT, modified TEXT, fresh_until INTEGER NOT NULL DEFAULT 0, body TEXT, PRIMARY KEY(profile_id,endpoint));
CREATE TABLE sync_runs(profile_id INTEGER PRIMARY KEY REFERENCES profiles(id) ON DELETE CASCADE, status TEXT NOT NULL, discovered INTEGER NOT NULL DEFAULT 0, processed INTEGER NOT NULL DEFAULT 0, imported INTEGER NOT NULL DEFAULT 0, skipped INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0, last_error TEXT, started_at INTEGER, finished_at INTEGER);
CREATE TABLE sync_failures(profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, month TEXT NOT NULL, item TEXT NOT NULL, error TEXT NOT NULL, PRIMARY KEY(profile_id,month,item));
"#;
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Progress {
    pub profile: i64,
    pub username: String,
    pub status: String,
    pub last_success: Option<i64>,
    pub error: Option<String>,
    pub retry_at: i64,
    pub discovered: usize,
    pub processed: usize,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
}
#[derive(Default)]
pub struct HttpCache {
    pub etag: Option<String>,
    pub modified: Option<String>,
    pub fresh_until: i64,
    pub body: Option<String>,
}
impl Store {
    pub fn sync_info(&self, profile: i64) -> Result<Progress> {
        let (username,status,last_success,error,retry_at):(String,String,Option<i64>,Option<String>,i64)=self.db.query_row("SELECT username,sync_status,last_sync,substr(sync_error,1,4096),sync_retry_at FROM profiles WHERE id=?1",[profile],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
        let mut p = Progress {
            profile,
            username,
            status,
            last_success,
            error,
            retry_at,
            ..Default::default()
        };
        let counts:Option<(u32,u32,u32,u32,u32)>=self.db.query_row("SELECT discovered,processed,imported,skipped,failed FROM sync_runs WHERE profile_id=?1",[profile],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(|e|e.to_string())?;
        if let Some((d, pd, i, s, f)) = counts {
            p.discovered = d as usize;
            p.processed = pd as usize;
            p.imported = i as usize;
            p.skipped = s as usize;
            p.failed = f as usize;
        }
        Ok(p)
    }
    pub fn recover_sync(&self) -> Result<()> {
        self.db.execute("UPDATE profiles SET sync_status='interrupted',sync_error='Previous sync interrupted; checkpoints retained' WHERE sync_status='syncing'",[]).map_err(|e|e.to_string())?;
        self.db
            .execute(
                "UPDATE sync_runs SET status='interrupted' WHERE status='syncing'",
                [],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn save_sync(&mut self, p: &Progress) -> Result<()> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute("UPDATE profiles SET sync_status=?1,sync_error=?2,sync_retry_at=?3,last_sync=?4,updated_at=unixepoch() WHERE id=?5",params![p.status,p.error,p.retry_at,p.last_success,p.profile]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO sync_runs(profile_id,status,discovered,processed,imported,skipped,failed,last_error,started_at,finished_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,unixepoch(),CASE WHEN ?2='syncing' THEN NULL ELSE unixepoch() END) ON CONFLICT(profile_id) DO UPDATE SET started_at=CASE WHEN sync_runs.status!='syncing' AND excluded.status='syncing' THEN unixepoch() ELSE sync_runs.started_at END,status=excluded.status,discovered=excluded.discovered,processed=excluded.processed,imported=excluded.imported,skipped=excluded.skipped,failed=excluded.failed,last_error=excluded.last_error,finished_at=excluded.finished_at",params![p.profile,p.status,p.discovered as i64,p.processed as i64,p.imported as i64,p.skipped as i64,p.failed as i64,p.error]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn http_cache(&self, profile: i64, endpoint: &str) -> Result<HttpCache> {
        self.db.query_row("SELECT etag,modified,fresh_until,body FROM sync_http WHERE profile_id=?1 AND endpoint=?2",params![profile,endpoint],|r|Ok(HttpCache{etag:r.get(0)?,modified:r.get(1)?,fresh_until:r.get(2)?,body:r.get(3)?})).optional().map(|c|c.unwrap_or_default()).map_err(|e|e.to_string())
    }
    pub fn save_http(&self, profile: i64, endpoint: &str, cache: &HttpCache) -> Result<()> {
        self.db.execute("INSERT INTO sync_http(profile_id,endpoint,etag,modified,fresh_until,body) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(profile_id,endpoint) DO UPDATE SET etag=excluded.etag,modified=excluded.modified,fresh_until=excluded.fresh_until,body=excluded.body",params![profile,endpoint,cache.etag,cache.modified,cache.fresh_until,cache.body]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn http_cooldown(&self) -> Result<i64> {
        let value: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='chesscom_retry_at'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        value.map_or(Ok(0), |s| {
            s.parse().map_err(|_| "Invalid stored HTTP cooldown".into())
        })
    }
    pub fn cooldown(&self, until: i64) -> Result<()> {
        self.db.execute("INSERT INTO settings(key,value) VALUES('chesscom_retry_at',?1) ON CONFLICT(key) DO UPDATE SET value=CAST(max(CAST(value AS INTEGER),CAST(excluded.value AS INTEGER)) AS TEXT),updated_at=unixepoch()",[until.to_string()]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn month_done(&self, profile: i64, month: &str) -> Result<bool> {
        self.db.query_row("SELECT EXISTS(SELECT 1 FROM archive_sync WHERE profile_id=?1 AND month=?2 AND status IN('complete','complete_with_errors'))",params![profile,month],|r|r.get(0)).map_err(|e|e.to_string())
    }
    pub fn finish_month(
        &mut self,
        p: &Progress,
        month: &str,
        counts: (usize, usize, usize),
        cache: &HttpCache,
        endpoint: &str,
    ) -> Result<()> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO archive_sync(profile_id,month,status,imported,skipped,failed,checked_at) VALUES(?1,?2,?3,?4,?5,?6,unixepoch()) ON CONFLICT(profile_id,month) DO UPDATE SET status=excluded.status,imported=excluded.imported,skipped=excluded.skipped,failed=excluded.failed,checked_at=excluded.checked_at",params![p.profile,month,if counts.2==0{"complete"}else{"complete_with_errors"},counts.0 as i64,counts.1 as i64,counts.2 as i64]).map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO sync_http(profile_id,endpoint,etag,modified,fresh_until) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(profile_id,endpoint) DO UPDATE SET etag=excluded.etag,modified=excluded.modified,fresh_until=excluded.fresh_until,body=NULL",params![p.profile,endpoint,cache.etag,cache.modified,cache.fresh_until]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn sync_failure(&self, profile: i64, month: &str, item: usize, error: &str) -> Result<()> {
        let count: u32 = self
            .db
            .query_row(
                "SELECT count(*) FROM sync_failures WHERE profile_id=?1 AND month=?2",
                params![profile, month],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if count >= 100 {
            return Ok(());
        }
        let error: String = error.chars().take(2000).collect();
        self.db.execute("INSERT INTO sync_failures(profile_id,month,item,error) VALUES(?1,?2,?3,?4) ON CONFLICT(profile_id,month,item) DO UPDATE SET error=excluded.error",params![profile,month,item.to_string(),error]).map_err(|e|e.to_string())?;
        Ok(())
    }
}
impl Store {
    pub fn resolve_public(&self, profile: i64, p: &crate::sync::api::PublicProfile) -> Result<()> {
        let (name, identity): (String, Option<String>) = self
            .db
            .query_row(
                "SELECT username,chesscom_id FROM profiles WHERE id=?1",
                [profile],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        if !name.eq_ignore_ascii_case(&p.username)
            || identity
                .as_ref()
                .zip(p.player_id)
                .is_some_and(|(old, new)| *old != new.to_string())
        {
            return Err(
                "Account identity changed; select it explicitly without merging profiles".into(),
            );
        }
        self.db.execute("UPDATE profiles SET username=?1,display_name=coalesce(?2,display_name),chesscom_id=coalesce(chesscom_id,?3),updated_at=unixepoch() WHERE id=?4",params![p.username,p.name,p.player_id.map(|v|v.to_string()),profile]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn import_remote(
        &mut self,
        profile: i64,
        remote: &crate::sync::api::RemoteGame,
        record: &crate::notation::GameRecord,
    ) -> Result<bool> {
        let owner: String = self
            .db
            .query_row(
                "SELECT username FROM profiles WHERE id=?1",
                [profile],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !owner.eq_ignore_ascii_case(&remote.white.username)
            && !owner.eq_ignore_ascii_case(&remote.black.username)
        {
            return Err("Remote game does not belong to the target profile".into());
        }
        let url = crate::sync::api::game_url(&remote.url).map_err(|e| e.message)?;
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let linked: Option<i64> = tx
            .query_row(
                "SELECT game_id FROM remote_games WHERE profile_id=?1 AND external_id=?2",
                params![profile, url],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let manually_linked:Option<i64>=tx.query_row("SELECT id FROM games WHERE profile_id=?1 AND (external_id=?2 OR external_id=?3) ORDER BY id LIMIT 1",params![profile,url,url.replace("www.chess.com","chess.com")],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let hash = super::games::fingerprint(record);
        let matching: Option<i64> = tx
            .query_row(
                "SELECT id FROM games WHERE profile_id=?1 AND fingerprint=?2",
                params![profile, hash],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let mut identity = None;
        let reusable = if let Some(id) = matching {
            let assigned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM remote_games WHERE profile_id=?1 AND game_id=?2 AND external_id!=?3)",params![profile,id,url],|r|r.get(0)).map_err(|e|e.to_string())?;
            if assigned {
                identity = Some(url.as_str());
                None
            } else {
                Some(id)
            }
        } else {
            None
        };
        let (inserted, id) = if let Some(id) = linked.or(manually_linked).or(reusable) {
            (false, id)
        } else {
            super::games::insert_validated(&tx, profile, record, Some(&remote.pgn), identity)?
        };
        tx.execute("INSERT INTO remote_games(profile_id,external_id,game_id,white_rating,black_rating,end_time,rated,white_result,black_result,eco_url,opening,pgn_fingerprint) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(profile_id,external_id) DO UPDATE SET white_rating=coalesce(remote_games.white_rating,excluded.white_rating),black_rating=coalesce(remote_games.black_rating,excluded.black_rating),end_time=coalesce(remote_games.end_time,excluded.end_time),rated=coalesce(remote_games.rated,excluded.rated),eco_url=coalesce(remote_games.eco_url,excluded.eco_url),opening=coalesce(remote_games.opening,excluded.opening)",params![profile,url,id,remote.white.rating,remote.black.rating,remote.end_time,remote.rated,remote.white.result,remote.black.result,remote.eco,remote.opening,hash]).map_err(|e|e.to_string())?;
        tx.execute("UPDATE games SET external_id=CASE WHEN external_id IS NULL OR external_id='' THEN ?1 ELSE external_id END,time_class=CASE WHEN time_class='' THEN coalesce(?2,'') ELSE time_class END,time_control=CASE WHEN time_control='' THEN coalesce(?3,'') ELSE time_control END WHERE id=?4 AND profile_id=?5",params![url,remote.time_class,remote.time_control,id,profile]).map_err(|e|e.to_string())?;
        let code = remote.eco.as_deref().filter(|s| {
            s.len() == 3
                && matches!(s.as_bytes()[0], b'A'..=b'E')
                && s.as_bytes()[1..].iter().all(u8::is_ascii_digit)
        });
        tx.execute("UPDATE games SET eco=CASE WHEN eco='' THEN coalesce(?1,'') ELSE eco END,opening=CASE WHEN opening='' THEN coalesce(?2,'') ELSE opening END WHERE profile_id=?3 AND id=?4",params![code,remote.opening,profile,id]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(inserted)
    }
}
impl Store {
    pub fn retained_month_counts(&self, profile: i64, month: &str) -> Result<(usize, usize)> {
        let (saved, failed): (u32, u32) = self
            .db
            .query_row(
                "SELECT imported+skipped,failed FROM archive_sync WHERE profile_id=?1 AND month=?2",
                params![profile, month],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        Ok((saved as usize, failed as usize))
    }
    pub fn refresh_month(
        &mut self,
        profile: i64,
        month: &str,
        cache: &HttpCache,
        endpoint: &str,
    ) -> Result<()> {
        self.save_http(profile, endpoint, cache)?;
        self.db
            .execute(
                "UPDATE archive_sync SET checked_at=unixepoch() WHERE profile_id=?1 AND month=?2",
                params![profile, month],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
impl Store {
    pub fn begin_month(&mut self, profile: i64, month: &str, endpoint: &str) -> Result<()> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO archive_sync(profile_id,month,status) VALUES(?1,?2,'pending') ON CONFLICT(profile_id,month) DO UPDATE SET status='pending'",params![profile,month]).map_err(|e|e.to_string())?;
        tx.execute(
            "DELETE FROM sync_failures WHERE profile_id=?1 AND month=?2",
            params![profile, month],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM sync_http WHERE profile_id=?1 AND endpoint=?2",
            params![profile, endpoint],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
}
