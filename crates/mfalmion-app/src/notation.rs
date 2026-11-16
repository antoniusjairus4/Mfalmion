//! Reusable, UI-independent game records. Positions retain counters, rights and EP.
use pgn_reader::{RawTag, Reader, SanPlus, Visitor};
use shakmaty::{CastlingMode, Chess, EnPassantMode, Move, Position, fen::Fen};
use std::{collections::BTreeMap, ops::ControlFlow, sync::Arc};

#[derive(Clone, Debug)]
pub struct GameRecord {
    pub headers: BTreeMap<String, String>,
    pub original_pgn: Option<Arc<str>>,
    pub positions: Vec<Chess>,
    pub moves: Vec<String>,
    pub result: String,
}
impl Default for GameRecord {
    fn default() -> Self {
        Self::new(Chess::default())
    }
}
impl GameRecord {
    pub fn new(position: Chess) -> Self {
        Self {
            headers: BTreeMap::new(),
            original_pgn: None,
            positions: vec![position],
            moves: vec![],
            result: "*".into(),
        }
    }
    pub fn append(&mut self, mv: Move) {
        let before = self.positions.last().expect("initial snapshot");
        let san = SanPlus::from_move(before.clone(), mv).to_string();
        let mut next = before.clone();
        next.play_unchecked(mv);
        self.moves.push(san);
        self.result = next.outcome().to_string();
        self.positions.push(next);
    }
    pub fn fork(&self, cursor: usize) -> Self {
        let mut fork = self.clone();
        fork.positions.truncate(cursor + 1);
        fork.moves.truncate(cursor);
        fork.result = "*".into();
        fork.original_pgn = None;
        fork
    }
    pub fn pgn(&self) -> String {
        let mut tags = self.headers.clone();
        for name in ["Event", "Site", "Date", "Round", "White", "Black"] {
            tags.entry(name.into()).or_insert_with(|| "?".into());
        }
        tags.insert("Result".into(), self.result.clone());
        let initial = fen(&self.positions[0]);
        if initial != fen(&Chess::default()) {
            tags.insert("SetUp".into(), "1".into());
            tags.insert("FEN".into(), initial);
        } else {
            tags.remove("SetUp");
            tags.remove("FEN");
        }
        let mut out = String::new();
        let mut ordered = Vec::new();
        for name in ["Event", "Site", "Date", "Round", "White", "Black", "Result"] {
            if let Some(value) = tags.remove(name) {
                ordered.push((name.to_owned(), value));
            }
        }
        ordered.extend(tags);
        for (name, value) in ordered {
            let escaped = value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace(['\n', '\r'], " ");
            out.push_str(&format!("[{name} \"{escaped}\"]\n"));
        }
        out.push('\n');
        let mut tokens = Vec::new();
        for (index, san) in self.moves.iter().enumerate() {
            let position = &self.positions[index];
            if position.turn().is_white() {
                tokens.push(format!("{}.", position.fullmoves()));
            } else if index == 0 {
                tokens.push(format!("{}...", position.fullmoves()));
            }
            tokens.push(san.clone());
        }
        tokens.push(self.result.clone());
        let mut column = 0;
        for token in tokens {
            if column > 0 {
                if column + 1 + token.len() > 80 {
                    out.push('\n');
                    column = 0;
                } else {
                    out.push(' ');
                    column += 1;
                }
            }
            out.push_str(&token);
            column += token.len();
        }
        out.push('\n');
        out
    }
}
pub fn fen(position: &Chess) -> String {
    Fen::from_position(position, EnPassantMode::Always).to_string()
}
pub fn parse_fen(value: &str) -> Result<Chess, String> {
    let fields: Vec<_> = value.split_whitespace().collect();
    if fields.len() != 6 {
        return Err("FEN must contain all six fields.".into());
    }
    if !fields[5].parse::<u32>().is_ok_and(|counter| counter > 0) {
        return Err("FEN fullmove number must be a positive integer.".into());
    }
    value
        .parse::<Fen>()
        .map_err(|e| format!("Invalid FEN: {e}"))?
        .into_position(CastlingMode::Standard)
        .map_err(|e| format!("Illegal FEN position: {e}"))
}
struct Parser;
impl Visitor for Parser {
    type Tags = BTreeMap<String, String>;
    type Movetext = GameRecord;
    type Output = Result<GameRecord, String>;
    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(BTreeMap::new())
    }
    fn tag(
        &mut self,
        tags: &mut Self::Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        let Ok(name) = std::str::from_utf8(name) else {
            return ControlFlow::Break(Err("Non UTF-8 PGN header".into()));
        };
        let Ok(value) = value.decode_utf8() else {
            return ControlFlow::Break(Err("Non UTF-8 PGN value".into()));
        };
        if tags.insert(name.into(), value.into_owned()).is_some() {
            return ControlFlow::Break(Err(format!("Duplicate PGN header: {name}")));
        }
        ControlFlow::Continue(())
    }
    fn begin_movetext(&mut self, tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        let position = match tags.get("FEN") {
            Some(value) => match parse_fen(value) {
                Ok(p) => p,
                Err(e) => return ControlFlow::Break(Err(e)),
            },
            None if tags.get("SetUp").is_some_and(|s| s == "1") => {
                return ControlFlow::Break(Err("SetUp requires FEN".into()));
            }
            None => Chess::default(),
        };
        if tags
            .get("Variant")
            .is_some_and(|v| v != "Standard" && v != "Chess")
        {
            return ControlFlow::Break(Err("Only standard chess PGN is supported".into()));
        }
        let mut record = GameRecord::new(position);
        if let Some(result) = tags.get("Result") {
            if !["*", "1-0", "0-1", "1/2-1/2"].contains(&result.as_str()) {
                return ControlFlow::Break(Err("Invalid Result header".into()));
            }
            record.result = result.clone();
        }
        record.headers = tags;
        ControlFlow::Continue(record)
    }
    fn san(&mut self, record: &mut GameRecord, san: SanPlus) -> ControlFlow<Self::Output> {
        match san
            .san
            .to_move(record.positions.last().expect("initial snapshot"))
        {
            Ok(mv) => {
                let canonical =
                    SanPlus::from_move(record.positions.last().expect("snapshot").clone(), mv);
                if san.suffix.is_some() && san.suffix != canonical.suffix {
                    return ControlFlow::Break(Err(format!(
                        "Incorrect check/checkmate suffix: {san}"
                    )));
                }
                record.append(mv);
                ControlFlow::Continue(())
            }
            Err(e) => ControlFlow::Break(Err(format!(
                "Illegal SAN {san} at ply {}: {e}",
                record.moves.len() + 1
            ))),
        }
    }
    fn outcome(
        &mut self,
        record: &mut GameRecord,
        outcome: shakmaty::Outcome,
    ) -> ControlFlow<Self::Output> {
        let result = outcome.to_string();
        if record.headers.get("Result").is_some_and(|r| r != &result) {
            return ControlFlow::Break(Err("Result header differs from movetext result".into()));
        }
        record.result = result;
        ControlFlow::Continue(())
    }
    fn end_game(&mut self, mut record: GameRecord) -> Self::Output {
        if let Some(result) = record.headers.get("Result") {
            record.result = result.clone();
        }
        Ok(record)
    }
}
pub fn parse_pgn(text: &str) -> Result<Vec<GameRecord>, String> {
    if text.len() > 16 * 1024 * 1024 {
        return Err("PGN exceeds the 16 MiB import limit".into());
    }
    validate_tokens(text)?;
    let mut reader = Reader::new(text.as_bytes());
    let mut records = Vec::new();
    let original: Arc<str> = text.into();
    while let Some(result) = reader
        .read_game(&mut Parser)
        .map_err(|e| format!("PGN syntax: {e}"))?
    {
        let mut record = result.map_err(|e| format!("Game {}: {e}", records.len() + 1))?;
        if record.moves.is_empty() && record.headers.is_empty() {
            return Err("No PGN headers or legal moves found".into());
        }
        record.original_pgn = Some(original.clone());
        records.push(record);
    }
    if records.is_empty() {
        return Err("No games found".into());
    }
    Ok(records)
}

// pgn-reader intentionally tolerates unknown tokens. Reject malformed input first,
// so a broken SAN token cannot disappear from an otherwise legal mainline.
fn validate_tokens(text: &str) -> Result<(), String> {
    let bytes = text.trim_start_matches('\u{feff}').as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b if b.is_ascii_whitespace() => i += 1,
            b';' | b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                if i == bytes.len() {
                    return Err("Unclosed PGN comment".into());
                }
                i += 1;
            }
            b'[' => {
                i += 1;
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if start == i {
                    return Err("Invalid PGN tag name".into());
                }
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if bytes.get(i) != Some(&b'"') {
                    return Err("PGN tag value must be quoted".into());
                }
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err("Unclosed PGN tag value".into());
                }
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if bytes.get(i) != Some(&b']') {
                    return Err("Unclosed PGN tag".into());
                }
                i += 1;
            }
            b'!' | b'?' => {
                i += 1;
                if bytes.get(i).is_some_and(|b| *b == b'!' || *b == b'?') {
                    i += 1;
                }
            }
            b'$' => {
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i == start {
                    return Err("Missing numeric annotation glyph".into());
                }
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                if depth == 0 {
                    return Err("Unmatched PGN variation close".into());
                }
                depth -= 1;
                i += 1;
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && !b"{}[]();$!?".contains(&bytes[i])
                {
                    i += 1;
                }
                if i == start {
                    return Err("Unexpected PGN delimiter".into());
                }
                let token = std::str::from_utf8(&bytes[start..i]).map_err(|e| e.to_string())?;
                if ["*", "1-0", "0-1", "1/2-1/2"].contains(&token) {
                    continue;
                }
                if let Some(nag) = token.strip_prefix('$')
                    && !nag.is_empty()
                    && nag.bytes().all(|b| b.is_ascii_digit())
                {
                    continue;
                }
                let token = token
                    .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
                    .trim_end_matches(['!', '?']);
                if token.is_empty() {
                    continue;
                }
                token
                    .parse::<SanPlus>()
                    .map_err(|_| format!("Invalid PGN token: {token}"))?;
            }
        }
    }
    if depth != 0 {
        return Err("Unclosed PGN variation".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn one(text: &str) -> GameRecord {
        parse_pgn(text).unwrap().remove(0)
    }
    #[test]
    fn standard_headers_san_and_multiple_games() {
        let source = "[Event \"Example\"]\n[Site \"Offline\"]\n[Date \"2026.11.16\"]\n[Round \"1\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"*\"]\n[TimeControl \"600\"]\n[ECO \"C20\"]\n[Opening \"King Pawn\"]\n\n1.e4 e5 2.Nf3 Nc6 *\n\n[White \"Carol\"]\n1.d4 d5 *";
        let games = parse_pgn(source).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].moves, ["e4", "e5", "Nf3", "Nc6"]);
        assert_eq!(games[0].headers["TimeControl"], "600");
        assert_eq!(games[0].headers["Opening"], "King Pawn");
        assert_eq!(games[0].original_pgn.as_deref(), Some(source));
        assert_eq!(games[0].positions.len(), 5);
    }
    #[test]
    fn comments_nested_variations_and_nags_keep_mainline() {
        let record = one("1. e4 {hello} (1. d4 d5 (1... Nf6)) e5$1 2.Nf3!? ; line\n Nc6 *");
        assert_eq!(record.moves, ["e4", "e5", "Nf3", "Nc6"]);
    }
    #[test]
    fn custom_fen_castling_and_full_state_snapshots() {
        let record =
            one("[SetUp \"1\"]\n[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 17 12\"]\n12.O-O O-O-O *");
        assert_eq!(record.moves, ["O-O", "O-O-O"]);
        assert_eq!(
            fen(&record.positions[0]),
            "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 17 12"
        );
        assert_eq!(
            fen(&record.positions[2]),
            "2kr3r/8/8/8/8/8/8/R4RK1 w - - 19 13"
        );
        let game = crate::board::Game::restore(&record.positions, 0);
        assert_eq!(fen(game.authoritative()), fen(&record.positions[0]));
    }
    #[test]
    fn en_passant_and_promotion_are_legal_san() {
        let ep = one("1.e4 a6 2.e5 d5 3.exd6 *");
        assert_eq!(
            ep.positions[4].to_setup(EnPassantMode::Always).ep_square,
            Some(shakmaty::Square::D6)
        );
        assert_eq!(ep.positions[5].board().piece_at(shakmaty::Square::D5), None);
        for piece in ["Q+", "R+", "B", "N"] {
            let record = one(&format!(
                "[SetUp \"1\"]\n[FEN \"7k/P7/8/8/8/8/8/7K w - - 0 1\"]\n1.a8={piece} *"
            ));
            assert_eq!(record.positions.len(), 2);
        }
    }
    #[test]
    fn illegal_or_malformed_pgn_is_rejected() {
        for text in [
            "",
            "garbage",
            "1.e5 *",
            "1.e4 e5 2.Bh6 *",
            "1.e4 banana *",
            "1.e4 {unclosed",
            "1.e4 (1.d4 *",
            "[SetUp \"1\"]\n*",
            "[FEN \"invalid\"]\n*",
            "[Result \"1-0\"]\n1.e4 0-1",
            "[White unquoted] 1.e4 *",
        ] {
            assert!(parse_pgn(text).is_err(), "accepted {text}");
        }
    }
    #[test]
    fn fen_validation_preserves_every_field() {
        let source = "r3k2r/8/8/3pP3/8/8/8/R3K2R w KQkq d6 17 42";
        assert_eq!(fen(&parse_fen(source).unwrap()), source);
        for text in [
            "invalid",
            "8/8/8/8/8/8/8/8 w - - 0 1",
            "8/8/8/8/8/8/8/K6k w - -",
            "8/8/8/8/8/8/8/K6k w K - 0 1",
        ] {
            assert!(parse_fen(text).is_err());
        }
    }
    #[test]
    fn exports_round_trip_headers_results_and_custom_start() {
        for source in [
            "1.f3 e5 2.g4 Qh4# 0-1",
            "[SetUp \"1\"]\n[FEN \"7k/8/8/8/8/8/p7/7K b - - 0 12\"]\n12...a1=Q+ *",
            "[White \"A\\\"B\"]\n1.e4 e5 *",
        ] {
            let original = one(source);
            let exported = one(&original.pgn());
            assert_eq!(exported.moves, original.moves);
            assert_eq!(exported.result, original.result);
            for (a, b) in exported.positions.iter().zip(&original.positions) {
                assert_eq!(fen(a), fen(b));
            }
        }
    }
    #[test]
    fn forking_does_not_modify_imported_mainline() {
        let original = one("1.e4 e5 2.Nf3 Nc6 *");
        let before = original.pgn();
        let mut fork = original.fork(2);
        let mv = "Bc4"
            .parse::<SanPlus>()
            .unwrap()
            .san
            .to_move(&fork.positions[2])
            .unwrap();
        fork.append(mv);
        assert_eq!(fork.moves, ["e4", "e5", "Bc4"]);
        assert_eq!(original.pgn(), before);
        assert!(original.original_pgn.is_some());
    }
}
