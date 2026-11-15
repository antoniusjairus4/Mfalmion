//! Display-only board UI: no move generation, FEN parser, or chess rules.
mod position;
mod square;
mod state;
mod view;

pub use position::Position;
pub use state::BoardState;
pub use view::{install_font, show};
