use super::square::Square;
use shakmaty::Position as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    White,
    Black,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub side: Side,
    pub kind: Kind,
}

impl From<shakmaty::Color> for Side {
    fn from(color: shakmaty::Color) -> Self {
        match color {
            shakmaty::Color::White => Self::White,
            shakmaty::Color::Black => Self::Black,
        }
    }
}

impl From<shakmaty::Role> for Kind {
    fn from(piece: shakmaty::Role) -> Self {
        match piece {
            shakmaty::Role::Pawn => Self::Pawn,
            shakmaty::Role::Knight => Self::Knight,
            shakmaty::Role::Bishop => Self::Bishop,
            shakmaty::Role::Rook => Self::Rook,
            shakmaty::Role::Queen => Self::Queen,
            shakmaty::Role::King => Self::King,
        }
    }
}

impl From<Kind> for shakmaty::Role {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Pawn => Self::Pawn,
            Kind::Knight => Self::Knight,
            Kind::Bishop => Self::Bishop,
            Kind::Rook => Self::Rook,
            Kind::Queen => Self::Queen,
            Kind::King => Self::King,
        }
    }
}

/// Read-only renderer snapshot adapted from the authoritative chess-library board.
pub struct Position([Option<Piece>; 64]);

impl Default for Position {
    fn default() -> Self {
        Self::from_board(&shakmaty::Chess::default())
    }
}

impl Position {
    pub fn from_board(board: &shakmaty::Chess) -> Self {
        Self(std::array::from_fn(|index| {
            let square = shakmaty::Square::new(index as u32);
            let piece = board.board().piece_at(square)?;
            Some(Piece {
                side: piece.color.into(),
                kind: piece.role.into(),
            })
        }))
    }

    pub fn piece_at(&self, square: Square) -> Option<Piece> {
        self.0[square.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_32_starting_pieces_match_standard_position() {
        let position = Position::default();
        let back = [
            Kind::Rook,
            Kind::Knight,
            Kind::Bishop,
            Kind::Queen,
            Kind::King,
            Kind::Bishop,
            Kind::Knight,
            Kind::Rook,
        ];
        let mut count = 0;
        for rank in 0..8 {
            for file in 0..8 {
                let expected = match rank {
                    0 => Some(Piece {
                        side: Side::White,
                        kind: back[file as usize],
                    }),
                    1 => Some(Piece {
                        side: Side::White,
                        kind: Kind::Pawn,
                    }),
                    6 => Some(Piece {
                        side: Side::Black,
                        kind: Kind::Pawn,
                    }),
                    7 => Some(Piece {
                        side: Side::Black,
                        kind: back[file as usize],
                    }),
                    _ => None,
                };
                let actual = position.piece_at(Square::new(file, rank).unwrap());
                assert_eq!(actual, expected, "file={file} rank={rank}");
                count += usize::from(actual.is_some());
            }
        }
        assert_eq!(count, 32);
    }

    #[test]
    fn queens_and_kings_are_on_d_and_e_files() {
        let p = Position::default();
        for (rank, side) in [(0, Side::White), (7, Side::Black)] {
            assert_eq!(
                p.piece_at(Square::new(3, rank).unwrap()),
                Some(Piece {
                    side,
                    kind: Kind::Queen
                })
            );
            assert_eq!(
                p.piece_at(Square::new(4, rank).unwrap()),
                Some(Piece {
                    side,
                    kind: Kind::King
                })
            );
        }
    }
}
