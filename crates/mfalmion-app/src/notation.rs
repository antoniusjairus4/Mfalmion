//! Reusable, UI-independent game records. Positions retain counters, rights and EP.
use pgn_reader::{RawTag, Reader, SanPlus, Visitor};
use shakmaty::{CastlingMode, Chess, EnPassantMode, Move, Position, fen::Fen};
use std::{collections::BTreeMap, ops::ControlFlow, sync::Arc};

pub const MAX_PGN_BYTES: usize = 16 * 1024 * 1024;
const MAX_GAME_PLIES: usize = 4096;
const MAX_IMPORT_POSITIONS: usize = 200_000;

#[derive(Clone, Debug)]
pub struct GameRecord {
    pub headers: BTreeMap<String, String>,
    pub original_pgn: Option<Arc<str>>,
    pub positions: Vec<Chess>,
    pub moves: Vec<String>,
    pub result: String,
    pub played_moves: Vec<Move>,
    pub claimed_draw: Option<&'static str>,
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
            positions: vec![position.clone()],
            moves: vec![],
            result: position_result(&position),
            played_moves: vec![],
            claimed_draw: None,
        }
    }
    pub fn append(&mut self, mv: Move) -> Result<(), String> {
        if self.automatic_result() != "*" {
            return Err("Cannot append after an automatic game ending".into());
        }
        if self.moves.len() >= MAX_GAME_PLIES {
            return Err("Game exceeds the 4096-ply timeline limit".into());
        }
        let before = self.positions.last().expect("initial snapshot");
        let next = before
            .clone()
            .play(mv)
            .map_err(|e| format!("Illegal timeline move: {e}"))?;
        let san = SanPlus::from_move(before.clone(), mv).to_string();
        self.moves.push(san);
        self.played_moves.push(mv);
        self.result = position_result(&next);
        if self.result == "*" && self.positions.iter().filter(|p| **p == next).count() >= 4 {
            self.result = "1/2-1/2".into();
        }
        self.positions.push(next);
        Ok(())
    }
    fn automatic_result(&self) -> String {
        let position = self.positions.last().expect("initial snapshot");
        let result = position_result(position);
        if result == "*" && self.positions.iter().filter(|p| *p == position).count() >= 5 {
            "1/2-1/2".into()
        } else {
            result
        }
    }
    pub fn fork(&self, cursor: usize) -> Self {
        let mut fork = self.clone();
        fork.positions.truncate(cursor + 1);
        fork.moves.truncate(cursor);
        fork.played_moves.truncate(cursor);
        fork.claimed_draw = None;
        fork.result = fork.automatic_result();
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
    if value.len() > 512 {
        return Err("FEN exceeds the 512-byte limit".into());
    }
    let fields: Vec<_> = value.split_whitespace().collect();
    if fields.len() != 6 {
        return Err("FEN must contain all six fields.".into());
    }
    if fields[4].parse::<u32>().is_err() {
        return Err("FEN halfmove counter must fit an unsigned 32-bit integer.".into());
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
fn position_result(position: &Chess) -> String {
    let result = position.outcome().to_string();
    if result == "*" && position.halfmoves() >= 150 {
        "1/2-1/2".into()
    } else {
        result
    }
}
struct ParsedGame {
    record: GameRecord,
    terminated: bool,
}
struct Parser {
    remaining: usize,
}

impl Visitor for Parser {
    type Tags = BTreeMap<String, String>;
    type Movetext = ParsedGame;
    type Output = Result<GameRecord, String>;
    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        if self.remaining == 0 {
            return ControlFlow::Break(Err(
                "Import exceeds the 200000-position timeline limit".into()
            ));
        }
        self.remaining -= 1;
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
        let name = name.trim();
        let Ok(value) = value.decode_utf8() else {
            return ControlFlow::Break(Err("Non UTF-8 PGN value".into()));
        };
        if tags.insert(name.into(), value.into_owned()).is_some() {
            return ControlFlow::Break(Err(format!("Duplicate PGN header: {name}")));
        }
        ControlFlow::Continue(())
    }
    fn begin_movetext(&mut self, tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        if tags.get("SetUp").is_some_and(|s| s != "0" && s != "1")
            || (tags.get("SetUp").is_some_and(|s| s == "0") && tags.contains_key("FEN"))
        {
            return ControlFlow::Break(Err("Invalid or contradictory SetUp/FEN headers".into()));
        }
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
        ControlFlow::Continue(ParsedGame {
            record,
            terminated: false,
        })
    }
    fn san(&mut self, game: &mut ParsedGame, san: SanPlus) -> ControlFlow<Self::Output> {
        if game.terminated {
            return ControlFlow::Break(Err("Move appears after the PGN result marker".into()));
        }
        if self.remaining == 0 {
            return ControlFlow::Break(Err(
                "Import exceeds the 200000-position timeline limit".into()
            ));
        }
        self.remaining -= 1;
        let record = &mut game.record;
        if record.automatic_result() != "*" {
            return ControlFlow::Break(Err(format!(
                "Move {san} appears after an automatic game ending"
            )));
        }
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
                if let Err(error) = record.append(mv) {
                    return ControlFlow::Break(Err(error));
                }
                ControlFlow::Continue(())
            }
            Err(e) => ControlFlow::Break(Err(format!(
                "Illegal SAN {san} at ply {}: {e} ({}{}).",
                record.moves.len() + 1,
                record.positions.last().expect("snapshot").fullmoves(),
                if record.positions.last().expect("snapshot").turn().is_white() {
                    ". White"
                } else {
                    "... Black"
                },
            ))),
        }
    }
    fn outcome(
        &mut self,
        game: &mut ParsedGame,
        outcome: shakmaty::Outcome,
    ) -> ControlFlow<Self::Output> {
        if game.terminated {
            return ControlFlow::Break(Err("Multiple PGN result markers in one game".into()));
        }
        game.terminated = true;
        let record = &mut game.record;
        let result = outcome.to_string();
        let expected = record.automatic_result();
        if expected != "*" && result != "*" && expected != result {
            return ControlFlow::Break(Err(
                "PGN result contradicts the final chess position".into()
            ));
        }
        if let Some(header) = record.headers.get("Result")
            && header != &result
        {
            return ControlFlow::Break(Err(format!(
                "Result header is {header}, but the movetext result is {result}. Make both results agree."
            )));
        }
        record.result = result;
        ControlFlow::Continue(())
    }
    fn end_game(&mut self, game: ParsedGame) -> Self::Output {
        if !game.terminated {
            return Err("Missing PGN termination marker (*, 1-0, 0-1 or 1/2-1/2)".into());
        }
        Ok(game.record)
    }
}
pub fn parse_pgn(text: &str) -> Result<Vec<GameRecord>, String> {
    if text.len() > MAX_PGN_BYTES {
        return Err("PGN exceeds the 16 MiB import limit".into());
    }
    validate_tokens(text)?;
    let mut reader = Reader::new(text.as_bytes());
    let mut records = Vec::new();
    let mut parser = Parser {
        remaining: MAX_IMPORT_POSITIONS,
    };
    let original: Arc<str> = text.into();
    while let Some(result) = reader
        .read_game(&mut parser)
        .map_err(|e| format!("PGN syntax: {e}"))?
    {
        let mut record = result.map_err(|e| format!("Game {}: {e}", records.len() + 1))?;
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
                if bytes[i] == b'%' && i != 0 && bytes[i - 1] != b'\n' {
                    return Err("PGN escape (%) must start a line".into());
                }
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
                while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
                    i += 1;
                }
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
                let token = if let Some((number, rest)) = token.split_once('.') {
                    if number.bytes().all(|b| b.is_ascii_digit()) {
                        rest.trim_start_matches('.')
                    } else {
                        token
                    }
                } else {
                    token
                };
                let token = token.trim_end_matches(['!', '?']);
                if token.is_empty() {
                    continue;
                }
                token
                    .replace("0-0-0", "O-O-O")
                    .replace("0-0", "O-O")
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
    #[test]
    fn supplied_rapid_game_reports_ambiguous_rook_and_conflicting_result() {
        let source = include_str!("../tests/fixtures/casual-rapid-result-mismatch.pgn");
        let error = parse_pgn(source).unwrap_err();
        assert!(
            error.contains("Illegal SAN Rxd8 at ply 44: ambiguous san"),
            "{error}"
        );
        let disambiguated = source.replace("22. Qxd8 Rxd8", "22. Qxd8 Raxd8");
        let error = parse_pgn(&disambiguated).unwrap_err();
        assert!(error.contains("Result header"), "{error}");
        let corrected = disambiguated.replace("[Result \"1-0\"]", "[Result \"0-1\"]");
        let games = parse_pgn(&corrected).unwrap();
        assert_eq!(games[0].moves.len(), 98);
        assert_eq!(games[0].result, "0-1");
        assert_eq!(games[0].moves.last().unwrap(), "Rxc6");
        assert_eq!(
            parse_pgn(&games[0].pgn()).unwrap()[0].played_moves,
            games[0].played_moves
        );
    }

    #[test]
    fn audit_header_spacing_and_escape_lines_keep_metadata_and_setup() {
        let game = one(
            "% export comment\n[ White    \"Alice\"]\n[Black\t\"Bob\"]\n[ FEN    \"7k/P7/8/8/8/8/8/7K w - - 0 1\"]\n1.a8=Q+ *",
        );
        assert_eq!(game.headers["White"], "Alice");
        assert_eq!(game.headers["Black"], "Bob");
        assert_eq!(game.moves, ["a8=Q+"]);
        assert!(parse_pgn("1.e4 % ignored garbage\n e5 *").is_err());
    }

    #[test]
    fn audit_synthetic_729_game_batch_fits_import_policy() {
        let fixture = format!(
            "[White \"Synthetic White\"]\n[Black \"Synthetic Black\"]\n1.e4 {{{}}} e5 *\n\n",
            "x".repeat(16_000)
        );
        let batch = fixture.repeat(729);
        assert!(batch.len() < MAX_PGN_BYTES);
        let games = parse_pgn(&batch).unwrap();
        assert_eq!(games.len(), 729);
        assert!(games.iter().all(|game| game.positions.len() == 3));
    }

    #[test]
    fn audit_empty_games_numeric_castles_and_uci_identifiers_round_trip() {
        assert_eq!(one("*").positions.len(), 1);
        let game = one("[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1\"]\n1.0-0 0-0-0 *");
        assert_eq!(game.played_moves.len(), game.moves.len());
        assert_eq!(
            game.played_moves[0]
                .to_uci(shakmaty::CastlingMode::Standard)
                .to_string(),
            "e1g1"
        );
        assert_eq!(one(&game.pgn()).played_moves, game.played_moves);
        assert!(parse_pgn("1.e4").is_err());
    }
    #[test]
    fn audit_size_and_timeline_budget_fail_before_unbounded_allocation() {
        assert!(parse_pgn(&" ".repeat(MAX_PGN_BYTES + 1)).is_err());
        let mut reader = Reader::new("1.e4 e5 *".as_bytes());
        let error = reader
            .read_game(&mut Parser { remaining: 2 })
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.contains("200000-position"));
        assert!(parse_fen(&" ".repeat(513)).is_err());
    }
    #[test]
    fn audit_automatic_fivefold_and_seventyfive_results_are_consistent() {
        let repeated =
            "1.Nf3 Nf6 2.Ng1 Ng8 3.Nf3 Nf6 4.Ng1 Ng8 5.Nf3 Nf6 6.Ng1 Ng8 7.Nf3 Nf6 8.Ng1 Ng8";
        assert_eq!(one(&format!("{repeated} 1/2-1/2")).result, "1/2-1/2");
        assert!(parse_pgn(&format!("{repeated} 1-0")).is_err());
        assert!(parse_pgn(&format!("{repeated} 9.e4 *")).is_err());
        assert!(
            parse_pgn("[FEN \"r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 149 50\"]\n50.Ke2 Ke7 *").is_err()
        );
    }
    #[test]
    fn audit_illegal_timeline_append_is_transactional() {
        let mut game = GameRecord::default();
        let before = game.pgn();
        let illegal = Move::Normal {
            role: shakmaty::Role::Pawn,
            from: shakmaty::Square::E2,
            to: shakmaty::Square::E5,
            capture: None,
            promotion: None,
        };
        assert!(game.append(illegal).is_err());
        assert_eq!(game.pgn(), before);
        assert!(game.played_moves.is_empty());
    }

    #[test]
    fn audit_rejects_conflicting_or_premature_results() {
        for text in [
            "1.f3 e5 2.g4 Qh4# 1-0",
            "1.e4 1-0 e5 *",
            "1.e4 1-0 0-1",
            "[SetUp \"0\"]\n[FEN \"7k/P7/8/8/8/8/8/7K w - - 0 1\"]\n1.a8=Q+ *",
        ] {
            assert!(parse_pgn(text).is_err(), "accepted {text}");
        }
    }
    #[test]
    fn audit_fen_counters_must_not_saturate() {
        assert!(parse_fen("7k/8/8/8/8/8/8/KR6 w - - 4294967296 1").is_err());
    }

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
        fork.append(mv).unwrap();
        assert_eq!(fork.moves, ["e4", "e5", "Bc4"]);
        assert_eq!(original.pgn(), before);
        assert!(original.original_pgn.is_some());
    }
}
