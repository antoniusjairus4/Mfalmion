use crate::{
    board::{self, BoardInput, BoardState, Game, Kind, MoveResult, Square},
    theme,
};
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct GameReview {
    game: Game,
    textures: board::PieceTextures,
    animations: board::AnimationState,
    state: BoardState,
    promotion: Option<(Square, Square)>,
    notice: Option<&'static str>,
    claim_next_move: bool,
}

impl GameReview {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("MILESTONE 02B.1 · SMOOTH MOVEMENT")
                .small()
                .color(theme::CYAN),
        );
        ui.heading(RichText::new("Game Review").size(32.0));
        ui.label(
            RichText::new("Move pieces by clicking a piece then a destination, or drag and drop.")
                .color(theme::MUTED),
        );
        ui.add_space(16.0);
        let available = ui.available_width();
        let max_side = (ui.ctx().content_rect().height() - 240.0).clamp(240.0, 640.0);
        if available >= 820.0 {
            ui.horizontal_top(|ui| {
                let width = (available - 280.0).min(max_side + 44.0);
                ui.allocate_ui(egui::vec2(width, width), |ui| self.board(ui, max_side));
                ui.vertical(|ui| {
                    ui.set_width(240.0);
                    self.details(ui);
                });
            });
        } else {
            self.board(ui, max_side);
            ui.add_space(16.0);
            self.details(ui);
        }
    }

    fn board(&mut self, ui: &mut egui::Ui, side: f32) {
        let now = ui.input(|input| input.time);
        self.animations.advance(now);
        let targets = self
            .state
            .selected
            .map_or_else(Vec::new, |s| self.game.legal_targets(s));
        let captures = self
            .state
            .selected
            .map_or_else(Vec::new, |s| self.game.capture_targets(s));
        let mut input = None;
        ui.add_enabled_ui(self.promotion.is_none() && !self.game.is_over(), |ui| {
            input = board::show(
                ui,
                &self.game.position(),
                &self.state,
                side,
                &mut self.textures,
                board::Highlights {
                    legal_targets: &targets,
                    capture_targets: &captures,
                    checked_king: self.game.checked_king(),
                    animations: Some(&self.animations),
                    draggable_side: Some(self.game.side_to_move()),
                },
            );
        });
        if let Some(input) = input {
            self.handle_input(input, now);
            ui.ctx().request_repaint();
        }
    }

    fn handle_input(&mut self, input: BoardInput, now: f64) {
        if self.promotion.is_some() || self.game.is_over() {
            return;
        }
        self.notice = None;
        match input {
            BoardInput::DragStart(square) => {
                self.animations.clear();
                self.state.selected = self.game.owns(square).then_some(square);
            }
            BoardInput::Drop { from, to, at } => {
                if let Some(to) = to {
                    self.attempt(from, to, None, now, Some(at));
                } else {
                    self.snap_back(from, at, now);
                }
            }
            BoardInput::Click(square) => {
                if self.state.selected == Some(square) {
                    self.state.select(square);
                } else if self.game.owns(square) {
                    self.state.selected = Some(square);
                } else if let Some(from) = self.state.selected {
                    self.attempt(from, square, None, now, None);
                } else {
                    self.notice = Some("Select a piece belonging to the side to move.");
                }
            }
        }
    }

    fn attempt(
        &mut self,
        from: Square,
        to: Square,
        promotion: Option<Kind>,
        now: f64,
        drop_origin: Option<[f32; 2]>,
    ) {
        let before = self.game.position();
        let result = if self.claim_next_move {
            self.game.claim_with_move(from, to, promotion)
        } else {
            self.game.try_move(from, to, promotion)
        };
        match result {
            MoveResult::Played => {
                self.animations
                    .transition(&before, &self.game.position(), now);
                if let Some(origin) = drop_origin {
                    self.animations.settle_drop(from, to, origin);
                }
                self.state.selected = None;
                self.claim_next_move = false;
                self.promotion = None;
                self.notice = None;
            }
            MoveResult::DrawClaimed => {
                self.animations.clear();
                self.state.selected = None;
                self.claim_next_move = false;
                self.promotion = None;
            }
            MoveResult::NeedsPromotion => {
                self.animations.clear();
                self.promotion = Some((from, to));
            }
            MoveResult::NotClaimable | MoveResult::Illegal => {
                self.promotion = None;
                self.notice = Some(if result == MoveResult::NotClaimable {
                    "That move cannot claim a draw. Uncheck the claim option to play it."
                } else {
                    "That move is not legal. Choose a mint destination."
                });
                if let Some(origin) = drop_origin {
                    self.snap_back(from, origin, now);
                }
            }
        }
    }

    fn snap_back(&mut self, from: Square, origin: [f32; 2], now: f64) {
        if self.game.owns(from)
            && let Some(piece) = self.game.position().piece_at(from)
        {
            self.animations.snap_back(piece, from, origin, now);
        }
    }

    fn clear_interaction(&mut self) {
        self.animations.clear();
        self.state.selected = None;
        self.promotion = None;
        self.notice = None;
        self.claim_next_move = false;
    }
    fn new_game(&mut self) {
        self.game = Game::default();
        self.clear_interaction();
    }
    fn undo(&mut self) {
        self.game.undo();
        self.clear_interaction();
    }
    fn flip(&mut self) {
        self.animations.clear();
        self.state.flip();
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(8)
            .inner_margin(16.0);
        frame.show(ui, |ui| {
            ui.label(RichText::new("BOARD CONTROLS").small().color(theme::PURPLE));
            ui.label(
                RichText::new(self.game.status())
                    .size(18.0)
                    .color(theme::MINT),
            );
            ui.separator();
            ui.label("Selected square");
            let selected = self
                .state
                .selected
                .map_or_else(|| "None".into(), |s| s.to_string());
            ui.label(RichText::new(selected).size(26.0).color(theme::CYAN));
            ui.label(RichText::new(self.state.orientation.label()).color(theme::MUTED));
            if ui.button("Flip Board").clicked() {
                self.flip();
                ui.ctx().stop_dragging();
                ui.ctx().request_repaint();
            }
            if let Some((from, to)) = self.promotion {
                ui.separator();
                ui.label(RichText::new("Choose promotion").color(theme::CYAN));
                ui.horizontal_wrapped(|ui| {
                    for (label, kind) in [
                        ("Queen", Kind::Queen),
                        ("Rook", Kind::Rook),
                        ("Bishop", Kind::Bishop),
                        ("Knight", Kind::Knight),
                    ] {
                        if ui.button(label).clicked() {
                            self.attempt(from, to, Some(kind), ui.input(|input| input.time), None);
                            ui.ctx().request_repaint();
                        }
                    }
                });
                if ui.button("Cancel promotion").clicked() {
                    self.promotion = None;
                    ui.ctx().request_repaint();
                }
            }
            if let Some(notice) = self.notice {
                ui.label(RichText::new(notice).color(theme::PURPLE));
            }
            ui.separator();
            ui.label(
                RichText::new(
                    "Mint markers show legal moves. Castle by moving the king to c1/g1 or c8/g8.",
                )
                .small()
                .color(theme::MUTED),
            );
            if ui.button("New Game").clicked() {
                self.new_game();
                ui.ctx().stop_dragging();
                ui.ctx().request_repaint();
            }
            if ui.add_enabled(self.game.can_undo() || self.promotion.is_some(), egui::Button::new("Undo Last Move")).clicked() {
                self.undo();
                ui.ctx().stop_dragging();
                ui.ctx().request_repaint();
            }
            ui.separator();
            if ui.add_enabled(self.promotion.is_none() && self.game.can_claim_draw(), egui::Button::new("Claim Draw")).clicked() {
                self.game.claim_draw();
                self.animations.clear();
                self.state.selected = None;
                self.claim_next_move = false;
                ui.ctx().request_repaint();
            }
            ui.add_enabled_ui(!self.game.is_over() && self.promotion.is_none(), |ui| {
                ui.checkbox(&mut self.claim_next_move, "Claim draw with next move");
            });
            if self.claim_next_move {
                ui.label(RichText::new("Declare a legal move that produces a threefold or 50-move claim. The move will not be played.").small().color(theme::MUTED));
            }
            ui.separator();
            ui.label("Engine evaluation");
            ui.label(
                RichText::new("Not connected. Both sides are controlled locally.")
                    .small()
                    .color(theme::MUTED),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(file: u8, rank: u8) -> Square {
        Square::new(file, rank).unwrap()
    }
    fn point(column: u8, row: u8) -> egui::Pos2 {
        egui::pos2(
            22.0 + (f32::from(column) + 0.5) * 35.0,
            22.0 + (f32::from(row) + 0.5) * 35.0,
        )
    }
    fn frame(ctx: &egui::Context, review: &mut GameReview, events: Vec<egui::Event>) {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::splat(400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| review.board(ui, 280.0),
        )
        .drop_without_applying_deltas();
    }
    fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn click(ctx: &egui::Context, review: &mut GameReview, column: u8, row: u8) {
        let p = point(column, row);
        for pressed in [true, false] {
            frame(
                ctx,
                review,
                vec![egui::Event::PointerMoved(p), button(p, pressed)],
            );
        }
    }
    fn drag(ctx: &egui::Context, review: &mut GameReview, from: egui::Pos2, to: egui::Pos2) {
        frame(
            ctx,
            review,
            vec![egui::Event::PointerMoved(from), button(from, true)],
        );
        frame(ctx, review, vec![egui::Event::PointerMoved(to)]);
        frame(ctx, review, vec![button(to, false)]);
    }

    #[test]
    fn headless_click_move_and_flipped_drag_reach_rules_engine() {
        let ctx = egui::Context::default();
        let mut review = GameReview::default();
        frame(&ctx, &mut review, vec![]);
        click(&ctx, &mut review, 4, 6); // e2
        assert_eq!(review.state.selected, Some(sq(4, 1)));
        click(&ctx, &mut review, 4, 4); // e4
        assert!(review.game.position().piece_at(sq(4, 1)).is_none());
        assert!(review.game.position().piece_at(sq(4, 3)).is_some());
        assert_eq!(review.state.selected, None);
        review.flip();
        frame(&ctx, &mut review, vec![]);
        drag(&ctx, &mut review, point(3, 6), point(3, 4)); // black e7-e5, flipped
        assert!(review.game.position().piece_at(sq(4, 6)).is_none());
        assert!(review.game.position().piece_at(sq(4, 4)).is_some());
        assert_eq!(review.game.status(), "White to move");
    }

    #[test]
    fn illegal_and_off_board_drags_leave_position_unchanged() {
        let ctx = egui::Context::default();
        let mut review = GameReview::default();
        frame(&ctx, &mut review, vec![]);
        drag(&ctx, &mut review, point(4, 6), point(4, 3)); // illegal e2-e5
        assert!(review.game.position().piece_at(sq(4, 1)).is_some());
        assert!(review.game.position().piece_at(sq(4, 4)).is_none());
        assert!(review.notice.is_some());
        drag(&ctx, &mut review, point(4, 6), egui::pos2(380.0, 380.0));
        assert!(review.game.position().piece_at(sq(4, 1)).is_some());
        assert_eq!(review.game.status(), "White to move");
        drag(&ctx, &mut review, point(0, 1), point(0, 2)); // wrong side a7-a6
        assert!(review.game.position().piece_at(sq(0, 6)).is_some());
        assert!(review.game.position().piece_at(sq(0, 5)).is_none());
    }

    #[test]
    fn selecting_same_piece_deselects_and_friendly_piece_reselects() {
        let mut review = GameReview::default();
        review.handle_input(BoardInput::Click(sq(4, 1)), 0.0);
        review.handle_input(BoardInput::Click(sq(3, 1)), 0.0);
        assert_eq!(review.state.selected, Some(sq(3, 1)));
        review.handle_input(BoardInput::Click(sq(3, 1)), 0.0);
        assert_eq!(review.state.selected, None);
    }
    fn animated_opening() -> GameReview {
        let mut review = GameReview::default();
        review.attempt(sq(4, 1), sq(4, 3), None, 1.0, None);
        assert!(review.animations.is_active());
        assert_eq!(review.game.status(), "Black to move"); // authority updates before animation completes
        review
    }

    #[test]
    fn undo_during_animation_cancels_motion_and_restores_authority() {
        let mut review = animated_opening();
        review.undo();
        assert!(!review.animations.is_active());
        assert_eq!(review.game.status(), "White to move");
        assert!(review.game.position().piece_at(sq(4, 1)).is_some());
        assert!(review.game.position().piece_at(sq(4, 3)).is_none());
        review.animations.advance(10.0);
        assert!(!review.animations.is_active());
    }

    #[test]
    fn new_game_during_animation_clears_pending_visual_and_input_state() {
        let mut review = animated_opening();
        review.promotion = Some((sq(0, 6), sq(0, 7)));
        review.claim_next_move = true;
        review.new_game();
        assert!(!review.animations.is_active());
        assert!(review.promotion.is_none());
        assert!(!review.claim_next_move);
        assert!(review.state.selected.is_none());
        assert_eq!(review.game.status(), "White to move");
        assert!(!review.game.can_undo());
    }

    #[test]
    fn flip_during_animation_finishes_visual_without_mutating_position() {
        let mut review = animated_opening();
        review.flip();
        assert!(!review.animations.is_active());
        assert_eq!(review.state.orientation.label(), "Black at bottom");
        assert_eq!(review.game.status(), "Black to move");
        assert!(review.game.position().piece_at(sq(4, 3)).is_some());
        review.flip();
        assert!(!review.animations.is_active());
    }

    #[test]
    fn rapid_legal_moves_replace_old_motion_and_converge() {
        let mut review = animated_opening();
        review.attempt(sq(4, 6), sq(4, 4), None, 1.04, None);
        assert_eq!(review.animations.tracks().len(), 1);
        assert_eq!(review.animations.tracks()[0].from, sq(4, 6));
        assert!(review.game.position().piece_at(sq(4, 3)).is_some());
        assert!(review.game.position().piece_at(sq(4, 4)).is_some());
        assert!(!review.animations.hides(sq(4, 3)));
        review.animations.advance(2.0);
        assert!(!review.animations.is_active());
        assert_eq!(review.game.status(), "White to move");
    }

    #[test]
    fn promotion_does_not_animate_until_confirmation() {
        let mut review = GameReview::default();
        for (from, to) in [
            (sq(0, 1), sq(0, 3)),
            (sq(7, 6), sq(7, 5)),
            (sq(0, 3), sq(0, 4)),
            (sq(7, 5), sq(7, 4)),
            (sq(0, 4), sq(0, 5)),
            (sq(7, 4), sq(7, 3)),
            (sq(0, 5), sq(1, 6)),
            (sq(7, 3), sq(7, 2)),
        ] {
            assert_eq!(review.game.try_move(from, to, None), MoveResult::Played);
        }
        review.attempt(sq(1, 6), sq(0, 7), None, 2.0, None);
        assert!(review.promotion.is_some());
        assert!(!review.animations.is_active());
        assert_eq!(
            review.game.position().piece_at(sq(1, 6)).unwrap().kind,
            Kind::Pawn
        );
        review.attempt(sq(1, 6), sq(0, 7), Some(Kind::Knight), 3.0, None);
        assert!(review.promotion.is_none());
        assert!(review.animations.is_active());
        assert_eq!(review.animations.tracks()[0].piece.kind, Kind::Knight);
        assert_eq!(
            review.game.position().piece_at(sq(0, 7)).unwrap().kind,
            Kind::Knight
        );
    }
}
