//! Authoritative rules/session state. No GUI dependencies and no independent move generator.
use super::{
    Position,
    position::{Kind, Side},
    square::Square,
};
use shakmaty::{Chess, Color, KnownOutcome, Move, Outcome, Position as _};

#[derive(Debug, PartialEq, Eq)]
pub enum MoveResult {
    Played,
    NeedsPromotion,
    Illegal,
    DrawClaimed,
    NotClaimable,
}

pub struct Game {
    board: Chess,
    /// Full previous positions, including counters, rights and EP. Also the exact repetition record.
    history: Vec<Chess>,
    last_played: Option<Move>,
    claimed_draw: Option<&'static str>,
}

impl Default for Game {
    fn default() -> Self {
        Self::with_board(Chess::default())
    }
}

fn ui_square(square: shakmaty::Square) -> Square {
    Square::new(square.file() as u8, square.rank() as u8).expect("library square")
}

impl Game {
    pub fn with_board(board: Chess) -> Self {
        Self {
            board,
            history: Vec::new(),
            last_played: None,
            claimed_draw: None,
        }
    }

    pub fn last_played(&self) -> Option<Move> {
        self.last_played
    }
    pub fn claimed_draw(&self) -> Option<&'static str> {
        self.claimed_draw
    }
    pub fn restore_claim(&mut self, reason: Option<&'static str>) {
        self.claimed_draw = reason;
    }
    pub fn authoritative(&self) -> &Chess {
        &self.board
    }
    pub fn restore(positions: &[Chess], cursor: usize) -> Self {
        Self {
            board: positions[cursor].clone(),
            history: positions[..cursor].to_vec(),
            last_played: None,
            claimed_draw: None,
        }
    }

    /// Fresh, immutable display adapter. No independently mutable UI position is retained.
    pub fn position(&self) -> Position {
        Position::from_board(&self.board)
    }
    pub fn side_to_move(&self) -> Side {
        self.board.turn().into()
    }
    pub fn owns(&self, square: Square) -> bool {
        self.board
            .board()
            .color_at(shakmaty::Square::new(square.index() as u32))
            == Some(self.board.turn())
    }
    pub fn checked_king(&self) -> Option<Square> {
        self.board
            .is_check()
            .then(|| self.board.board().king_of(self.board.turn()))
            .flatten()
            .map(ui_square)
    }

    /// Chess equality is FIDE repetition equality: clocks ignored, legal EP and rights included.
    fn occurrences(&self, candidate: &Chess) -> usize {
        self.history
            .iter()
            .chain(std::iter::once(&self.board))
            .filter(|p| *p == candidate)
            .count()
    }

    fn terminal_status(&self) -> Option<&'static str> {
        // The library's outcome includes mate, stalemate and conservative material detection.
        match self.board.outcome() {
            Outcome::Known(KnownOutcome::Decisive { winner }) => {
                return Some(match winner {
                    Color::White => "Checkmate · White wins",
                    Color::Black => "Checkmate · Black wins",
                });
            }
            Outcome::Known(KnownOutcome::Draw) => {
                return Some(if self.board.is_stalemate() {
                    "Draw · Stalemate"
                } else {
                    "Draw · Insufficient material"
                });
            }
            Outcome::Unknown => {}
        }
        if let Some(reason) = self.claimed_draw {
            return Some(reason);
        }
        if self.board.halfmoves() >= 150 {
            return Some("Draw · 75-move rule");
        }
        if self.occurrences(&self.board) >= 5 {
            return Some("Draw · Fivefold repetition");
        }
        None
    }

    pub fn is_over(&self) -> bool {
        self.terminal_status().is_some()
    }
    pub fn status(&self) -> &'static str {
        self.terminal_status().unwrap_or_else(|| {
            match (self.side_to_move(), self.board.is_check()) {
                (Side::White, false) => "White to move",
                (Side::White, true) => "White to move · Check",
                (Side::Black, false) => "Black to move",
                (Side::Black, true) => "Black to move · Check",
            }
        })
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty() || self.claimed_draw.is_some()
    }
    pub fn undo(&mut self) -> bool {
        self.last_played = None;
        let claim_cleared = self.claimed_draw.take().is_some();
        if let Some(previous) = self.history.pop() {
            self.board = previous;
            true
        } else {
            claim_cleared
        }
    }

    fn claim_reason(&self, candidate: &Chess, intended: bool) -> Option<&'static str> {
        if self.occurrences(candidate) + usize::from(intended) >= 3 {
            Some("Draw · Threefold repetition claimed")
        } else if candidate.halfmoves() >= 100 {
            Some("Draw · 50-move rule claimed")
        } else {
            None
        }
    }

    pub fn can_claim_draw(&self) -> bool {
        !self.is_over() && self.claim_reason(&self.board, false).is_some()
    }
    pub fn claim_draw(&mut self) -> bool {
        if self.is_over() {
            return false;
        }
        self.claimed_draw = self.claim_reason(&self.board, false);
        self.claimed_draw.is_some()
    }

    /// Standard-chess UI castling destinations are c/g, while the library stores king/rook squares.
    fn destination(mv: Move) -> Square {
        if let Move::Castle { king, rook } = mv {
            Square::new(
                if rook.file() > king.file() { 6 } else { 2 },
                king.rank() as u8,
            )
            .expect("castling square")
        } else {
            ui_square(mv.to())
        }
    }

    fn moves_from(&self, from: Square) -> Vec<Move> {
        if self.is_over() {
            return Vec::new();
        }
        self.board
            .legal_moves()
            .into_iter()
            .filter(|mv| mv.from().is_some_and(|s| s as usize == from.index()))
            .collect()
    }
    pub fn legal_targets(&self, from: Square) -> Vec<Square> {
        let mut targets = Vec::new();
        for mv in self.moves_from(from) {
            let to = Self::destination(mv);
            if !targets.contains(&to) {
                targets.push(to);
            }
        }
        targets
    }
    pub fn capture_targets(&self, from: Square) -> Vec<Square> {
        self.moves_from(from)
            .into_iter()
            .filter(|mv| mv.is_capture())
            .map(Self::destination)
            .collect()
    }

    pub fn try_move(&mut self, from: Square, to: Square, promotion: Option<Kind>) -> MoveResult {
        self.attempt(from, to, promotion, false)
    }
    pub fn claim_with_move(
        &mut self,
        from: Square,
        to: Square,
        promotion: Option<Kind>,
    ) -> MoveResult {
        self.attempt(from, to, promotion, true)
    }
    fn attempt(
        &mut self,
        from: Square,
        to: Square,
        promotion: Option<Kind>,
        claim: bool,
    ) -> MoveResult {
        let candidates: Vec<_> = self
            .moves_from(from)
            .into_iter()
            .filter(|mv| Self::destination(*mv) == to)
            .collect();
        if promotion.is_none() && candidates.iter().any(|mv| mv.promotion().is_some()) {
            return MoveResult::NeedsPromotion;
        }
        let Some(mv) = candidates
            .into_iter()
            .find(|mv| mv.promotion() == promotion.map(Into::into))
        else {
            return MoveResult::Illegal;
        };
        let Ok(next) = self.board.clone().play(mv) else {
            return MoveResult::Illegal;
        };
        if claim {
            // Claiming with a declared legal move ends the game without executing that move.
            let Some(reason) = self.claim_reason(&next, true) else {
                return MoveResult::NotClaimable;
            };
            self.claimed_draw = Some(reason);
            MoveResult::DrawClaimed
        } else {
            self.last_played = Some(mv);
            self.history.push(std::mem::replace(&mut self.board, next));
            MoveResult::Played
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::position::Piece;

    fn sq(value: &str) -> Square {
        Square::new(value.as_bytes()[0] - b'a', value.as_bytes()[1] - b'1').unwrap()
    }
    fn play(game: &mut Game, from: &str, to: &str) {
        assert_eq!(
            game.try_move(sq(from), sq(to), None),
            MoveResult::Played,
            "{from}{to}"
        );
    }
    fn fen(value: &str) -> Game {
        Game::with_board(
            value
                .parse::<shakmaty::fen::Fen>()
                .unwrap()
                .into_position(shakmaty::CastlingMode::Standard)
                .unwrap(),
        )
    }

    #[test]
    fn turns_illegal_moves_captures_and_reset() {
        let mut game = Game::default();
        assert_eq!(game.legal_targets(sq("e2")), vec![sq("e3"), sq("e4")]);
        let original =
            shakmaty::fen::Fen::from_position(&game.board, shakmaty::EnPassantMode::Always)
                .to_string();
        for (from, to) in [("e7", "e5"), ("a1", "a4"), ("e2", "f3"), ("e2", "e5")] {
            assert_eq!(game.try_move(sq(from), sq(to), None), MoveResult::Illegal);
            assert_eq!(
                shakmaty::fen::Fen::from_position(&game.board, shakmaty::EnPassantMode::Always)
                    .to_string(),
                original
            );
        }
        play(&mut game, "e2", "e4");
        assert_eq!(game.side_to_move(), Side::Black);
        assert!(game.position().piece_at(sq("e2")).is_none());
        play(&mut game, "d7", "d5");
        play(&mut game, "e4", "d5");
        assert_eq!(
            game.position().piece_at(sq("d5")),
            Some(Piece {
                side: Side::White,
                kind: Kind::Pawn
            })
        );
        assert_eq!(game.board.board().occupied().count(), 31);
        game = Game::default();
        assert_eq!(
            shakmaty::fen::Fen::from_position(&game.board, shakmaty::EnPassantMode::Always)
                .to_string(),
            original
        );
    }

    #[test]
    fn standard_castling_maps_king_destination_and_moves_rook() {
        let mut game = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        assert!(game.legal_targets(sq("e1")).contains(&sq("g1")));
        assert!(game.legal_targets(sq("e1")).contains(&sq("c1")));
        assert!(!game.legal_targets(sq("e1")).contains(&sq("h1")));
        play(&mut game, "e1", "g1");
        assert_eq!(game.position().piece_at(sq("g1")).unwrap().kind, Kind::King);
        assert_eq!(game.position().piece_at(sq("f1")).unwrap().kind, Kind::Rook);
        assert!(game.position().piece_at(sq("h1")).is_none());
        play(&mut game, "e8", "c8");
        assert_eq!(game.position().piece_at(sq("c8")).unwrap().kind, Kind::King);
        assert_eq!(game.position().piece_at(sq("d8")).unwrap().kind, Kind::Rook);
    }

    #[test]
    fn pinned_pieces_king_safety_and_castling_through_check() {
        let mut pin = fen("4r1k1/8/8/8/8/8/4R3/4K3 w - - 0 1");
        assert_eq!(pin.try_move(sq("e2"), sq("f2"), None), MoveResult::Illegal);
        play(&mut pin, "e2", "e8");
        let mut king = fen("5rk1/8/8/8/8/8/8/4K3 w - - 0 1");
        assert_eq!(king.try_move(sq("e1"), sq("f1"), None), MoveResult::Illegal);
        let mut castle = fen("r3kr1r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        assert_eq!(
            castle.try_move(sq("e1"), sq("g1"), None),
            MoveResult::Illegal
        );
    }

    #[test]
    fn en_passant_capture_and_expiration() {
        let mut game = Game::default();
        for (from, to) in [("e2", "e4"), ("a7", "a6"), ("e4", "e5"), ("d7", "d5")] {
            play(&mut game, from, to);
        }
        assert!(game.legal_targets(sq("e5")).contains(&sq("d6")));
        let mut expired = Game::with_board(game.board.clone());
        play(&mut expired, "h2", "h3");
        play(&mut expired, "a6", "a5");
        assert_eq!(
            expired.try_move(sq("e5"), sq("d6"), None),
            MoveResult::Illegal
        );
        play(&mut game, "e5", "d6");
        assert!(game.position().piece_at(sq("d5")).is_none());
        assert_eq!(
            game.position().piece_at(sq("d6")).unwrap().side,
            Side::White
        );
        assert_eq!(game.board.board().occupied().count(), 31);
    }

    #[test]
    fn promotion_requires_choice_and_supports_all_four_pieces() {
        for kind in [Kind::Queen, Kind::Rook, Kind::Bishop, Kind::Knight] {
            let mut game = fen("7k/P7/8/8/8/8/8/7K w - - 0 1");
            let before =
                shakmaty::fen::Fen::from_position(&game.board, shakmaty::EnPassantMode::Always)
                    .to_string();
            assert_eq!(
                game.try_move(sq("a7"), sq("a8"), None),
                MoveResult::NeedsPromotion
            );
            assert_eq!(
                shakmaty::fen::Fen::from_position(&game.board, shakmaty::EnPassantMode::Always)
                    .to_string(),
                before
            );
            assert_eq!(
                game.try_move(sq("a7"), sq("a8"), Some(Kind::King)),
                MoveResult::Illegal
            );
            assert_eq!(
                game.try_move(sq("a7"), sq("a8"), Some(kind)),
                MoveResult::Played
            );
            assert_eq!(
                game.position().piece_at(sq("a8")),
                Some(Piece {
                    side: Side::White,
                    kind
                })
            );
            assert_eq!(game.side_to_move(), Side::Black);
        }
        let mut black = fen("7k/8/8/8/8/8/p7/7K b - - 0 1");
        assert_eq!(
            black.try_move(sq("a2"), sq("a1"), Some(Kind::Knight)),
            MoveResult::Played
        );
        assert_eq!(
            black.position().piece_at(sq("a1")),
            Some(Piece {
                side: Side::Black,
                kind: Kind::Knight
            })
        );
    }

    #[test]
    fn mate_and_stalemate_prevent_further_moves() {
        let mut mate = Game::default();
        for (from, to) in [("f2", "f3"), ("e7", "e5"), ("g2", "g4"), ("d8", "h4")] {
            play(&mut mate, from, to);
        }
        assert_eq!(mate.status(), "Checkmate · Black wins");
        assert!(mate.is_over());
        assert!(mate.legal_targets(sq("a2")).is_empty());
        assert_eq!(mate.try_move(sq("a2"), sq("a3"), None), MoveResult::Illegal);
        let stalemate = fen("7k/5K2/6Q1/8/8/8/8/8 b - - 0 1");
        assert_eq!(stalemate.status(), "Draw · Stalemate");
    }
    fn setup(game: &Game) -> shakmaty::Setup {
        game.board.to_setup(shakmaty::EnPassantMode::Always)
    }

    fn knight_cycle(game: &mut Game) {
        for (from, to) in [("g1", "f3"), ("g8", "f6"), ("f3", "g1"), ("f6", "g8")] {
            play(game, from, to);
        }
    }

    #[test]
    fn initial_position_has_exactly_twenty_legal_moves() {
        let game = Game::default();
        assert_eq!(game.board.legal_moves().len(), 20);
        assert_eq!(
            shakmaty::fen::Fen::from_position(&game.board, shakmaty::EnPassantMode::Always)
                .to_string(),
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        );
    }

    #[test]
    fn castling_out_of_check_into_check_and_lost_rights_are_rejected() {
        for value in [
            "4r1k1/8/8/8/8/8/8/R3K2R w KQ - 0 1",
            "4k1r1/8/8/8/8/8/8/R3K2R w KQ - 0 1",
        ] {
            let mut game = fen(value);
            assert_eq!(game.try_move(sq("e1"), sq("g1"), None), MoveResult::Illegal);
        }
        let mut king = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        for (from, to) in [("e1", "e2"), ("e8", "e7"), ("e2", "e1"), ("e7", "e8")] {
            play(&mut king, from, to);
        }
        for to in ["c1", "g1"] {
            assert_eq!(king.try_move(sq("e1"), sq(to), None), MoveResult::Illegal);
        }
        let mut rook = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        for (from, to) in [("a1", "a2"), ("h8", "h7"), ("a2", "a1"), ("h7", "h8")] {
            play(&mut rook, from, to);
        }
        assert_eq!(rook.try_move(sq("e1"), sq("c1"), None), MoveResult::Illegal);
    }

    #[test]
    fn en_passant_exposing_king_is_rejected() {
        let mut game = fen("k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 2");
        let before = setup(&game);
        assert!(!game.legal_targets(sq("e5")).contains(&sq("d6")));
        assert_eq!(game.try_move(sq("e5"), sq("d6"), None), MoveResult::Illegal);
        assert_eq!(setup(&game), before);
    }

    #[test]
    fn undo_restores_counters_castling_en_passant_and_promotion() {
        for (value, from, to, promotion) in [
            ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 17 12", "e1", "g1", None),
            ("7k/8/8/3pP3/8/8/8/7K w - d6 0 12", "e5", "d6", None),
            (
                "7k/P7/8/8/8/8/8/7K w - - 0 12",
                "a7",
                "a8",
                Some(Kind::Knight),
            ),
        ] {
            let mut game = fen(value);
            let before = setup(&game);
            assert_eq!(
                game.try_move(sq(from), sq(to), promotion),
                MoveResult::Played
            );
            assert!(game.can_undo());
            assert!(game.undo());
            assert_eq!(setup(&game), before);
            assert_eq!(game.history.len(), 0);
            assert_eq!(game.occurrences(&game.board), 1);
            assert!(!game.can_undo());
        }
        let mut game = Game::default();
        let before = setup(&game);
        play(&mut game, "e2", "e4");
        assert!(game.undo());
        assert_eq!(setup(&game), before);
    }

    #[test]
    fn check_status_and_king_square_are_from_library() {
        let mut game = Game::default();
        for (from, to) in [("e2", "e4"), ("f7", "f6"), ("d1", "h5")] {
            play(&mut game, from, to);
        }
        assert_eq!(game.status(), "Black to move · Check");
        assert_eq!(game.checked_king(), Some(sq("e8")));
    }

    #[test]
    fn repetition_claims_fivefold_and_undo_use_complete_history() {
        let mut game = Game::default();
        knight_cycle(&mut game);
        assert!(!game.can_claim_draw());
        knight_cycle(&mut game);
        assert!(game.can_claim_draw());
        assert!(!game.is_over());
        knight_cycle(&mut game);
        assert!(!game.is_over());
        knight_cycle(&mut game);
        assert_eq!(game.status(), "Draw · Fivefold repetition");
        assert!(game.is_over());
        assert!(game.undo());
        assert!(!game.is_over());
        play(&mut game, "f6", "g8");
        assert_eq!(game.status(), "Draw · Fivefold repetition");
        let mut claim = Game::default();
        knight_cycle(&mut claim);
        knight_cycle(&mut claim);
        assert!(claim.claim_draw());
        assert_eq!(claim.status(), "Draw · Threefold repetition claimed");
        assert_eq!(
            claim.try_move(sq("e2"), sq("e4"), None),
            MoveResult::Illegal
        );
        assert!(claim.undo());
        assert!(!claim.is_over());
        assert_eq!(claim.history.len(), 7);
        assert_eq!(claim.occurrences(&claim.board), 2);
    }

    #[test]
    fn claims_with_declared_moves_do_not_execute_the_move() {
        let mut game = Game::default();
        knight_cycle(&mut game);
        for (from, to) in [("g1", "f3"), ("g8", "f6"), ("f3", "g1")] {
            play(&mut game, from, to);
        }
        let before = setup(&game);
        assert_eq!(
            game.claim_with_move(sq("f6"), sq("g8"), None),
            MoveResult::DrawClaimed
        );
        assert_eq!(setup(&game), before);
        assert_eq!(game.history.len(), 7);
        let mut fifty = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 99 50");
        let before = setup(&fifty);
        assert!(!fifty.can_claim_draw());
        assert_eq!(
            fifty.claim_with_move(sq("e1"), sq("e2"), None),
            MoveResult::DrawClaimed
        );
        assert_eq!(setup(&fifty), before);
        let mut invalid = Game::default();
        let before = setup(&invalid);
        assert_eq!(
            invalid.claim_with_move(sq("e2"), sq("e4"), None),
            MoveResult::NotClaimable
        );
        assert_eq!(setup(&invalid), before);
    }

    #[test]
    fn fifty_is_claimable_seventyfive_automatic_and_mate_takes_precedence() {
        let mut fifty = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 100 51");
        assert!(fifty.can_claim_draw());
        assert!(!fifty.is_over());
        play(&mut fifty, "e1", "e2");
        assert!(!fifty.is_over());
        assert!(fifty.claim_draw());
        assert_eq!(fifty.status(), "Draw · 50-move rule claimed");
        let mut seventyfive = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 149 75");
        assert!(!seventyfive.is_over());
        play(&mut seventyfive, "e1", "e2");
        assert_eq!(seventyfive.status(), "Draw · 75-move rule");
        assert!(seventyfive.legal_targets(sq("e8")).is_empty());
        let mut mate = Game::default();
        for (from, to) in [("f2", "f3"), ("e7", "e5"), ("g2", "g4")] {
            play(&mut mate, from, to);
        }
        let mut before = setup(&mate);
        before.halfmoves = 149;
        let mut mate = Game::with_board(
            <Chess as shakmaty::FromSetup>::from_setup(before, shakmaty::CastlingMode::Standard)
                .unwrap(),
        );
        play(&mut mate, "d8", "h4");
        assert_eq!(mate.board.halfmoves(), 150);
        assert_eq!(mate.status(), "Checkmate · Black wins");
    }

    #[test]
    fn material_draw_is_conservative_and_not_a_two_knight_heuristic() {
        assert_eq!(
            fen("7k/8/8/8/8/8/8/KN6 w - - 0 1").status(),
            "Draw · Insufficient material"
        );
        assert_eq!(
            fen("7k/8/8/8/8/8/8/KB6 w - - 0 1").status(),
            "Draw · Insufficient material"
        );
        assert!(!fen("7k/8/8/8/8/8/8/KNN5 w - - 0 1").is_over());
    }

    #[test]
    fn repetition_equality_ignores_irrelevant_ep_and_clocks_but_not_live_ep_or_rights() {
        let mut game = Game::default();
        play(&mut game, "e2", "e4");
        let mut no_ep = setup(&game);
        no_ep.ep_square = None;
        no_ep.halfmoves = 7;
        let other =
            <Chess as shakmaty::FromSetup>::from_setup(no_ep, shakmaty::CastlingMode::Standard)
                .unwrap();
        assert_eq!(game.board, other);
        let ep = fen("7k/8/8/3pP3/8/8/8/7K w - d6 0 2");
        let no_ep = fen("7k/8/8/3pP3/8/8/8/7K w - - 0 2");
        assert_ne!(ep.board, no_ep.board);
        let rights = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let no_rights = fen("r3k2r/8/8/8/8/8/8/R3K2R w - - 0 1");
        assert_ne!(rights.board, no_rights.board);
    }
}
