//! Presentation-only transitions between adjacent authoritative position snapshots.
//! Canonical coordinates are in board cells (a1 center = [0.5, 0.5]), never pixels.
use super::{
    Position, Square,
    position::{Kind, Piece},
};

pub const MOVE_DURATION: f64 = 0.200;
pub const SETTLE_DURATION: f64 = 0.080;
pub const SNAP_BACK_DURATION: f64 = 0.120;

pub fn square_center(square: Square) -> [f32; 2] {
    [
        f32::from(square.file()) + 0.5,
        f32::from(square.rank()) + 0.5,
    ]
}

#[derive(Clone, Copy, Debug)]
pub struct PieceAnimation {
    pub piece: Piece,
    pub from: Square,
    pub to: Square,
    pub started_at: f64,
    pub duration: f64,
    origin: [f32; 2],
}

impl PieceAnimation {
    pub fn new(piece: Piece, from: Square, to: Square, started_at: f64, duration: f64) -> Self {
        Self {
            piece,
            from,
            to,
            started_at,
            duration,
            origin: square_center(from),
        }
    }
    pub fn progress(self, now: f64) -> f32 {
        if self.duration <= 0.0 {
            return 1.0;
        }
        ((now - self.started_at) / self.duration).clamp(0.0, 1.0) as f32
    }
    pub fn point(self, now: f64) -> [f32; 2] {
        let t = self.progress(now);
        let eased = 1.0 - (1.0 - t).powi(3);
        let destination = square_center(self.to);
        std::array::from_fn(|i| self.origin[i] + (destination[i] - self.origin[i]) * eased)
    }
}

#[derive(Default)]
pub struct AnimationState {
    tracks: Vec<PieceAnimation>,
}

impl AnimationState {
    pub fn clear(&mut self) {
        self.tracks.clear();
    }
    pub fn advance(&mut self, now: f64) {
        self.tracks.retain(|a| a.progress(now) < 1.0);
    }
    pub fn is_active(&self) -> bool {
        !self.tracks.is_empty()
    }
    pub fn tracks(&self) -> &[PieceAnimation] {
        &self.tracks
    }
    pub fn hides(&self, square: Square) -> bool {
        self.tracks.iter().any(|a| a.to == square)
    }

    /// No mouse dependency: future replay may call this with consecutive valid snapshots.
    /// Captured pieces are absent in `after`; only arriving pieces get animation tracks.
    pub fn transition(&mut self, before: &Position, after: &Position, now: f64) {
        self.clear(); // Deterministic rapid-input policy: finish the old visual transition first.
        let mut removed = [None; 64];
        for rank in 0..8 {
            for file in 0..8 {
                let square = Square::new(file, rank).expect("board bounds");
                if before.piece_at(square) != after.piece_at(square) {
                    removed[square.index()] = before.piece_at(square).map(|piece| (square, piece));
                }
            }
        }
        for rank in 0..8 {
            for file in 0..8 {
                let to = Square::new(file, rank).expect("board bounds");
                let Some(piece) = after.piece_at(to) else {
                    continue;
                };
                if before.piece_at(to) == Some(piece) {
                    continue;
                }
                let matching = removed
                    .iter()
                    .position(|entry| entry.is_some_and(|(_, old)| old == piece));
                // Promotion is the only role-changing arrival in a forward standard-chess move.
                let promotion = || {
                    removed.iter().position(|entry| {
                        entry.is_some_and(|(_, old)| {
                            old.side == piece.side
                                && old.kind == Kind::Pawn
                                && piece.kind != Kind::Pawn
                                && (to.rank() == 0 || to.rank() == 7)
                        })
                    })
                };
                if let Some(index) = matching.or_else(promotion)
                    && let Some((from, _)) = removed[index].take()
                {
                    self.tracks
                        .push(PieceAnimation::new(piece, from, to, now, MOVE_DURATION));
                }
            }
        }
    }

    pub fn settle_drop(&mut self, from: Square, to: Square, origin: [f32; 2]) {
        if let Some(track) = self
            .tracks
            .iter_mut()
            .find(|a| a.from == from && a.to == to)
        {
            track.origin = origin;
            track.duration = SETTLE_DURATION;
        }
        // A drop already centered on its destination needs no redundant movement at all.
        self.tracks.retain(|a| {
            let destination = square_center(a.to);
            a.origin
                .iter()
                .zip(destination)
                .any(|(p, q)| (p - q).abs() > 0.001)
        });
    }

    pub fn snap_back(&mut self, piece: Piece, source: Square, origin: [f32; 2], now: f64) {
        self.clear();
        let mut track = PieceAnimation::new(piece, source, source, now, SNAP_BACK_DURATION);
        track.origin = origin;
        self.tracks.push(track);
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::Orientation;
    use super::*;
    use shakmaty::{Chess, Position as _, Role, Square as S};

    fn sq(file: u8, rank: u8) -> Square {
        Square::new(file, rank).unwrap()
    }
    fn fen(value: &str) -> Chess {
        value
            .parse::<shakmaty::fen::Fen>()
            .unwrap()
            .into_position(shakmaty::CastlingMode::Standard)
            .unwrap()
    }
    fn play(position: &Chess, from: S, to: S, promotion: Option<Role>) -> Chess {
        let mv = position
            .legal_moves()
            .into_iter()
            .find(|mv| mv.from() == Some(from) && mv.to() == to && mv.promotion() == promotion)
            .unwrap();
        position.clone().play(mv).unwrap()
    }
    fn near(actual: [f32; 2], expected: [f32; 2]) {
        assert!(
            (actual[0] - expected[0]).abs() < 0.0001,
            "{actual:?} != {expected:?}"
        );
        assert!(
            (actual[1] - expected[1]).abs() < 0.0001,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn interpolation_start_midpoint_completion_and_frame_rate_independence() {
        let piece = Piece {
            side: super::super::position::Side::White,
            kind: Kind::Pawn,
        };
        let track = PieceAnimation::new(piece, sq(4, 1), sq(4, 3), 10.0, MOVE_DURATION);
        near(track.point(9.0), [4.5, 1.5]);
        near(track.point(10.0), [4.5, 1.5]);
        near(track.point(10.1), [4.5, 3.25]); // ease-out cubic: progress .5 => eased .875
        near(track.point(10.2), [4.5, 3.5]);
        near(track.point(11.0), [4.5, 3.5]);
        for time in [10.01, 10.04, 10.09, 10.13] {
            let _ = track.point(time);
        }
        near(track.point(10.1), [4.5, 3.25]); // absolute time, no accumulated per-frame steps
        let zero = PieceAnimation::new(piece, sq(4, 1), sq(4, 3), 10.0, 0.0);
        near(zero.point(10.0), [4.5, 3.5]);
        assert_eq!(zero.progress(10.0), 1.0);
    }

    #[test]
    fn source_destination_and_flipped_coordinates() {
        let before = Chess::default();
        let after = play(&before, S::E2, S::E4, None);
        let mut animation = AnimationState::default();
        animation.transition(
            &Position::from_board(&before),
            &Position::from_board(&after),
            0.0,
        );
        let track = animation.tracks()[0];
        assert_eq!(track.from, sq(4, 1));
        assert_eq!(track.to, sq(4, 3));
        near(
            Orientation::WhiteBottom.project(track.point(0.0)),
            [4.5, 6.5],
        );
        near(
            Orientation::WhiteBottom.project(track.point(0.2)),
            [4.5, 4.5],
        );
        near(
            Orientation::BlackBottom.project(track.point(0.0)),
            [3.5, 1.5],
        );
        near(
            Orientation::BlackBottom.project(track.point(0.2)),
            [3.5, 3.5],
        );
        assert!(animation.hides(sq(4, 3)));
        assert!(!animation.hides(sq(4, 1)));
        animation.advance(0.2);
        assert!(!animation.is_active());
        assert!(!animation.hides(sq(4, 3)));
    }

    #[test]
    fn castling_animates_king_and_rook_simultaneously() {
        for (rook, king_to, rook_to) in [(S::H1, sq(6, 0), sq(5, 0)), (S::A1, sq(2, 0), sq(3, 0))] {
            let before = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
            let after = play(&before, S::E1, rook, None);
            let mut animation = AnimationState::default();
            animation.transition(
                &Position::from_board(&before),
                &Position::from_board(&after),
                7.0,
            );
            assert_eq!(animation.tracks().len(), 2);
            let king = animation
                .tracks()
                .iter()
                .find(|a| a.piece.kind == Kind::King)
                .unwrap();
            let rook = animation
                .tracks()
                .iter()
                .find(|a| a.piece.kind == Kind::Rook)
                .unwrap();
            assert_eq!(king.from, sq(4, 0));
            assert_eq!(king.to, king_to);
            assert_eq!(rook.to, rook_to);
            assert_eq!(king.started_at, rook.started_at);
            assert_eq!(king.duration, rook.duration);
            assert!(animation.hides(king_to));
            assert!(animation.hides(rook_to));
        }
    }

    #[test]
    fn capture_and_en_passant_use_final_position_without_captured_ghosts() {
        for (value, from, to, captured) in [
            ("7k/8/8/3p4/4P3/8/8/7K w - - 0 1", S::E4, S::D5, S::D5),
            ("7k/8/8/3pP3/8/8/8/7K w - d6 0 2", S::E5, S::D6, S::D5),
        ] {
            let before = fen(value);
            let after = play(&before, from, to, None);
            let final_view = Position::from_board(&after);
            let mut animation = AnimationState::default();
            animation.transition(&Position::from_board(&before), &final_view, 0.0);
            assert_eq!(animation.tracks().len(), 1);
            assert_eq!(
                animation.tracks()[0].piece.side,
                super::super::position::Side::White
            );
            assert_eq!(after.board().by_color(shakmaty::Color::Black).count(), 1); // only black king remains
            let destination = Square::new(to.file() as u8, to.rank() as u8).unwrap();
            assert!(animation.hides(destination));
            if captured != to {
                assert!(after.board().piece_at(captured).is_none());
            }
        }
    }

    #[test]
    fn promotion_uses_confirmed_piece_and_does_not_mutate_positions() {
        for role in [Role::Queen, Role::Rook, Role::Bishop, Role::Knight] {
            let before = fen("7k/P7/8/8/8/8/8/7K w - - 0 1");
            let after = play(&before, S::A7, S::A8, Some(role));
            let original_before = before.to_setup(shakmaty::EnPassantMode::Always);
            let original_after = after.to_setup(shakmaty::EnPassantMode::Always);
            let mut animation = AnimationState::default();
            animation.transition(
                &Position::from_board(&before),
                &Position::from_board(&before),
                0.0,
            );
            assert!(!animation.is_active()); // chooser not yet confirmed
            animation.transition(
                &Position::from_board(&before),
                &Position::from_board(&after),
                0.0,
            );
            assert_eq!(animation.tracks().len(), 1);
            assert_eq!(animation.tracks()[0].piece.kind, Kind::from(role));
            assert_eq!(animation.tracks()[0].from, sq(0, 6));
            animation.advance(1.0);
            assert!(!animation.is_active());
            assert_eq!(
                before.to_setup(shakmaty::EnPassantMode::Always),
                original_before
            );
            assert_eq!(
                after.to_setup(shakmaty::EnPassantMode::Always),
                original_after
            );
        }
    }

    #[test]
    fn legal_drop_settles_from_pointer_and_invalid_drop_snaps_back() {
        let before = Chess::default();
        let after = play(&before, S::E2, S::E4, None);
        let mut animation = AnimationState::default();
        animation.transition(
            &Position::from_board(&before),
            &Position::from_board(&after),
            0.0,
        );
        animation.settle_drop(sq(4, 1), sq(4, 3), [4.7, 3.3]);
        assert_eq!(animation.tracks()[0].duration, SETTLE_DURATION);
        near(animation.tracks()[0].point(0.0), [4.7, 3.3]);
        near(animation.tracks()[0].point(0.08), [4.5, 3.5]);
        animation.settle_drop(sq(4, 1), sq(4, 3), [4.5, 3.5]);
        assert!(!animation.is_active());
        let piece = Position::from_board(&before).piece_at(sq(4, 1)).unwrap();
        animation.snap_back(piece, sq(4, 1), [-1.0, 9.0], 2.0);
        assert!(animation.hides(sq(4, 1)));
        near(animation.tracks()[0].point(2.0), [-1.0, 9.0]);
        near(animation.tracks()[0].point(2.12), [4.5, 1.5]);
        animation.advance(3.0);
        assert!(!animation.is_active());
    }
}
