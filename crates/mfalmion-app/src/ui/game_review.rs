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
    record: crate::notation::GameRecord,
    cursor: usize,
    imported: Vec<crate::notation::GameRecord>,
    selected_game: usize,
    protected: bool,
    branch_base: Option<crate::notation::GameRecord>,
    text_input: String,
    fen_input: String,
    error: Option<String>,
    file_task: Option<std::sync::mpsc::Receiver<Result<crate::file_dialog::FileReply, String>>>,
}

impl GameReview {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll_file();
        if !ui.ctx().egui_wants_keyboard_input() && self.promotion.is_none() {
            let target = ui.input(|i| {
                if i.key_pressed(egui::Key::Home) {
                    Some(0)
                } else if i.key_pressed(egui::Key::End) {
                    Some(self.record.moves.len())
                } else if i.key_pressed(egui::Key::ArrowLeft) {
                    Some(self.cursor.saturating_sub(1))
                } else if i.key_pressed(egui::Key::ArrowRight) {
                    Some((self.cursor + 1).min(self.record.moves.len()))
                } else {
                    None
                }
            });
            if let Some(target) = target {
                self.navigate(target, ui.input(|i| i.time));
            }
        }
        ui.label(
            RichText::new("MILESTONE 02C · GAME REPLAY")
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
        ui.add_enabled_ui(!self.game.is_over(), |ui| {
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
                    draggable_side: (!self.protected && self.cursor == self.record.moves.len())
                        .then(|| self.game.side_to_move()),
                    promotion: self.promotion.map(|(_, to)| (to, self.game.side_to_move())),
                },
            );
        });
        if let Some(input) = input {
            self.handle_input(input, now);
            ui.ctx().request_repaint();
        }
    }

    fn handle_input(&mut self, input: BoardInput, now: f64) {
        if let Some((from, to)) = self.promotion {
            match input {
                BoardInput::Promote(kind) => self.attempt(from, to, Some(kind), now, None),
                BoardInput::CancelPromotion => {
                    self.promotion = None;
                    self.state.selected = None;
                }
                _ => {}
            }
            return;
        }
        if self.game.is_over()
            || self.record.result != "*"
            || self.protected
            || self.cursor != self.record.moves.len()
        {
            return;
        }
        self.notice = None;
        match input {
            BoardInput::Promote(_) | BoardInput::CancelPromotion => {}
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
        let before_authority = self.game.authoritative().clone();
        let before = self.game.position();
        let result = if self.claim_next_move {
            self.game.claim_with_move(from, to, promotion)
        } else {
            self.game.try_move(from, to, promotion)
        };
        match result {
            MoveResult::Played => {
                use shakmaty::Position as _;
                // Derive SAN from the validated library move, never from display coordinates.
                if let Some(mv) = before_authority.legal_moves().into_iter().find(|mv| {
                    before_authority.clone().play(*mv).is_ok_and(|p| {
                        crate::notation::fen(&p) == crate::notation::fen(self.game.authoritative())
                    })
                }) {
                    self.record.append(mv);
                    if self.game.status().starts_with("Draw") {
                        self.record.result = "1/2-1/2".into();
                    }
                    self.cursor = self.record.moves.len();
                }
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
                self.record.result = "1/2-1/2".into();
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
        self.record = crate::notation::GameRecord::default();
        self.cursor = 0;
        self.protected = false;
        self.branch_base = None;
        self.clear_interaction();
    }
    fn undo(&mut self) {
        if self.cursor > 0 {
            self.navigate(self.cursor - 1, 0.0);
        } else {
            self.game.undo();
        }
        self.clear_interaction();
    }
    fn flip(&mut self) {
        self.animations.clear();
        self.state.flip();
    }

    fn navigate(&mut self, target: usize, now: f64) {
        let target = target.min(self.record.moves.len());
        let before = self.game.position();
        let adjacent = self.cursor.abs_diff(target) == 1;
        self.clear_interaction();
        self.cursor = target;
        self.game = Game::restore(&self.record.positions, target);
        if adjacent {
            self.animations
                .transition(&before, &self.game.position(), now);
        }
    }
    fn install(&mut self, record: crate::notation::GameRecord, protected: bool) {
        self.record = record;
        self.protected = protected;
        self.branch_base = None;
        self.cursor = 0;
        self.game = Game::restore(&self.record.positions, 0);
        self.clear_interaction();
        self.error = None;
    }
    fn poll_file(&mut self) {
        let reply = self
            .file_task
            .as_ref()
            .and_then(|task| task.try_recv().ok());
        if let Some(reply) = reply {
            self.file_task = None;
            match reply {
                Ok(crate::file_dialog::FileReply::Imported(records)) => {
                    self.import_records(records)
                }
                Ok(crate::file_dialog::FileReply::Saved) => self.notice = Some("PGN saved."),
                Ok(crate::file_dialog::FileReply::Cancelled) => {}
                Err(error) => self.error = Some(error),
            }
        }
    }
    fn import_records(&mut self, records: Vec<crate::notation::GameRecord>) {
        self.install(records[0].clone(), true);
        self.imported = records;
        self.selected_game = 0;
    }
    fn replay_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if !self.imported.is_empty() {
            let mut selected = None;
            egui::ComboBox::from_id_salt("imported_games")
                .selected_text(format!(
                    "Game {} of {}",
                    self.selected_game + 1,
                    self.imported.len()
                ))
                .show_ui(ui, |ui| {
                    for (index, record) in self.imported.iter().enumerate() {
                        if ui
                            .selectable_label(
                                self.selected_game == index,
                                format!(
                                    "{}: {} – {}",
                                    index + 1,
                                    record.headers.get("White").map_or("?", String::as_str),
                                    record.headers.get("Black").map_or("?", String::as_str)
                                ),
                            )
                            .clicked()
                        {
                            selected = Some(index);
                        }
                    }
                });
            if let Some(index) = selected {
                self.selected_game = index;
                self.install(self.imported[index].clone(), true);
            }
        }
        if let (Some(white), Some(black)) = (
            self.record.headers.get("White"),
            self.record.headers.get("Black"),
        ) {
            ui.label(format!("{white} – {black}"));
        }
        ui.horizontal(|ui| {
            for (label, target) in [
                ("|<", 0),
                ("<", self.cursor.saturating_sub(1)),
                (">", (self.cursor + 1).min(self.record.moves.len())),
                (">|", self.record.moves.len()),
            ] {
                if ui.button(label).clicked() {
                    self.navigate(target, ui.input(|i| i.time));
                    ui.ctx().stop_dragging();
                }
            }
        });
        ui.collapsing("Game metadata", |ui| {
            for (name, value) in &self.record.headers {
                ui.label(format!("{name}: {value}"));
            }
        });
        ui.label(format!(
            "Position {} / {} · {}",
            self.cursor,
            self.record.moves.len(),
            self.record.result
        ));
        let mut jump = None;
        egui::ScrollArea::vertical()
            .id_salt("move_history")
            .max_height(160.0)
            .show(ui, |ui| {
                egui::Grid::new("san_moves").show(ui, |ui| {
                    use shakmaty::Position as _;
                    for (index, san) in self.record.moves.iter().enumerate() {
                        let pos = &self.record.positions[index];
                        if pos.turn().is_white() || index == 0 {
                            ui.label(format!("{}.", pos.fullmoves()));
                            if pos.turn().is_black() {
                                ui.label("…");
                            }
                        }
                        if ui.selectable_label(self.cursor == index + 1, san).clicked() {
                            jump = Some(index + 1);
                        }
                        if pos.turn().is_black() {
                            ui.end_row();
                        }
                    }
                });
            });
        if let Some(target) = jump {
            self.navigate(target, ui.input(|i| i.time));
        }
        if self.protected || self.cursor != self.record.moves.len() {
            ui.label("Replay is read-only. Create a local variation to play from here.");
            if ui.button("Create local variation from here").clicked() {
                self.branch_base = Some(self.record.clone());
                self.record = self.record.fork(self.cursor);
                self.protected = false;
                self.clear_interaction();
            }
        }
        if self.branch_base.is_some() {
            ui.colored_label(theme::CYAN, "Local variation · original mainline preserved");
            if ui.button("Return to original mainline").clicked() {
                let original = self.branch_base.take().expect("branch base");
                self.install(original, true);
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.file_task.is_none(),
                    egui::Button::new("Import PGN file"),
                )
                .clicked()
            {
                self.file_task = Some(crate::file_dialog::start(None, ui.ctx().clone()));
            }
            if ui
                .add_enabled(
                    self.file_task.is_none(),
                    egui::Button::new("Export PGN file"),
                )
                .clicked()
            {
                self.file_task = Some(crate::file_dialog::start(
                    Some(self.record.pgn()),
                    ui.ctx().clone(),
                ));
            }
            if ui.button("Copy PGN").clicked() {
                ui.ctx().copy_text(self.record.pgn());
            }
        });
        if self.file_task.is_some() {
            ui.label("Waiting for native file dialog / file operation…");
        }
        ui.collapsing("Paste PGN", |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.text_input)
                    .desired_rows(4)
                    .desired_width(210.0),
            );
            if ui
                .add_enabled(
                    self.file_task.is_none(),
                    egui::Button::new("Load pasted PGN"),
                )
                .clicked()
            {
                self.file_task = Some(crate::file_dialog::parse_text(
                    self.text_input.clone(),
                    ui.ctx().clone(),
                ));
            }
        });
        ui.collapsing("FEN position", |ui| {
            let current = crate::notation::fen(self.game.authoritative());
            ui.label(&current);
            if ui.button("Copy current FEN").clicked() {
                ui.ctx().copy_text(current);
            }
            ui.add(
                egui::TextEdit::multiline(&mut self.fen_input)
                    .desired_rows(2)
                    .desired_width(210.0),
            );
            if ui.button("Load FEN as new session").clicked() {
                match crate::notation::parse_fen(&self.fen_input) {
                    Ok(position) => self.install(crate::notation::GameRecord::new(position), false),
                    Err(e) => self.error = Some(e),
                }
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(theme::DANGER, error);
        }
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
            if self.promotion.is_some() { ui.label("Choose a piece on the board. Escape or click outside to cancel."); }
            self.replay_controls(ui);
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
            if ui.add_enabled(self.promotion.is_none() && !self.protected && self.cursor == self.record.moves.len() && self.record.result == "*" && self.game.can_claim_draw(), egui::Button::new("Claim Draw")).clicked() {
                self.game.claim_draw();
                self.record.result = "1/2-1/2".into();
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

    #[test]
    fn local_variation_can_return_to_complete_imported_mainline() {
        let mut review = imported_review();
        let original = review.record.pgn();
        review.navigate(2, 1.0);
        review.branch_base = Some(review.record.clone());
        review.record = review.record.fork(2);
        review.protected = false;
        review.attempt(sq(5, 0), sq(2, 3), None, 2.0, None); // Bc4
        assert_eq!(review.record.moves[2], "Bc4");
        assert_eq!(review.imported[0].pgn(), original);
        let base = review.branch_base.take().unwrap();
        review.install(base, true);
        review.navigate(4, 3.0);
        assert_eq!(review.record.pgn(), original);
        assert_eq!(review.cursor, 4);
    }
    #[test]
    fn replay_restores_ep_rights_counters_and_repetition_prefix() {
        let mut review = GameReview::default();
        review.import_records(crate::notation::parse_pgn("1.e4 a6 2.e5 d5 3.exd6 *").unwrap());
        review.navigate(4, 1.0);
        assert!(review.game.capture_targets(sq(4, 4)).contains(&sq(3, 5)));
        let expected = crate::notation::fen(review.game.authoritative());
        review.navigate(0, 2.0);
        review.navigate(5, 3.0);
        review.navigate(4, 4.0);
        assert_eq!(crate::notation::fen(review.game.authoritative()), expected);
        let repeated =
            crate::notation::parse_pgn("1.Nf3 Nf6 2.Ng1 Ng8 3.Nf3 Nf6 4.Ng1 Ng8 *").unwrap();
        review.import_records(repeated);
        review.navigate(8, 5.0);
        assert!(review.game.can_claim_draw());
        review.navigate(0, 6.0);
        assert!(!review.game.can_claim_draw());
    }

    fn imported_review() -> GameReview {
        let mut review = GameReview::default();
        review.import_records(crate::notation::parse_pgn("1.e4 e5 2.Nf3 Nc6 *").unwrap());
        review
    }
    #[test]
    fn indexed_navigation_preserves_mainline_and_replaces_motion() {
        let mut review = imported_review();
        let original = review.record.pgn();
        for target in [1, 2, 1, 0, 1, 2, 3, 4, 3, 4, 0, 4] {
            review.navigate(target, 1.0 + target as f64 * 0.01);
            assert_eq!(
                crate::notation::fen(review.game.authoritative()),
                crate::notation::fen(&review.record.positions[target])
            );
            assert!(review.animations.tracks().len() <= 2);
            assert_eq!(review.record.pgn(), original);
        }
        review.animations.advance(10.0);
        assert!(!review.animations.is_active());
        review.navigate(0, 11.0);
        assert_eq!(review.cursor, 0);
        assert!(!review.animations.is_active());
        review.handle_input(BoardInput::Click(sq(4, 1)), 12.0);
        assert!(review.state.selected.is_none()); // imported mainline read-only
    }
    #[test]
    fn promotion_cancel_and_both_castles_never_confused() {
        let mut review = GameReview::default();
        let promotion = crate::notation::parse_fen("7k/P7/8/8/8/8/8/7K w - - 0 1").unwrap();
        review.install(crate::notation::GameRecord::new(promotion), false);
        let before = crate::notation::fen(review.game.authoritative());
        review.handle_input(
            BoardInput::Drop {
                from: sq(0, 6),
                to: Some(sq(0, 7)),
                at: [0.5, 7.5],
            },
            1.0,
        );
        assert!(review.promotion.is_some());
        review.handle_input(BoardInput::Click(sq(7, 0)), 1.1);
        assert!(review.promotion.is_some());
        review.handle_input(BoardInput::CancelPromotion, 1.2);
        assert!(review.promotion.is_none());
        assert_eq!(crate::notation::fen(review.game.authoritative()), before);
        for to in [2, 6] {
            let position =
                crate::notation::parse_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
            review.install(crate::notation::GameRecord::new(position), false);
            review.attempt(sq(4, 0), sq(to, 0), None, 2.0, None);
            assert!(review.promotion.is_none());
            assert_eq!(
                review.game.position().piece_at(sq(to, 0)).unwrap().kind,
                Kind::King
            );
            assert_eq!(review.animations.tracks().len(), 2);
        }
    }

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
