#![allow(dead_code)] // Persistent lifecycle API reserved for an explicit future scheduler.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}
impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
    fn parse(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err("Invalid job status in database".into()),
        }
    }
}
#[derive(Debug)]
pub struct Job {
    pub id: i64,
    pub status: Status,
    pub total: u32,
    pub completed: u32,
    pub last_position: Option<u32>,
    pub retries: u32,
    pub last_error: Option<String>,
}
impl Store {
    pub fn create_job(
        &mut self,
        profile: i64,
        game: Option<i64>,
        kind: &str,
        key: &str,
        total: u32,
    ) -> Result<i64> {
        if kind.is_empty() || kind.len() > 100 || key.is_empty() || key.len() > 4096 {
            return Err("Invalid job description".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        if let Some(game) = game {
            let belongs: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM games WHERE id=?1 AND profile_id=?2)",
                    params![game, profile],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !belongs {
                return Err("Job game does not belong to selected profile".into());
            }
        }
        tx.execute("INSERT INTO jobs(profile_id,game_id,kind,work_key,status,total) VALUES(?1,?2,?3,?4,'pending',?5)",params![profile,game,kind,key,total]).map_err(|e|format!("Cannot create job (possibly duplicate active work): {e}"))?;
        let id = tx.last_insert_rowid();
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }
    pub fn job(&self, id: i64) -> Result<Job> {
        let mut job=self.db.query_row("SELECT id,status,total,completed,last_position,retries,last_error FROM jobs WHERE id=?1",[id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,u32>(2)?,r.get::<_,u32>(3)?,r.get::<_,Option<u32>>(4)?,r.get::<_,u32>(5)?,r.get::<_,Option<String>>(6)?))).map_err(|e|e.to_string())?;
        let status = Status::parse(&job.1)?;
        Ok(Job {
            id: job.0,
            status,
            total: job.2,
            completed: job.3,
            last_position: job.4,
            retries: job.5,
            last_error: job.6.take(),
        })
    }
    pub fn transition_job(&mut self, id: i64, status: Status, error: Option<&str>) -> Result<()> {
        if error.is_some_and(|e| e.len() > 4096) {
            return Err("Job error is too long".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let old: String = tx
            .query_row("SELECT status FROM jobs WHERE id=?1", [id], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let previous = Status::parse(&old)?;
        let valid = matches!(
            (previous, status),
            (
                Status::Pending | Status::Paused | Status::Failed,
                Status::Running
            ) | (
                Status::Running,
                Status::Paused | Status::Completed | Status::Failed
            ) | (
                Status::Pending | Status::Running | Status::Paused | Status::Failed,
                Status::Cancelled
            )
        );
        if !valid {
            return Err("Invalid job lifecycle transition".into());
        }
        let (done, total): (u32, u32) = tx
            .query_row("SELECT completed,total FROM jobs WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|e| e.to_string())?;
        if status == Status::Completed && done != total {
            return Err("Cannot complete unfinished work".into());
        }
        tx.execute("UPDATE jobs SET status=?1,last_error=?2,retries=retries+?3,started_at=CASE WHEN ?1='running' THEN coalesce(started_at,unixepoch()) ELSE started_at END,completed_at=CASE WHEN ?1 IN('completed','cancelled') THEN unixepoch() ELSE NULL END WHERE id=?4",params![status.name(),error,u32::from(previous==Status::Failed && status==Status::Running),id]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn progress_job(&mut self, id: i64, completed: u32, last: Option<u32>) -> Result<()> {
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let (status, total, old, old_position): (String, u32, u32, Option<u32>) = tx
            .query_row(
                "SELECT status,total,completed,last_position FROM jobs WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(|e| e.to_string())?;
        if status != "running"
            || completed < old
            || completed > total
            || last.is_some_and(|p| p >= total)
            || old_position.is_some_and(|old| last.is_none_or(|next| next < old))
        {
            return Err("Invalid or nonmonotonic job progress".into());
        }
        tx.execute(
            "UPDATE jobs SET completed=?1,last_position=?2 WHERE id=?3",
            params![completed, last, id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn recover_jobs(&self) -> Result<usize> {
        self.db.execute("UPDATE batch_runs SET status='paused',last_error='Interrupted; resume explicitly' WHERE status IN('running','pending')",[]).map_err(|e|e.to_string())?;
        self.db.execute("UPDATE jobs SET status='paused',last_error='Interrupted by shutdown; resume explicitly' WHERE status='running' OR (kind='batch-analysis' AND status='pending')",[]).map_err(|e|e.to_string())
    }
}
