pub mod game_review;
pub mod pages;
pub mod sidebar;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Dashboard,
    GameReview,
    GameLibrary,
    Play,
    Coach,
    Settings,
}

impl Page {
    pub const ALL: [Self; 6] = [
        Self::Dashboard,
        Self::GameReview,
        Self::GameLibrary,
        Self::Play,
        Self::Coach,
        Self::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::GameReview => "Game Review",
            Self::GameLibrary => "Game Library",
            Self::Play => "Play vs Stockfish",
            Self::Coach => "AI Coach",
            Self::Settings => "Settings",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Dashboard => "A quiet place to understand your chess journey.",
            Self::GameReview => "Explore the decisions that shape every game.",
            Self::GameLibrary => "Your games, organized locally and always within reach.",
            Self::Play => "A future practice space powered by Stockfish.",
            Self::Coach => "Turn engine insights into understandable chess ideas.",
            Self::Settings => "Make your analysis workspace your own.",
        }
    }
}

mod engine_panel;
