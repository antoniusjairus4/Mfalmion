//! Read-only presentation queries. Never updates analysis, configuration or evidence.
use super::*;
use crate::analysis::{Line, Summary, VERSION};
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewKey {
    pub profile: i64,
    pub game: i64,
    pub fingerprint: String,
    pub ply: usize,
}
#[derive(Clone, Debug, Default)]
pub struct ReviewData {
    pub key: Option<ReviewKey>,
    pub summary: Option<Arc<Summary>>,
    pub evaluation: Option<Line>,
    pub status: String,
}
impl ReviewData {
    pub fn matches(&self, identity: Option<&(i64, i64, String)>) -> bool {
        self.key
            .as_ref()
            .zip(identity)
            .is_some_and(|(k, (p, g, f))| k.profile == *p && k.game == *g && k.fingerprint == *f)
    }
    pub fn move_at(&self, ply: usize) -> Option<&crate::analysis::MoveResult> {
        self.summary
            .as_ref()?
            .moves
            .get(ply.checked_sub(1)?)
            .filter(|m| m.ply == ply)
    }
}
impl Store {
    pub fn review_data(&self, key: &ReviewKey) -> Result<ReviewData> {
        let mut data = ReviewData {
            key: Some(key.clone()),
            status: "Not analyzed · no compatible completed saved review".into(),
            ..Default::default()
        };
        let record = self.load_game(key.profile, key.game)?;
        if games::fingerprint(&record) != key.fingerprint {
            data.status = "Stored game changed; reopen it from Game Library".into();
            return Ok(data);
        }
        if key.ply >= record.positions.len() {
            return Err("Review position is outside the game timeline".into());
        }
        let attempt: Option<String> = self
            .db
            .query_row(
                "SELECT status FROM batch_runs WHERE game_id=?1 ORDER BY id DESC LIMIT 1",
                [key.game],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        // Saved review selection deliberately does not depend on today's batch preferences.
        let row:Option<(i64,String)>=self.db.query_row("SELECT b.id,j.configuration FROM batch_runs b JOIN batch_jobs j ON j.job_id=b.job_id WHERE b.game_id=?1 AND b.algorithm=?2 AND b.status='completed' ORDER BY b.id DESC LIMIT 1",params![key.game,VERSION],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
        let Some((run, json)) = row else {
            if let Some(attempt) = attempt {
                data.status = if attempt == "completed" {
                    "Not analyzed with a supported algorithm · saved review is incompatible".into()
                } else {
                    format!("Analysis incomplete ({attempt}) · no completed compatible review")
                };
            }
            return Ok(data);
        };
        let config: crate::analysis::Config =
            serde_json::from_str(&json).map_err(|e| e.to_string())?;
        config.validate()?;
        let summary = self
            .completed_review(key.profile, key.game, &config)?
            .ok_or("Completed review disappeared; refresh the game")?;
        let depth = key
            .ply
            .checked_sub(1)
            .and_then(|i| summary.moves.get(i))
            .map_or(config.engine.depth, |m| m.depth);
        // Terminal roots have depth zero in the model but are stored under the requested budget.
        let depth = if depth == 0 {
            config.engine.depth
        } else {
            depth
        };
        data.evaluation = self
            .load_batch_position(run, &record, key.ply, depth, &config, false)?
            .and_then(|e| e.lines.first().cloned());
        data.summary = Some(Arc::new(summary));
        data.status = "Completed saved batch review".into();
        if let Some(attempt) = attempt
            && attempt != "completed"
        {
            data.status.push_str(&format!(
                " · latest attempt {attempt}; showing prior completed evidence"
            ));
        }
        Ok(data)
    }
}
