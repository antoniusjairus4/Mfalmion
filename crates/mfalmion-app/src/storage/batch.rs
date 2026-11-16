//! Additive history-aware analysis storage. Older FEN-only cache is never consulted.
use super::*;
use crate::analysis::{Config, Evaluation, Summary, VERSION};
use crate::notation::GameRecord;
pub const MIGRATION: &str = r#"
CREATE TABLE batch_jobs(job_id INTEGER PRIMARY KEY REFERENCES jobs(id) ON DELETE CASCADE, configuration TEXT NOT NULL, config_key TEXT NOT NULL, reuse INTEGER NOT NULL CHECK(reuse IN(0,1)));
CREATE TABLE batch_runs(id INTEGER PRIMARY KEY, job_id INTEGER NOT NULL REFERENCES batch_jobs(job_id) ON DELETE CASCADE, game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE, config_key TEXT NOT NULL, algorithm TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN('pending','running','paused','completed','failed','cancelled')), summary TEXT, last_error TEXT, updated_at INTEGER NOT NULL DEFAULT(unixepoch()), UNIQUE(job_id,game_id));
CREATE INDEX batch_game ON batch_runs(game_id,config_key,status,id);
CREATE TABLE batch_positions(run_id INTEGER NOT NULL REFERENCES batch_runs(id) ON DELETE CASCADE, ply INTEGER NOT NULL CHECK(ply>=0), depth INTEGER NOT NULL, context TEXT NOT NULL, evaluation TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT(unixepoch()), PRIMARY KEY(run_id,ply,depth));
CREATE TABLE batch_moves(run_id INTEGER NOT NULL REFERENCES batch_runs(id) ON DELETE CASCADE, ply INTEGER NOT NULL CHECK(ply>0), result TEXT NOT NULL, PRIMARY KEY(run_id,ply));
"#;
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    pub total: u32,
    pub analyzed: u32,
    pub failed: u32,
    pub pending: u32,
}
#[derive(Clone, Debug)]
pub struct Pending {
    pub run: i64,
    pub game: i64,
}
#[derive(Clone, Copy, Debug)]
pub enum Selection {
    Unanalyzed,
    Selected(i64),
    Reanalyze(i64),
    RetryFailed,
}
impl Store {
    pub fn batch_config(&self) -> Result<Config> {
        let v: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='batch_analysis'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        match v {
            Some(v) => {
                let c: Config = serde_json::from_str(&v)
                    .map_err(|e| format!("Invalid saved batch settings: {e}"))?;
                c.validate()?;
                Ok(c)
            }
            None => Ok(Config::default()),
        }
    }
    pub fn save_batch_config(&self, c: &Config) -> Result<()> {
        c.validate()?;
        self.db.execute("INSERT INTO settings(key,value) VALUES('batch_analysis',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=unixepoch()",[serde_json::to_string(c).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn batch_coverage(&self, profile: i64, c: &Config) -> Result<Coverage> {
        let (total,analyzed,failed):(u32,u32,u32)=self.db.query_row("SELECT count(*),coalesce(sum(EXISTS(SELECT 1 FROM batch_runs b WHERE b.game_id=g.id AND b.config_key=?2 AND b.algorithm=?3 AND b.status='completed')),0),coalesce(sum(NOT EXISTS(SELECT 1 FROM batch_runs b WHERE b.game_id=g.id AND b.config_key=?2 AND b.status='completed') AND EXISTS(SELECT 1 FROM batch_runs b WHERE b.game_id=g.id AND b.config_key=?2 AND b.status='failed')),0) FROM games g WHERE profile_id=?1",params![profile,c.key(),VERSION],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        Ok(Coverage {
            total,
            analyzed,
            failed,
            pending: total.saturating_sub(analyzed + failed),
        })
    }
    pub fn start_batch(&mut self, profile: i64, c: &Config, selection: Selection) -> Result<i64> {
        c.validate()?;
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM jobs WHERE profile_id=?1 AND kind='batch-analysis' AND status IN('pending','running','paused'))",[profile],|r|r.get(0)).map_err(|e|e.to_string())?;
        if active {
            return Err("Resume or cancel the existing account-analysis job first".into());
        }
        tx.execute("INSERT INTO jobs(profile_id,kind,work_key,status,total) VALUES(?1,'batch-analysis','account-analysis-v1','pending',0)",[profile]).map_err(|e|e.to_string())?;
        let job = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO batch_jobs(job_id,configuration,config_key,reuse) VALUES(?1,?2,?3,?4)",
            params![
                job,
                serde_json::to_string(c).map_err(|e| e.to_string())?,
                c.key(),
                !matches!(selection, Selection::Reanalyze(_))
            ],
        )
        .map_err(|e| e.to_string())?;
        let (id, reanalyze, retry) = match selection {
            Selection::Unanalyzed => (None, false, false),
            Selection::Selected(id) => (Some(id), false, false),
            Selection::Reanalyze(id) => (Some(id), true, false),
            Selection::RetryFailed => (None, true, true),
        };
        let count=tx.execute("INSERT INTO batch_runs(job_id,game_id,config_key,algorithm,status) SELECT ?1,g.id,?3,?4,'pending' FROM games g WHERE g.profile_id=?2 AND (?5 IS NULL OR g.id=?5) AND (?6 OR NOT EXISTS(SELECT 1 FROM batch_runs b WHERE b.game_id=g.id AND b.config_key=?3 AND b.status='completed')) AND (NOT ?7 OR EXISTS(SELECT 1 FROM batch_runs b WHERE b.game_id=g.id AND b.config_key=?3 AND b.status='failed' AND b.id=(SELECT max(b2.id) FROM batch_runs b2 WHERE b2.game_id=g.id AND b2.config_key=?3 AND b2.status IN('completed','failed'))))",params![job,profile,c.key(),VERSION,id,reanalyze,retry]).map_err(|e|e.to_string())?;
        if retry {
            tx.execute("UPDATE jobs SET retries=1+coalesce((SELECT max(j.retries) FROM jobs j JOIN batch_jobs b ON b.job_id=j.id WHERE j.profile_id=?2 AND b.config_key=?3 AND j.id!=?1),0) WHERE id=?1",params![job,profile,c.key()]).map_err(|e|e.to_string())?;
        }
        if count == 0 {
            return Err("No eligible games for this selection/configuration".into());
        }
        tx.execute(
            "UPDATE jobs SET total=?2 WHERE id=?1",
            params![job, count as i64],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO settings(key,value) VALUES('batch_analysis',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=unixepoch()",[serde_json::to_string(c).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(job)
    }
    pub fn resumable_batch(&self, profile: i64) -> Result<Option<(i64, Config)>> {
        let row:Option<(i64,String)>=self.db.query_row("SELECT j.id,b.configuration FROM jobs j JOIN batch_jobs b ON b.job_id=j.id WHERE j.profile_id=?1 AND j.status IN('pending','running','paused') ORDER BY j.id DESC LIMIT 1",[profile],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
        row.map(|(id, v)| {
            let c: Config = serde_json::from_str(&v).map_err(|e| e.to_string())?;
            c.validate()?;
            Ok((id, c))
        })
        .transpose()
    }
    pub fn batch_reuse(&self, job: i64) -> Result<bool> {
        self.db
            .query_row("SELECT reuse FROM batch_jobs WHERE job_id=?1", [job], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())
    }
    pub fn next_batch_game(&self, job: i64) -> Result<Option<Pending>> {
        self.db.query_row("SELECT id,game_id FROM batch_runs WHERE job_id=?1 AND status IN('pending','paused','running') ORDER BY id LIMIT 1",[job],|r|Ok(Pending{run:r.get(0)?,game:r.get(1)?})).optional().map_err(|e|e.to_string())
    }
    pub fn mark_batch_run(&self, run: i64, status: &str, error: Option<&str>) -> Result<()> {
        if !["running", "paused", "failed", "cancelled"].contains(&status) {
            return Err("Invalid batch run state".into());
        }
        self.db.execute("UPDATE batch_runs SET status=?2,last_error=?3,updated_at=unixepoch() WHERE id=?1 AND status!='completed'",params![run,status,error]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn load_batch_position(
        &self,
        run: i64,
        record: &GameRecord,
        ply: usize,
        depth: u16,
        c: &Config,
        reuse: bool,
    ) -> Result<Option<Evaluation>> {
        let context = crate::analysis::context(record, ply)?;
        let row:Option<String>=self.db.query_row("SELECT p.evaluation FROM batch_positions p JOIN batch_runs b ON b.id=p.run_id WHERE (p.run_id=?1 OR (?5 AND b.game_id=(SELECT game_id FROM batch_runs WHERE id=?1))) AND b.config_key=?4 AND p.ply=?2 AND p.depth=?3 AND p.context=?6 ORDER BY (p.run_id=?1) DESC,b.id DESC LIMIT 1",params![run,ply as i64,depth,c.key(),reuse,context],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        row.map(|s| {
            let e: Evaluation =
                serde_json::from_str(&s).map_err(|e| format!("Corrupt batch evaluation: {e}"))?;
            e.validate(record, ply, depth, c.engine.multipv)?;
            Ok(e)
        })
        .transpose()
    }
    pub fn save_batch_position(
        &self,
        run: i64,
        record: &GameRecord,
        ply: usize,
        depth: u16,
        c: &Config,
        e: &Evaluation,
    ) -> Result<()> {
        e.validate(record, ply, depth, c.engine.multipv)?;
        if self.size()? > 512 * 1024 * 1024 {
            return Err(
                "Database and WAL exceed the 512 MiB batch safety limit; no data was deleted"
                    .into(),
            );
        }
        let config: Option<String> = self
            .db
            .query_row(
                "SELECT config_key FROM batch_runs WHERE id=?1 AND status='running'",
                [run],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if config.as_deref() != Some(&c.key()) {
            return Err("Run/configuration no longer active".into());
        }
        self.db.execute("INSERT INTO batch_positions(run_id,ply,depth,context,evaluation) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(run_id,ply,depth) DO NOTHING",params![run,ply as i64,depth,crate::analysis::context(record,ply)?,serde_json::to_string(e).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn finish_batch_game(
        &mut self,
        run: i64,
        record: &GameRecord,
        summary: &Summary,
    ) -> Result<()> {
        summary.validate(record)?;
        if summary.moves.len() != record.moves.len()
            || summary
                .moves
                .iter()
                .enumerate()
                .any(|(i, m)| m.ply != i + 1 || m.san != record.moves[i])
        {
            return Err("Incomplete move results".into());
        }
        for ply in 0..record.positions.len() {
            if self
                .load_batch_position(
                    run,
                    record,
                    ply,
                    summary.config.engine.depth,
                    &summary.config,
                    false,
                )?
                .is_none()
            {
                return Err("Missing checkpoint; game cannot be completed".into());
            }
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        for m in &summary.moves {
            tx.execute(
                "INSERT INTO batch_moves(run_id,ply,result) VALUES(?1,?2,?3)",
                params![
                    run,
                    m.ply as i64,
                    serde_json::to_string(m).map_err(|e| e.to_string())?
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        let count=tx.execute("UPDATE batch_runs SET status='completed',summary=?2,last_error=NULL,updated_at=unixepoch() WHERE id=?1 AND status='running' AND config_key=?3",params![run,serde_json::to_string(summary).map_err(|e|e.to_string())?,summary.config.key()]).map_err(|e|e.to_string())?;
        if count != 1 {
            return Err("Run changed before completion".into());
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn completed_review(&self, profile: i64, game: i64, c: &Config) -> Result<Option<Summary>> {
        let text:Option<String>=self.db.query_row("SELECT b.summary FROM batch_runs b JOIN games g ON g.id=b.game_id WHERE g.profile_id=?1 AND g.id=?2 AND b.config_key=?3 AND b.algorithm=?4 AND b.status='completed' ORDER BY b.id DESC LIMIT 1",params![profile,game,c.key(),VERSION],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        text.map(|t| {
            let s: Summary =
                serde_json::from_str(&t).map_err(|e| format!("Invalid completed analysis: {e}"))?;
            s.validate(&self.load_game(profile, game)?)?;
            if s.config != *c {
                return Err("Analysis configuration mismatch".into());
            }
            Ok(s)
        })
        .transpose()
    }
    pub fn batch_progress(&mut self, job: i64) -> Result<(u32, u32)> {
        let (done,failed,total):(u32,u32,u32)=self.db.query_row("SELECT coalesce(sum(status IN('completed','failed')),0),coalesce(sum(status='failed'),0),count(*) FROM batch_runs WHERE job_id=?1",[job],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        self.db
            .execute(
                "UPDATE jobs SET completed=?2,total=?3 WHERE id=?1",
                params![job, done, total],
            )
            .map_err(|e| e.to_string())?;
        Ok((done, failed))
    }
    pub fn stop_batch(&mut self, job: i64, cancel: bool) -> Result<()> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute("UPDATE batch_runs SET status=?2 WHERE job_id=?1 AND status IN('running','pending','paused')",params![job,if cancel{"cancelled"}else{"paused"}]).map_err(|e|e.to_string())?;
        tx.execute(
            "UPDATE jobs SET status=?2 WHERE id=?1 AND status IN('running','pending','paused')",
            params![job, if cancel { "cancelled" } else { "paused" }],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Streaming compatible summaries only; no account-sized position/evaluation allocation.
    pub fn account_statistics(&self, profile: i64, c: &Config) -> Result<AccountStatistics> {
        let mut statement=self.db.prepare("SELECT b.summary,g.white,g.black,p.username,g.id FROM batch_runs b JOIN games g ON g.id=b.game_id JOIN profiles p ON p.id=g.profile_id WHERE p.id=?1 AND b.config_key=?2 AND b.status='completed' AND b.algorithm=?3 AND b.id=(SELECT max(b2.id) FROM batch_runs b2 WHERE b2.game_id=g.id AND b2.config_key=?2 AND b2.status='completed')").map_err(|e|e.to_string())?;
        let mut rows = statement
            .query(params![profile, c.key(), VERSION])
            .map_err(|e| e.to_string())?;
        let mut stats = AccountStatistics::default();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let s: Summary =
                serde_json::from_str(&row.get::<_, String>(0).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let game: i64 = row.get(4).map_err(|e| e.to_string())?;
            s.validate(&self.load_game(profile, game)?)?;
            if s.config != *c {
                return Err("Invalid account analysis configuration".into());
            }
            let white: String = row.get(1).map_err(|e| e.to_string())?;
            let black: String = row.get(2).map_err(|e| e.to_string())?;
            let user: String = row.get(3).map_err(|e| e.to_string())?;
            stats.games += 1;
            for (side, name) in [(true, white), (false, black)] {
                if !name.eq_ignore_ascii_case(&user) {
                    continue;
                }
                let accuracy = if side {
                    s.accuracy.white
                } else {
                    s.accuracy.black
                };
                if let Some(accuracy) = accuracy {
                    stats.game_sum += accuracy;
                    stats.game_samples += 1;
                }
                for m in s.moves.iter().filter(|m| m.white == side) {
                    let category = m.classification.label().to_owned();
                    *stats.classifications.entry(category).or_default() += 1;
                    stats.total_moves += 1;
                    if let Some(loss) = m.loss {
                        let value = 100.0 * (-5.0 * loss).exp();
                        stats.sum += value;
                        stats.eligible += 1;
                        let phase = stats.phases.entry(m.phase.label().into()).or_default();
                        phase.0 += value;
                        phase.1 += 1;
                        let color = usize::from(!side);
                        stats.colors[color].0 += value;
                        stats.colors[color].1 += 1;
                    }
                }
            }
        }
        Ok(stats)
    }
}
#[derive(Default, Clone, Debug)]
pub struct AccountStatistics {
    pub games: u32,
    pub sum: f64,
    pub game_sum: f64,
    pub game_samples: u32,
    pub eligible: u32,
    pub total_moves: u32,
    pub classifications: std::collections::BTreeMap<String, u32>,
    pub phases: std::collections::BTreeMap<String, (f64, u32)>,
    pub colors: [(f64, u32); 2],
}

impl AccountStatistics {
    pub fn average_accuracy(&self) -> Option<f64> {
        (self.game_samples > 0).then(|| self.game_sum / f64::from(self.game_samples))
    }
    pub fn classification_rates(&self) -> std::collections::BTreeMap<String, f64> {
        self.classifications
            .iter()
            .map(|(k, n)| {
                (
                    k.clone(),
                    if self.total_moves > 0 {
                        f64::from(*n) / f64::from(self.total_moves)
                    } else {
                        0.0
                    },
                )
            })
            .collect()
    }
}
