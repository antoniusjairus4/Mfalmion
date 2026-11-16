//! UCI wire messages and immutable analysis data. Scores use White's perspective.
use shakmaty::{Chess, Color, Position, san::SanPlus, uci::UciMove};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Score {
    Cp(i32),
    Mate { winner: Color, moves: u32 },
}
impl Score {
    pub fn label(self, bound: Bound) -> String {
        let prefix = match bound {
            Bound::Exact => "",
            Bound::Lower => "≥ ",
            Bound::Upper => "≤ ",
        };
        match self {
            Self::Cp(cp) => format!("{prefix}{:+.2}", f64::from(cp) / 100.0),
            Self::Mate { winner, moves } => format!(
                "{prefix}{} mates · {moves}",
                if winner.is_white() { "White" } else { "Black" }
            ),
        }
    }
    /// Display scale only, never a win probability.
    pub fn white_fraction(self) -> f32 {
        match self {
            Self::Cp(cp) => 0.5 + 0.48 * (cp as f32 / 400.0).tanh(),
            Self::Mate { winner, .. } => {
                if winner.is_white() {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpinOption {
    pub min: u32,
    pub max: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineOption {
    Spin(SpinOption),
    Check,
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub depth: Option<u16>,
    pub seldepth: Option<u16>,
    pub multipv: usize,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub time_ms: Option<u64>,
    pub score: Option<Score>,
    pub bound: Bound,
    pub pv: Vec<String>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Message {
    UciOk,
    ReadyOk,
    Name(String),
    Option(String, EngineOption),
    Info(Info),
    BestMove(Option<String>),
    Unknown,
}
fn number<T: std::str::FromStr>(words: &[&str], index: &mut usize) -> Result<T, String> {
    *index += 1;
    words
        .get(*index)
        .ok_or("Missing UCI value")?
        .parse()
        .map_err(|_| "Invalid UCI numeric value".into())
}
pub fn parse(line: &str, turn: Color) -> Result<Message, String> {
    let line = line.trim();
    if line == "uciok" {
        return Ok(Message::UciOk);
    }
    if line == "readyok" {
        return Ok(Message::ReadyOk);
    }
    if let Some(name) = line.strip_prefix("id name ") {
        return Ok(Message::Name(name.chars().take(256).collect()));
    }
    if let Some(option) = line.strip_prefix("option name ") {
        let (name, fields) = option.split_once(" type ").ok_or("Malformed UCI option")?;
        let words: Vec<_> = fields.split_whitespace().collect();
        let value = match words.first().copied() {
            Some("spin") => {
                let value = |key| {
                    words
                        .iter()
                        .position(|w| *w == key)
                        .and_then(|n| words.get(n + 1))
                        .and_then(|s| s.parse::<u32>().ok())
                };
                let min = value("min").ok_or("Spin option missing min")?;
                let max = value("max").ok_or("Spin option missing max")?;
                if min > max {
                    return Err("Invalid spin option range".into());
                }
                EngineOption::Spin(SpinOption { min, max })
            }
            Some("check") => EngineOption::Check,
            Some(_) => EngineOption::Other,
            None => return Err("Missing UCI option type".into()),
        };
        return Ok(Message::Option(name.into(), value));
    }
    let words: Vec<_> = line.split_whitespace().collect();
    if words.first() == Some(&"bestmove") {
        let mv = words.get(1).ok_or("Missing bestmove")?;
        return Ok(Message::BestMove(if ["0000", "(none)"].contains(mv) {
            None
        } else {
            Some((*mv).into())
        }));
    }
    if words.first() != Some(&"info") || words.get(1) == Some(&"string") {
        return Ok(Message::Unknown);
    }
    let mut info = Info {
        depth: None,
        seldepth: None,
        multipv: 1,
        nodes: None,
        nps: None,
        time_ms: None,
        score: None,
        bound: Bound::Exact,
        pv: vec![],
    };
    let mut index = 1;
    while index < words.len() {
        match words[index] {
            "depth" => info.depth = Some(number(&words, &mut index)?),
            "seldepth" => info.seldepth = Some(number(&words, &mut index)?),
            "multipv" => {
                info.multipv = number(&words, &mut index)?;
                if info.multipv == 0 {
                    return Err("Invalid MultiPV index".into());
                }
            }
            "nodes" => info.nodes = Some(number(&words, &mut index)?),
            "nps" => info.nps = Some(number(&words, &mut index)?),
            "time" => info.time_ms = Some(number(&words, &mut index)?),
            "score" => {
                index += 1;
                let kind = words.get(index).ok_or("Missing score kind")?;
                let value: i32 = number(&words, &mut index)?;
                info.score = Some(match *kind {
                    "cp" => Score::Cp(if turn.is_white() {
                        value
                    } else {
                        value.checked_neg().ok_or("Score overflow")?
                    }),
                    "mate" => Score::Mate {
                        winner: if value > 0 { turn } else { !turn },
                        moves: value.unsigned_abs(),
                    },
                    _ => return Err("Unknown score kind".into()),
                });
            }
            "lowerbound" => {
                info.bound = if turn.is_white() {
                    Bound::Lower
                } else {
                    Bound::Upper
                }
            }
            "upperbound" => {
                info.bound = if turn.is_white() {
                    Bound::Upper
                } else {
                    Bound::Lower
                }
            }
            "pv" => {
                info.pv = words[index + 1..]
                    .iter()
                    .take(256)
                    .map(|s| (*s).into())
                    .collect();
                break;
            }
            "string" => break,
            _ => {}
        }
        index += 1;
    }
    Ok(Message::Info(info))
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub depth: u16,
    pub lines: Vec<Info>,
}
pub struct Snapshots {
    count: usize,
    terminal: bool,
    published: Option<u16>,
    pending: BTreeMap<u16, Vec<Option<Info>>>,
}
impl Snapshots {
    pub fn new(count: usize, terminal: bool) -> Self {
        Self {
            count: count.clamp(1, 3),
            terminal,
            published: None,
            pending: BTreeMap::new(),
        }
    }
    pub fn push(&mut self, info: Info) -> Option<Snapshot> {
        let depth = info.depth?;
        if info.score.is_none()
            || (info.pv.is_empty() && !self.terminal)
            || info.multipv > self.count
            || info.multipv == 0
            || self.published.is_some_and(|d| depth < d)
        {
            return None;
        }
        let index = info.multipv - 1;
        let rows = self
            .pending
            .entry(depth)
            .or_insert_with(|| vec![None; self.count]);
        rows[index] = Some(info);
        let snapshot = rows
            .iter()
            .cloned()
            .collect::<Option<Vec<_>>>()
            .map(|lines| Snapshot { depth, lines });
        if snapshot.is_some() {
            self.published = Some(depth);
            self.pending.retain(|d, _| *d > depth);
        }
        while self.pending.len() > 4 {
            self.pending.pop_first();
        }
        snapshot
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedPv {
    pub text: String,
    pub invalid: Option<String>,
}
pub fn decode_pv(root: &Chess, moves: &[String]) -> DecodedPv {
    let mut position = root.clone();
    let mut tokens = Vec::new();
    let mut invalid = None;
    for (index, text) in moves.iter().take(12).enumerate() {
        let mv = match text
            .parse::<UciMove>()
            .ok()
            .and_then(|uci| uci.to_move(&position).ok())
        {
            Some(mv) => mv,
            None => {
                invalid = Some(format!("Invalid PV move: {text}"));
                break;
            }
        };
        if position.turn().is_white() {
            tokens.push(format!("{}.", position.fullmoves()));
        } else if index == 0 {
            tokens.push(format!("{}...", position.fullmoves()));
        }
        tokens.push(SanPlus::from_move(position.clone(), mv).to_string());
        position.play_unchecked(mv); // to_move validated against this exact cloned position.
    }
    DecodedPv {
        text: tokens.join(" "),
        invalid,
    }
}
