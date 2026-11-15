use super::square::Square;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    WhiteBottom,
    BlackBottom,
}

impl Orientation {
    /// Display row zero is the top, column zero is the left, in either orientation.
    pub fn square_at(self, column: u8, row: u8) -> Option<Square> {
        if column >= 8 || row >= 8 {
            return None;
        }
        match self {
            Self::WhiteBottom => Square::new(column, 7 - row),
            Self::BlackBottom => Square::new(7 - column, row),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::WhiteBottom => "White at bottom",
            Self::BlackBottom => "Black at bottom",
        }
    }
}

#[derive(Default)]
pub struct BoardState {
    pub orientation: Orientation,
    pub selected: Option<Square>,
}

impl BoardState {
    pub fn select(&mut self, square: Square) {
        self.selected = if self.selected == Some(square) {
            None
        } else {
            Some(square)
        };
    }

    pub fn flip(&mut self) {
        self.orientation = match self.orientation {
            Orientation::WhiteBottom => Orientation::BlackBottom,
            Orientation::BlackBottom => Orientation::WhiteBottom,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_corners_and_every_square_map_correctly() {
        let white = Orientation::WhiteBottom;
        let black = Orientation::BlackBottom;
        assert_eq!(white.square_at(0, 0).unwrap().to_string(), "a8");
        assert_eq!(white.square_at(7, 7).unwrap().to_string(), "h1");
        assert_eq!(black.square_at(0, 0).unwrap().to_string(), "h1");
        assert_eq!(black.square_at(7, 7).unwrap().to_string(), "a8");
        for row in 0..8 {
            for column in 0..8 {
                assert_eq!(
                    white.square_at(column, row),
                    black.square_at(7 - column, 7 - row)
                );
                assert_eq!(white.square_at(column, row).unwrap().file(), column);
                assert_eq!(white.square_at(column, row).unwrap().rank(), 7 - row);
            }
        }
        assert!(white.square_at(8, 0).is_none());
        assert!(black.square_at(0, 8).is_none());
    }

    #[test]
    fn selection_toggles_and_flip_preserves_logical_selection() {
        let mut state = BoardState::default();
        let d1 = Square::new(3, 0).unwrap();
        let e4 = Square::new(4, 3).unwrap();
        assert_eq!(state.selected, None);
        state.select(d1);
        assert_eq!(state.selected, Some(d1));
        state.select(e4);
        assert_eq!(state.selected, Some(e4));
        state.flip();
        assert_eq!(state.orientation, Orientation::BlackBottom);
        assert_eq!(state.selected, Some(e4));
        state.flip();
        assert_eq!(state.orientation, Orientation::WhiteBottom);
        state.select(e4);
        assert_eq!(state.selected, None);
    }
}
