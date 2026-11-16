//! History-aware account analysis, independent of egui and interactive settings.
pub mod classify;
pub mod metrics;
pub mod phase;
pub mod runner;
#[cfg(test)]
mod tests;
pub mod worker;
use crate::{engine, notation::GameRecord, storage::digest};
use serde::{Deserialize, Serialize};
use shakmaty::{CastlingMode, Position, uci::UciMove};
pub const VERSION: &str = "mfalmion-expected-score-v1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub engine: engine::Settings,
    pub identity: String,
    pub algorithm: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            engine: engine::Settings {
                depth: 12,
                ..Default::default()
            },
            identity: "Stockfish 17.1".into(),
            algorithm: VERSION.into(),
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        self.engine.validate()?;
        if self.algorithm != VERSION || self.identity.is_empty() || self.identity.len() > 256 {
            return Err("Unsupported analysis algorithm/engine identity".into());
        }
        Ok(())
    }
    pub fn key(&self) -> String {
        digest(&serde_json::to_string(self).unwrap_or_default())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Cp(i32),
    Mate { white: bool, moves: u32 },
    Draw,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub rank: usize,
    pub value: Value,
    pub bound: String,
    pub pv: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evaluation {
    pub depth: u16,
    pub lines: Vec<Line>,
    pub terminal: bool,
}
impl Evaluation {
    pub fn validate(
        &self,
        record: &GameRecord,
        ply: usize,
        depth: u16,
        multipv: usize,
    ) -> Result<(), String> {
        let position = record
            .positions
            .get(ply)
            .ok_or("Position index outside timeline")?;
        if let Some(terminal) = terminal(record, ply) {
            return if *self == terminal {
                Ok(())
            } else {
                Err("Invalid terminal evaluation".into())
            };
        }
        if self.terminal
            || self.depth != depth
            || self.lines.len() != multipv.min(position.legal_moves().len())
        {
            return Err("Incomplete depth/MultiPV group".into());
        }
        let mut roots = std::collections::BTreeSet::new();
        for (i, line) in self.lines.iter().enumerate() {
            if line.rank != i + 1
                || !["exact", "lower", "upper"].contains(&line.bound.as_str())
                || line.pv.is_empty()
                || line.pv.len() > 256
            {
                return Err("Invalid analysis line".into());
            }
            let mut p = position.clone();
            for uci in &line.pv {
                let m: UciMove = uci.parse().map_err(|_| "Invalid PV move")?;
                let m = m.to_move(&p).map_err(|_| "Illegal PV move")?;
                p.play_unchecked(m);
            }
            if !roots.insert(line.pv[0].clone()) {
                return Err("Duplicate MultiPV root".into());
            }
        }
        Ok(())
    }
}
pub fn terminal(record: &GameRecord, ply: usize) -> Option<Evaluation> {
    let p = record.positions.get(ply)?;
    let value = if p.is_checkmate() {
        Value::Mate {
            white: !p.turn().is_white(),
            moves: 0,
        }
    } else if p.is_stalemate()
        || p.is_insufficient_material()
        || p.halfmoves() >= 150
        || record.positions[..=ply].iter().filter(|q| *q == p).count() >= 5
    {
        Value::Draw
    } else {
        return None;
    };
    Some(Evaluation {
        depth: 0,
        terminal: true,
        lines: vec![Line {
            rank: 1,
            value,
            bound: "exact".into(),
            pv: vec![],
        }],
    })
}
pub fn context(record: &GameRecord, ply: usize) -> Result<String, String> {
    let initial = record
        .positions
        .first()
        .ok_or("Missing initial position")?
        .clone();
    let moves = record
        .played_moves
        .get(..ply)
        .ok_or("Invalid ply")?
        .to_vec();
    let expected = record.positions.get(ply).ok_or("Missing snapshot")?;
    Ok(digest(&format!(
        "history-v1:{}",
        engine::History { initial, moves }.command(expected)?
    )))
}
pub fn uci(record: &GameRecord, ply: usize) -> String {
    UciMove::from_move(record.played_moves[ply], CastlingMode::Standard).to_string()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MoveResult {
    pub depth: u16,
    pub ply: usize,
    pub white: bool,
    pub san: String,
    pub phase: phase::Phase,
    pub classification: classify::Category,
    pub confidence: String,
    pub tags: Vec<String>,
    pub loss: Option<f64>,
    pub centipawn_loss: Option<i64>,
    pub best: Option<String>,
    pub pv: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Accuracy {
    pub white: Option<f64>,
    pub black: Option<f64>,
    pub eligible: [usize; 2],
    pub excluded: [usize; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub config: Config,
    pub accuracy: Accuracy,
    pub counts: std::collections::BTreeMap<String, usize>,
    pub phases: std::collections::BTreeMap<String, Accuracy>,
    pub phase_counts: std::collections::BTreeMap<String, std::collections::BTreeMap<String, usize>>,
    pub moves: Vec<MoveResult>,
}
impl Summary {
    pub fn validate(&self, record: &GameRecord) -> Result<(), String> {
        self.config.validate()?;
        if self.moves.len() != record.moves.len() {
            return Err("Incomplete analysis timeline".into());
        }
        for (i, m) in self.moves.iter().enumerate() {
            if m.ply != i + 1
                || m.san != record.moves[i]
                || m.white != record.positions[i].turn().is_white()
                || m.depth > 32
                || m.loss
                    .is_some_and(|l| !l.is_finite() || !(0.0..=1.0).contains(&l))
                || (m.classification == classify::Category::Unclassified) != m.loss.is_none()
                || m.pv.len() > 256
                || m.centipawn_loss.is_some_and(|l| l < 0)
            {
                return Err("Invalid move analysis data".into());
            }
        }
        let expected = Self::new(self.config.clone(), self.moves.clone());
        let same_accuracy = |a: &Accuracy, b: &Accuracy| {
            let same = |a: Option<f64>, b: Option<f64>| match (a, b) {
                (Some(a), Some(b)) => a.is_finite() && b.is_finite() && (a - b).abs() < 1e-9,
                (None, None) => true,
                _ => false,
            };
            a.eligible == b.eligible
                && a.excluded == b.excluded
                && same(a.white, b.white)
                && same(a.black, b.black)
        };
        if self.phase_counts != expected.phase_counts
            || self.counts != expected.counts
            || !same_accuracy(&self.accuracy, &expected.accuracy)
            || self.phases.len() != expected.phases.len()
            || self
                .phases
                .iter()
                .any(|(k, v)| expected.phases.get(k).is_none_or(|e| !same_accuracy(v, e)))
        {
            return Err("Analysis aggregates do not match moves".into());
        }
        Ok(())
    }

    pub fn new(config: Config, moves: Vec<MoveResult>) -> Self {
        let mut counts = std::collections::BTreeMap::new();
        let mut phases = std::collections::BTreeMap::new();
        let mut phase_counts = std::collections::BTreeMap::new();
        for m in &moves {
            *counts.entry(m.classification.label().into()).or_default() += 1;
        }
        for phase in [
            phase::Phase::Opening,
            phase::Phase::Middlegame,
            phase::Phase::Endgame,
        ] {
            let mut by_class = std::collections::BTreeMap::new();
            for m in moves.iter().filter(|m| m.phase == phase) {
                *by_class.entry(m.classification.label().into()).or_default() += 1;
            }
            phase_counts.insert(phase.label().into(), by_class);
            phases.insert(
                phase.label().into(),
                metrics::accuracy(moves.iter().filter(|m| m.phase == phase)),
            );
        }
        let accuracy = metrics::accuracy(moves.iter());
        Self {
            config,
            accuracy,
            counts,
            phases,
            phase_counts,
            moves,
        }
    }
}
