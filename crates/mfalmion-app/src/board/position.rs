use super::square::Square;

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

impl Piece {
    // Filled silhouettes for both sides; renderer supplies contrasting fill/outline.
    pub fn glyph(self) -> &'static str {
        match self.kind {
            Kind::King => "♚",
            Kind::Queen => "♛",
            Kind::Rook => "♜",
            Kind::Bishop => "♝",
            Kind::Knight => "♞",
            Kind::Pawn => "♟",
        }
    }
}

/// A display snapshot, deliberately independent of turn, legality and move history.
/// A future rules-library adapter can populate this snapshot without changing painting.
pub struct Position([Option<Piece>; 64]);

impl Default for Position {
    fn default() -> Self {
        let mut squares = [None; 64];
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
        for (file, kind) in back.into_iter().enumerate() {
            squares[file] = Some(Piece {
                side: Side::White,
                kind,
            });
            squares[8 + file] = Some(Piece {
                side: Side::White,
                kind: Kind::Pawn,
            });
            squares[48 + file] = Some(Piece {
                side: Side::Black,
                kind: Kind::Pawn,
            });
            squares[56 + file] = Some(Piece {
                side: Side::Black,
                kind,
            });
        }
        Self(squares)
    }
}

impl Position {
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
