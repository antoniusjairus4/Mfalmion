//! Conservative FEN-only cache: never shared between distinct game-history prefixes.
use super::*;
use crate::{
    engine::{
        Settings,
        protocol::{Bound, Score, Snapshot},
    },
    notation::{self, GameRecord},
};
use shakmaty::Color;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CachedScore {
    Cp(i32),
    Mate { white: bool, moves: u32 },
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Line {
    pub rank: usize,
    pub score: CachedScore,
    pub bound: String,
    pub pv: Vec<String>,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Configuration {
    settings: Settings,
    adapter: u32,
    search_ms: u32,
    history_mode: String,
    analyse_mode: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub game: i64,
    pub ply: usize,
    pub context: String,
    pub engine: String,
    pub configuration: String,
    pub depth: u16,
    pub multipv: usize,
}
impl Key {
    pub fn matches_settings(&self, settings: &Settings) -> bool {
        serde_json::from_str::<Configuration>(&self.configuration)
            .is_ok_and(|c| c.settings == *settings)
    }
    pub fn new(
        game: i64,
        record: &GameRecord,
        ply: usize,
        engine: &str,
        settings: &Settings,
    ) -> Result<Self> {
        settings.validate()?;
        if record
            .positions
            .first()
            .is_none_or(|p| notation::fen(p) != notation::fen(&shakmaty::Chess::default()))
        {
            return Err("Cache reuse requires a complete history from the standard starting position; custom FEN context is not reusable".into());
        }
        if engine.trim().is_empty() || engine.len() > 512 || ply >= record.positions.len() {
            return Err("Incomplete analysis identity or context".into());
        }
        let history = record.positions[..=ply]
            .iter()
            .map(notation::fen)
            .collect::<Vec<_>>()
            .join("\n");
        let context = digest(&format!("fen-only-v1; history-isolation-only;\n{history}"));
        let configuration = serde_json::to_string(&Configuration {
            settings: settings.clone(),
            adapter: 1,
            search_ms: 30_000,
            history_mode: "fen-only".into(),
            analyse_mode: "true-if-advertised".into(),
        })
        .map_err(|e| e.to_string())?;
        Ok(Self {
            game,
            ply,
            context,
            engine: engine.into(),
            configuration,
            depth: settings.depth,
            multipv: settings.multipv,
        })
    }
}
impl Store {
    fn validate_key(&self, key: &Key) -> Result<()> {
        let profile: i64 = self
            .db
            .query_row(
                "SELECT profile_id FROM games WHERE id=?1",
                [key.game],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let record = self.load_game(profile, key.game)?;
        let configuration: Configuration =
            serde_json::from_str(&key.configuration).map_err(|e| e.to_string())?;
        let settings = configuration.settings;
        let expected = Key::new(key.game, &record, key.ply, &key.engine, &settings)?;
        if expected.configuration != key.configuration
            || expected.context != key.context
            || key.depth != expected.depth
            || key.multipv != expected.multipv
        {
            return Err("Analysis context does not match stored game/settings".into());
        }
        Ok(())
    }
    pub fn cache_analysis(&self, key: &Key, snapshot: &Snapshot, completed: bool) -> Result<()> {
        self.validate_key(key)?;
        let profile: i64 = self
            .db
            .query_row(
                "SELECT profile_id FROM games WHERE id=?1",
                [key.game],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let record = self.load_game(profile, key.game)?;
        use shakmaty::Position;
        let root = &record.positions[key.ply];
        let expected = key.multipv.min(root.legal_moves().len()).max(1);
        if !completed || snapshot.depth != key.depth || snapshot.lines.len() != expected {
            return Err(
                "Only explicitly completed requested-depth MultiPV groups may be cached".into(),
            );
        }
        let mut lines = Vec::new();
        for (i, line) in snapshot.lines.iter().enumerate() {
            if line.multipv != i + 1 || line.depth != Some(key.depth) || line.pv.len() > 256 {
                return Err("Inconsistent cache MultiPV group".into());
            }
            let score = match line.score.ok_or("Missing cache score")? {
                Score::Cp(c) => CachedScore::Cp(c),
                Score::Mate { winner, moves } => CachedScore::Mate {
                    white: winner == Color::White,
                    moves,
                },
            };
            let mut position = root.clone();
            for token in &line.pv {
                let mv = token
                    .parse::<shakmaty::uci::UciMove>()
                    .map_err(|e| e.to_string())?
                    .to_move(&position)
                    .map_err(|e| e.to_string())?;
                position.play_unchecked(mv);
            }
            if line.pv.is_empty() && !root.legal_moves().is_empty() {
                return Err("Missing cache PV".into());
            }
            lines.push(Line {
                rank: i + 1,
                score,
                bound: match line.bound {
                    Bound::Exact => "exact",
                    Bound::Lower => "lower",
                    Bound::Upper => "upper",
                }
                .into(),
                pv: line.pv.clone(),
            });
        }
        let serialized = serde_json::to_string(&lines).map_err(|e| e.to_string())?;
        self.db.execute("INSERT INTO analysis(game_id,ply,context,engine,configuration,depth,multipv,lines,status) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'completed') ON CONFLICT(game_id,ply,context,engine,configuration,depth,multipv) DO UPDATE SET lines=excluded.lines,created_at=unixepoch()",params![key.game,key.ply as i64,key.context,key.engine,key.configuration,key.depth,key.multipv as i64,serialized]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn cached_analysis(&self, key: &Key) -> Result<Option<Vec<Line>>> {
        self.validate_key(key)?;
        let value:Option<String>=self.db.query_row("SELECT lines FROM analysis WHERE game_id=?1 AND ply=?2 AND context=?3 AND engine=?4 AND configuration=?5 AND depth=?6 AND multipv=?7 AND status='completed'",params![key.game,key.ply as i64,key.context,key.engine,key.configuration,key.depth,key.multipv as i64],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let lines: Option<Vec<Line>> = value
            .map(|s| {
                serde_json::from_str(&s).map_err(|e| format!("Malformed cached analysis: {e}"))
            })
            .transpose()?;
        if let Some(lines) = &lines {
            let profile: i64 = self
                .db
                .query_row(
                    "SELECT profile_id FROM games WHERE id=?1",
                    [key.game],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            let record = self.load_game(profile, key.game)?;
            use shakmaty::Position;
            let root = &record.positions[key.ply];
            if lines.len() != key.multipv.min(root.legal_moves().len()).max(1) {
                return Err("Malformed cached MultiPV count".into());
            }
            for (i, line) in lines.iter().enumerate() {
                if line.rank != i + 1
                    || !matches!(line.bound.as_str(), "exact" | "lower" | "upper")
                    || line.pv.len() > 256
                    || line.pv.is_empty() && !root.legal_moves().is_empty()
                {
                    return Err("Malformed cached PV or bound".into());
                }
                let mut position = root.clone();
                for token in &line.pv {
                    let mv = token
                        .parse::<shakmaty::uci::UciMove>()
                        .map_err(|e| e.to_string())?
                        .to_move(&position)
                        .map_err(|e| e.to_string())?;
                    position.play_unchecked(mv);
                }
            }
        }
        Ok(lines)
    }
}
