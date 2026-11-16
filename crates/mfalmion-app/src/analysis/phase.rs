use shakmaty::{Chess, Color, Position, Role, Square};
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Phase {
    Opening,
    Middlegame,
    Endgame,
}
impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Opening => "Opening",
            Self::Middlegame => "Middlegame",
            Self::Endgame => "Endgame",
        }
    }
}
pub fn material(p: &Chess, white: bool) -> i32 {
    p.board()
        .iter()
        .map(|(_, piece)| {
            let v = match piece.role {
                Role::Pawn => 1,
                Role::Knight | Role::Bishop => 3,
                Role::Rook => 5,
                Role::Queen => 9,
                Role::King => 0,
            };
            v * if piece.color.is_white() == white {
                1
            } else {
                -1
            }
        })
        .sum()
}
pub fn detect(p: &Chess) -> Phase {
    let total: i32 = p
        .board()
        .iter()
        .map(|(_, piece)| match piece.role {
            Role::Knight | Role::Bishop => 3,
            Role::Rook => 5,
            Role::Queen => 9,
            _ => 0,
        })
        .sum();
    let queens = p.board().iter().any(|(_, piece)| piece.role == Role::Queen);
    if total <= 20 || (!queens && total <= 30) {
        return Phase::Endgame;
    }
    let home = [
        (Square::B1, Color::White, Role::Knight),
        (Square::C1, Color::White, Role::Bishop),
        (Square::F1, Color::White, Role::Bishop),
        (Square::G1, Color::White, Role::Knight),
        (Square::B8, Color::Black, Role::Knight),
        (Square::C8, Color::Black, Role::Bishop),
        (Square::F8, Color::Black, Role::Bishop),
        (Square::G8, Color::Black, Role::Knight),
    ]
    .iter()
    .filter(|(sq, c, r)| {
        p.board().piece_at(*sq)
            == Some(shakmaty::Piece {
                color: *c,
                role: *r,
            })
    })
    .count();
    if total >= 52 && home >= 4 {
        Phase::Opening
    } else {
        Phase::Middlegame
    }
}
