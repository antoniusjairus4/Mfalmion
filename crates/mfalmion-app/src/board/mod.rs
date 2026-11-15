//! Native board renderer with library-backed legal moves.
mod animation;
mod artwork;
mod game;
mod position;
mod square;
mod state;
mod view;

pub use artwork::PieceTextures;
pub use position::Position;
pub use state::BoardState;
pub use view::{BoardInput, Highlights, show};

pub use game::{Game, MoveResult};
pub use position::Kind;
pub use square::Square;

pub use animation::AnimationState;
