use std::fmt;

/// Zero-based file/rank, stored in rank-major order: a1=0, h8=63.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Square(u8);

impl Square {
    pub fn new(file: u8, rank: u8) -> Option<Self> {
        if file < 8 && rank < 8 {
            Some(Self(rank * 8 + file))
        } else {
            None
        }
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
    pub fn file(self) -> u8 {
        self.0 % 8
    }
    pub fn rank(self) -> u8 {
        self.0 / 8
    }
    pub fn is_dark(self) -> bool {
        (self.file() + self.rank()).is_multiple_of(2)
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", char::from(b'a' + self.file()), self.rank() + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_64_valid_squares_and_coordinates() {
        let mut indices = std::collections::BTreeSet::new();
        for rank in 0..8 {
            for file in 0..8 {
                let square = Square::new(file, rank).unwrap();
                assert_eq!(square.file(), file);
                assert_eq!(square.rank(), rank);
                assert_eq!(
                    square.to_string(),
                    format!("{}{}", char::from(b'a' + file), rank + 1)
                );
                indices.insert(square.index());
            }
        }
        assert_eq!(
            indices.into_iter().collect::<Vec<_>>(),
            (0..64).collect::<Vec<_>>()
        );
        assert!(Square::new(8, 0).is_none());
        assert!(Square::new(0, 8).is_none());
        assert!(Square::new(255, 255).is_none());
    }

    #[test]
    fn colors_alternate_with_dark_a1_and_light_h1() {
        assert!(Square::new(0, 0).unwrap().is_dark());
        assert!(!Square::new(7, 0).unwrap().is_dark());
        for rank in 0..8 {
            for file in 0..8 {
                let dark = Square::new(file, rank).unwrap().is_dark();
                if file < 7 {
                    assert_ne!(dark, Square::new(file + 1, rank).unwrap().is_dark());
                }
                if rank < 7 {
                    assert_ne!(dark, Square::new(file, rank + 1).unwrap().is_dark());
                }
            }
        }
    }
}
