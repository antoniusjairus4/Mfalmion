use crate::{
    board::{self, BoardInput, BoardState, Game, Kind, MoveResult, Square},
    theme,
};
use eframe::egui::{self, RichText};

enum SessionAction {
    NewGame,
    Install(Box<crate::notation::GameRecord>, bool),
    Stored(i64, i64, Box<crate::notation::GameRecord>),
    Select(usize),
    Import(Vec<crate::notation::GameRecord>),
    Return,
    Fork,
}

#[derive(Default)]
pub struct GameReview {
    game: Game,
    saved: crate::storage::review::ReviewData,
    tools_open: bool,
    live_open: bool,
    pub persistence_import: Option<(i64, Vec<crate::notation::GameRecord>)>,
    pub current_profile: i64,
    pub stored_identity: Option<(i64, i64, String)>,
    file_profile: Option<i64>,
    import_profile: Option<i64>,
    analysis: super::engine_panel::EnginePanel,
    analysis_session: u64,
    textures: board::PieceTextures,
    animations: board::AnimationState,
    state: BoardState,
    promotion: Option<(Square, Square)>,
    notice: Option<&'static str>,
    claim_next_move: bool,
    record: crate::notation::GameRecord,
    cursor: usize,
    imported: Vec<crate::notation::GameRecord>,
    selected_game: Option<usize>,
    protected: bool,
    branch_base: Option<crate::notation::GameRecord>,
    text_input: String,
    fen_input: String,
    error: Option<String>,
    local_work: bool,
    pending_action: Option<SessionAction>,
    file_task: Option<crate::file_dialog::FileTask>,
}

impl GameReview {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll_file();
        self.analysis
            .sync(self.game.authoritative(), self.analysis_session);
        ui.add_enabled_ui(self.pending_action.is_none(), |ui| self.contents(ui));
        if self.pending_action.is_some() {
            ui.ctx().stop_dragging();
            egui::Window::new("Replace local work?").collapsible(false).resizable(false).show(ui.ctx(), |ui| {
                ui.label("This action replaces local moves or an analysis position. Export or copy the PGN first if you want to keep it.");
                if ui.button("Keep current session").clicked() { self.pending_action = None; }
                if ui.button("Replace session").clicked() && let Some(action) = self.pending_action.take() { self.apply_action(action); }
            });
        }
    }
    pub fn set_saved_review(&mut self, data: crate::storage::review::ReviewData) {
        self.saved = data;
    }
    fn saved_summary(&self) -> Option<&crate::analysis::Summary> {
        (self.is_stored_mainline()
            && self
                .stored_identity
                .as_ref()
                .is_some_and(|(p, _, _)| *p == self.current_profile)
            && self.saved.matches(self.stored_identity.as_ref()))
        .then_some(self.saved.summary.as_deref())
        .flatten()
    }
    pub fn set_batch_owned(&mut self, owned: bool) {
        self.analysis.batch_owned = owned;
    }
    pub fn shutdown_engine(&mut self) {
        self.analysis.shutdown();
    }
    pub fn engine_stopped(&self) -> bool {
        self.analysis.is_shutdown()
    }
    pub fn open_stored(&mut self, profile: i64, id: i64, record: crate::notation::GameRecord) {
        self.request_action(SessionAction::Stored(profile, id, Box::new(record)));
    }
    pub fn is_stored_mainline(&self) -> bool {
        self.protected && self.stored_identity.is_some()
    }
    pub fn record(&self) -> &crate::notation::GameRecord {
        &self.record
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn engine_settings(&self) -> &crate::engine::Settings {
        &self.analysis.settings
    }
    pub fn set_engine_settings(&mut self, settings: crate::engine::Settings) {
        self.analysis.settings = settings;
    }
    pub fn completed_analysis(
        &mut self,
    ) -> Option<(
        String,
        crate::engine::Settings,
        crate::engine::protocol::Snapshot,
    )> {
        self.analysis.completed_analysis()
    }
    fn contents(&mut self, ui: &mut egui::Ui) {
        // Dense review controls use local spacing; the surrounding dark application theme remains unchanged.
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
        ui.spacing_mut().button_padding = egui::vec2(7.0, 4.0);
        if self.pending_action.is_none()
            && !ui.ctx().egui_wants_keyboard_input()
            && self.promotion.is_none()
        {
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
        ui.heading(RichText::new("Game Review").size(28.0));
        self.toolbar(ui);
        let layout =
            super::review_layout::Layout::calculate(ui.available_width(), ui.available_height());
        let gap = (ui.available_width() - layout.left - layout.right).max(0.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.allocate_ui_with_layout(
                egui::vec2(layout.left, layout.height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_max_width(layout.left);
                    self.board(ui, layout.board);
                    ui.label(RichText::new(self.status_text()).color(theme::MINT));
                    if let Some(notice) = self.notice {
                        ui.label(notice);
                    }
                    if self.promotion.is_some() {
                        ui.label("Choose a piece on the board; Escape cancels.");
                    }
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(layout.right, layout.height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(0.0);
                    ui.set_max_width(layout.right);
                    self.review_sidebar(ui, layout.compact);
                },
            );
        });
        let mut open = self.tools_open;
        egui::Window::new("Game tools · PGN / FEN / metadata")
            .open(&mut open)
            .default_width(480.0)
            .show(ui.ctx(), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("review_tools_metadata")
                    .max_height((ui.ctx().content_rect().height() - 140.0).max(80.0))
                    .show(ui, |ui| self.details(ui));
            });
        self.tools_open = open;
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        use shakmaty::Position as _;
        let white = self
            .record
            .headers
            .get("White")
            .map_or("White", String::as_str);
        let black = self
            .record
            .headers
            .get("Black")
            .map_or("Black", String::as_str);
        let players = format!("{white} — {black} · {}", self.record.result);
        ui.add(egui::Label::new(RichText::new(&players).strong()).truncate())
            .on_hover_text(players);
        ui.horizontal_wrapped(|ui| {
            for (label, target, hint) in [
                ("|◀ First", 0, "Home"),
                ("◀ Previous", self.cursor.saturating_sub(1), "Left arrow"),
                (
                    "Next ▶",
                    (self.cursor + 1).min(self.record.moves.len()),
                    "Right arrow",
                ),
                ("Last ▶|", self.record.moves.len(), "End"),
            ] {
                if ui
                    .add_enabled(
                        target != self.cursor && self.promotion.is_none(),
                        egui::Button::new(label),
                    )
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.navigate(target, ui.input(|i| i.time));
                    ui.ctx().stop_dragging();
                }
            }
            if ui.button("Flip board").clicked() {
                self.flip();
                ui.ctx().stop_dragging();
            }
            if ui.button("Game tools…").clicked() {
                self.tools_open = true;
            }
            let position = &self.record.positions[self.cursor.saturating_sub(1)];
            ui.label(format!(
                "Move {} · position {}/{}",
                position.fullmoves(),
                self.cursor,
                self.record.moves.len()
            ));
            ui.label(
                RichText::new("← / → · Home / End")
                    .size(12.0)
                    .color(theme::MUTED),
            );
        });
        ui.horizontal_wrapped(|ui| {
            for (label, forward, mistakes) in [
                ("Previous mistake/blunder", false, true),
                ("Next mistake/blunder", true, true),
                ("Previous critical move", false, false),
                ("Next critical move", true, false),
            ] {
                let target = super::review_layout::critical_target(
                    self.saved_summary(),
                    self.cursor,
                    forward,
                    mistakes,
                );
                if ui
                    .add_enabled(
                        target.is_some() && self.promotion.is_none(),
                        egui::Button::new(RichText::new(label).size(12.0)),
                    )
                    .clicked()
                    && let Some(target) = target
                {
                    self.navigate(target, ui.input(|i| i.time));
                    ui.ctx().stop_dragging();
                }
            }
        });
        ui.separator();
    }
    fn review_sidebar(&mut self, ui: &mut egui::Ui, compact: bool) {
        self.summary_card(ui, compact);
        ui.separator();
        ui.strong("Move history · Mfalmion classifications");
        let history_height = if self.live_open && compact {
            64.0
        } else {
            (ui.available_height() * 0.32).clamp(64.0, 220.0)
        };
        let mut target = None;
        egui::ScrollArea::vertical()
            .id_salt("review_move_history")
            .max_height(history_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let summary = self.saved_summary();
                use shakmaty::Position as _;
                for (i, san) in self.record.moves.iter().enumerate() {
                    let ply = i + 1;
                    let pos = &self.record.positions[i];
                    let detail = summary
                        .and_then(|s| s.moves.get(i))
                        .filter(|m| m.ply == ply);
                    let label = detail.map_or("Not analyzed", |m| m.classification.label());
                    let text = RichText::new(format!(
                        "{}{} {san}  ·  {label}",
                        pos.fullmoves(),
                        if pos.turn().is_white() { "." } else { "…" }
                    ))
                    .size(13.0)
                    .color(detail.map_or(theme::MUTED, |m| {
                        super::review_layout::color(m.classification)
                    }));
                    let response =
                        ui.add(egui::Button::new(text).selected(self.cursor == ply).wrap());
                    if response.clicked() {
                        target = Some(ply);
                    }
                    if let Some(m) = detail {
                        response.on_hover_text(format!(
                            "Mfalmion {} · {} · expected-score loss {}",
                            m.classification.label(),
                            m.confidence,
                            m.loss
                                .map_or_else(|| "Not analyzed".into(), |v| format!("{v:.4}"))
                        ));
                    }
                }
                if self.record.moves.is_empty() {
                    ui.label("No moves in this session.");
                }
            });
        if let Some(target) = target {
            self.navigate(target, ui.input(|i| i.time));
        }
        ui.separator();
        self.selected_card(ui);
        let label = format!(
            "{} Live interactive Stockfish · requested depth {}",
            if self.live_open { "▼" } else { "▶" },
            self.analysis.settings.depth
        );
        if ui.selectable_label(self.live_open, label).clicked() {
            self.live_open = !self.live_open;
        }
        if self.live_open {
            egui::ScrollArea::vertical()
                .id_salt("review_engine_details")
                .max_height(ui.available_height().max(1.0))
                .show(ui, |ui| {
                    ui.label("Live evaluations do not change saved batch classifications.");
                    self.analysis
                        .show(ui, self.game.authoritative(), self.analysis_session);
                });
        }
    }

    fn summary_card(&self, ui: &mut egui::Ui, compact: bool) {
        ui.strong("Game Analysis Summary");
        let Some(summary) = self.saved_summary() else {
            let status = if self
                .stored_identity
                .as_ref()
                .is_some_and(|(p, _, _)| *p != self.current_profile)
            {
                "This stored game belongs to another profile; select its profile to view saved analysis"
            } else if self.is_stored_mainline() && self.saved.matches(self.stored_identity.as_ref())
            {
                self.saved.status.as_str()
            } else if self.is_stored_mainline() {
                "Loading saved review…"
            } else {
                "Not analyzed · open a stored mainline or save and analyze this game in Game Library"
            };
            ui.add(egui::Label::new(status).wrap());
            return;
        };
        if self.saved.status.contains("latest attempt") {
            ui.add(egui::Label::new(&self.saved.status).wrap());
        }
        ui.add(egui::Label::new(super::review_layout::saved_label(summary)).truncate())
            .on_hover_text(format!(
                "{} · {} · completed saved evidence",
                super::review_layout::saved_label(summary),
                summary.config.algorithm
            ));
        ui.label(RichText::new("Mfalmion Estimated Accuracy").size(13.0));
        ui.horizontal_wrapped(|ui| {
            for (side, value) in [
                ("White", summary.accuracy.white),
                ("Black", summary.accuracy.black),
            ] {
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .corner_radius(4)
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        ui.strong(format!("{side} {}", super::review_layout::accuracy(value)));
                    });
            }
        });
        let counts = |ui: &mut egui::Ui| {
            use crate::analysis::classify::Category as C;
            ui.horizontal_wrapped(|ui|{for c in [C::Brilliant,C::Great,C::Best,C::Excellent,C::Good,C::Inaccuracy,C::Mistake,C::Blunder,C::Miss]{
                let n=summary.counts.get(c.label()).copied().unwrap_or(0);
                ui.label(RichText::new(format!("{} {n}",c.label())).size(12.0).background_color(theme::SURFACE).color(super::review_layout::color(c))).on_hover_text(format!("Mfalmion {} count — saved batch analysis, not an official Chess.com classification",c.label()));
            }});
        };
        if compact {
            egui::CollapsingHeader::new("Classification counts")
                .id_salt("review_counts")
                .show(ui, counts);
        } else {
            counts(ui);
        }
        ui.add(
            egui::Label::new(format!(
                "Eligible W/B {:?} · excluded W/B {:?}",
                summary.accuracy.eligible, summary.accuracy.excluded
            ))
            .truncate(),
        )
        .on_hover_text(&self.saved.status);
    }
    fn selected_card(&self, ui: &mut egui::Ui) {
        ui.strong("Selected Move Analysis");
        let Some(m) = self
            .saved_summary()
            .and_then(|_| self.saved.move_at(self.cursor))
        else {
            ui.label(if self.cursor == 0 {
                "Initial position · select a move"
            } else {
                "Not analyzed"
            });
            return;
        };
        ui.label(
            RichText::new(format!("{} · {}", m.san, m.classification.label()))
                .color(super::review_layout::color(m.classification))
                .strong(),
        );
        ui.label(format!(
            "Expected-score loss: {} · {} · depth {}",
            m.loss
                .map_or_else(|| "Not analyzed".into(), |v| format!("{v:.4}")),
            m.confidence,
            m.depth
        ));
        let before = &self.record.positions[self.cursor - 1];
        let best = m
            .best
            .as_ref()
            .map(|u| crate::engine::protocol::decode_pv(before, std::slice::from_ref(u)).text)
            .unwrap_or_else(|| "Not analyzed".into());
        ui.add(egui::Label::new(format!("Best engine move: {best}")).truncate())
            .on_hover_text(best);
        let line = crate::engine::protocol::decode_pv(before, &m.pv).text;
        ui.add(
            egui::Label::new(format!(
                "Best alternative: {}",
                if line.is_empty() {
                    "Not analyzed"
                } else {
                    &line
                }
            ))
            .truncate(),
        )
        .on_hover_text(line);
        let evaluation = if self
            .saved
            .key
            .as_ref()
            .is_some_and(|k| k.ply == self.cursor)
        {
            self.saved
                .evaluation
                .as_ref()
                .map(|l| {
                    use crate::analysis::Value;
                    use crate::engine::protocol::{Bound, Score};
                    let score = match l.value {
                        Value::Cp(cp) => Score::Cp(cp),
                        Value::Mate { white, moves } => Score::Mate {
                            winner: if white {
                                shakmaty::Color::White
                            } else {
                                shakmaty::Color::Black
                            },
                            moves,
                        },
                        Value::Draw => return "Draw".into(),
                    };
                    score.label(match l.bound.as_str() {
                        "lower" => Bound::Lower,
                        "upper" => Bound::Upper,
                        _ => Bound::Exact,
                    })
                })
                .unwrap_or_else(|| "Not analyzed".into())
        } else {
            "Loading saved evaluation…".into()
        };
        ui.label(format!("Position evaluation (White): {evaluation}"));
        if !m.tags.is_empty() {
            let tags = m.tags.join(" · ");
            ui.add(egui::Label::new(format!("Tags: {tags}")).truncate())
                .on_hover_text(tags);
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
        let draggable_side = self.can_play().then(|| self.game.side_to_move());
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
                    draggable_side,
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
        if !self.can_play() {
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
        let before = self.game.position();
        let result = if self.claim_next_move {
            self.game.claim_with_move(from, to, promotion)
        } else {
            self.game.try_move(from, to, promotion)
        };
        match result {
            MoveResult::Played => {
                let saved = self
                    .game
                    .last_played()
                    .ok_or_else(|| "Missing validated move identifier".to_owned())
                    .and_then(|mv| self.record.append(mv));
                if let Err(error) = saved {
                    self.game = Game::restore(&self.record.positions, self.cursor);
                    self.clear_interaction();
                    self.error = Some(error);
                    return;
                }
                self.analysis.invalidate();
                self.local_work = true;
                self.cursor = self.record.moves.len();
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
                self.record.claimed_draw = self.game.claimed_draw();
                self.local_work = true;
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
        self.analysis_session = self.analysis_session.wrapping_add(1);
        self.analysis.invalidate();
        self.local_work = false;
        self.selected_game = None;
        self.error = None;
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
        } else if self.game.undo() && self.record.claimed_draw.take().is_some() {
            use shakmaty::Position as _;
            self.record.result = self.game.authoritative().outcome().to_string();
            self.local_work = true;
        }
        self.clear_interaction();
    }
    fn flip(&mut self) {
        self.animations.clear();
        self.state.flip();
    }

    fn navigate(&mut self, target: usize, now: f64) {
        let target = target.min(self.record.moves.len());
        if target == self.cursor {
            self.clear_interaction();
            return;
        }
        self.analysis.invalidate();
        let before = self.game.position();
        let adjacent = self.cursor.abs_diff(target) == 1;
        self.clear_interaction();
        self.cursor = target;
        self.game = Game::restore(&self.record.positions, target);
        if target == self.record.moves.len() {
            self.game.restore_claim(self.record.claimed_draw);
        }
        if adjacent {
            self.animations
                .transition(&before, &self.game.position(), now);
        }
    }
    fn install(&mut self, record: crate::notation::GameRecord, protected: bool) {
        self.stored_identity = None;
        self.analysis_session = self.analysis_session.wrapping_add(1);
        self.analysis.invalidate();
        self.local_work = false;
        self.selected_game = None;
        self.record = record;
        self.protected = protected;
        self.branch_base = None;
        self.cursor = 0;
        self.game = Game::restore(&self.record.positions, 0);
        if self.record.moves.is_empty() {
            self.game.restore_claim(self.record.claimed_draw);
        }
        self.clear_interaction();
        self.error = None;
    }
    fn poll_file(&mut self) {
        let reply = self
            .file_task
            .as_ref()
            .and_then(|task| match task.try_recv() {
                Ok(reply) => Some(reply),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("File worker stopped before returning a result".into()))
                }
            });
        if let Some(reply) = reply {
            self.file_task = None;
            self.finish_file(reply);
        }
    }
    fn finish_file(&mut self, reply: Result<crate::file_dialog::FileReply, String>) {
        match reply {
            Ok(crate::file_dialog::FileReply::Imported(records)) => {
                self.import_profile = self.file_profile.take();
                self.request_action(SessionAction::Import(records))
            }
            Ok(crate::file_dialog::FileReply::Text(_)) => {}
            Ok(crate::file_dialog::FileReply::Saved) => self.notice = Some("PGN snapshot saved."),
            Ok(crate::file_dialog::FileReply::Cancelled) => {}
            Err(error) => self.error = Some(error),
        }
    }
    fn import_records(&mut self, records: Vec<crate::notation::GameRecord>) {
        let Some(first) = records.first() else {
            self.error = Some("No imported games".into());
            return;
        };
        self.install(first.clone(), true);
        self.persistence_import = Some((
            self.import_profile.take().unwrap_or(self.current_profile),
            records.clone(),
        ));
        self.imported = records;
        self.selected_game = Some(0);
    }
    fn session_tools(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if !self.imported.is_empty() {
            let mut selected = None;
            egui::ComboBox::from_id_salt("imported_games")
                .selected_text(self.selected_game.map_or_else(
                    || "Imported games".into(),
                    |index| format!("Game {} of {}", index + 1, self.imported.len()),
                ))
                .show_ui(ui, |ui| {
                    for (index, record) in self.imported.iter().enumerate() {
                        if ui
                            .selectable_label(
                                self.selected_game == Some(index),
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
                self.request_action(SessionAction::Select(index));
            }
        }
        if let (Some(white), Some(black)) = (
            self.record.headers.get("White"),
            self.record.headers.get("Black"),
        ) {
            ui.label(format!("{white} – {black}"));
        }
        ui.collapsing("Game metadata", |ui| {
            for (name, value) in &self.record.headers {
                ui.label(format!("{name}: {value}"));
            }
        });
        if self.protected || self.cursor != self.record.moves.len() {
            ui.label("Replay is read-only. Create a local variation to play from here.");
            if ui.button("Create local variation from here").clicked() {
                self.request_action(SessionAction::Fork);
            }
        }
        if self.branch_base.is_some() {
            ui.colored_label(theme::CYAN, "Local variation · original mainline preserved");
            if ui.button("Return to original mainline").clicked() {
                self.request_action(SessionAction::Return);
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
                self.file_profile = Some(self.current_profile);
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
                self.file_profile = Some(self.current_profile);
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
                    Ok(position) => self.request_action(SessionAction::Install(
                        Box::new(crate::notation::GameRecord::new(position)),
                        false,
                    )),
                    Err(e) => self.error = Some(e),
                }
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(theme::DANGER, error);
        }
    }

    fn can_play(&self) -> bool {
        !self.game.is_over()
            && self.record.result == "*"
            && !self.protected
            && self.cursor == self.record.moves.len()
            && self.pending_action.is_none()
    }
    fn status_text(&self) -> String {
        if self.cursor == self.record.moves.len() && self.record.result != "*" {
            format!("Result {} · {}", self.record.result, self.game.status())
        } else {
            self.game.status().into()
        }
    }
    fn request_action(&mut self, action: SessionAction) {
        if self.pending_action.is_some() {
            return;
        }
        if self.local_work {
            self.clear_interaction();
            self.pending_action = Some(action);
        } else {
            self.apply_action(action);
        }
    }
    fn apply_action(&mut self, action: SessionAction) {
        match action {
            SessionAction::NewGame => {
                self.stored_identity = None;
                self.new_game();
            }
            SessionAction::Stored(profile, id, record) => {
                let hash = crate::storage::games::fingerprint(&record);
                self.install(*record, true);
                self.stored_identity = Some((profile, id, hash));
            }
            SessionAction::Install(record, protected) => {
                self.install(*record, protected);
                self.local_work = !protected;
            }
            SessionAction::Select(index) => {
                if let Some(record) = self.imported.get(index) {
                    let record = record.clone();
                    self.install(record, true);
                    self.selected_game = Some(index);
                }
            }
            SessionAction::Import(records) => self.import_records(records),
            SessionAction::Return => {
                if let Some(record) = self.branch_base.take() {
                    let origin = self.selected_game;
                    self.install(record, true);
                    self.selected_game = origin;
                }
            }
            SessionAction::Fork => {
                self.analysis_session = self.analysis_session.wrapping_add(1);
                self.analysis.invalidate();
                self.branch_base = Some(self.record.clone());
                self.record = self.record.fork(self.cursor);
                self.protected = false;
                self.local_work = true;
                self.clear_interaction();
            }
        }
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let frame = egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(8)
            .inner_margin(16.0);
        frame.show(ui, |ui| {
            self.session_tools(ui);
            ui.separator();
            ui.label(RichText::new("BOARD CONTROLS").small().color(theme::PURPLE));
            ui.label(
                RichText::new(self.status_text())
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
            if self.promotion.is_some() { ui.label("Choose a piece on the board. Escape or click outside to cancel."); }

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
                self.request_action(SessionAction::NewGame);
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
                self.record.claimed_draw = self.game.claimed_draw();
                self.local_work = true;
                self.animations.clear();
                self.state.selected = None;
                self.claim_next_move = false;
                ui.ctx().request_repaint();
            }
            ui.add_enabled_ui(self.can_play() && self.promotion.is_none(), |ui| {
                ui.checkbox(&mut self.claim_next_move, "Claim draw with next move");
            });
            if self.claim_next_move {
                ui.label(RichText::new("Declare a legal move that produces a threefold or 50-move claim. The move will not be played.").small().color(theme::MUTED));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_invalid_import_keeps_loaded_game_and_selected_ply() {
        let mut review = imported_review();
        review.navigate(3, 1.0);
        let source = review.record.pgn();
        let before = crate::notation::fen(review.game.authoritative());
        review.finish_file(
            crate::notation::parse_pgn("1.e5 *").map(crate::file_dialog::FileReply::Imported),
        );
        assert_eq!(review.record.pgn(), source);
        assert_eq!(review.imported.len(), 1);
        assert_eq!(review.selected_game, Some(0));
        assert_eq!(review.cursor, 3);
        assert_eq!(crate::notation::fen(review.game.authoritative()), before);
    }

    #[test]
    fn audit_cpu_only_promotion_hit_targets_both_colors_and_orientations() {
        for (fen, from, to) in [
            ("7k/P7/8/8/8/8/8/7K w - - 0 1", sq(0, 6), sq(0, 7)),
            ("7k/8/8/8/8/8/p7/7K b - - 0 1", sq(0, 1), sq(0, 0)),
        ] {
            for flipped in [false, true] {
                for (index, kind) in [Kind::Queen, Kind::Rook, Kind::Bishop, Kind::Knight]
                    .into_iter()
                    .enumerate()
                {
                    let ctx = egui::Context::default();
                    let mut review = GameReview::default();
                    review.install(
                        crate::notation::GameRecord::new(crate::notation::parse_fen(fen).unwrap()),
                        false,
                    );
                    if flipped {
                        review.state.flip();
                    }
                    let orientation = review.state.orientation;
                    review.attempt(from, to, None, 0.0, None);
                    assert!(review.promotion.is_some());
                    frame(&ctx, &mut review, vec![]);
                    let display = orientation
                        .project([f32::from(to.file()) + 0.5, f32::from(to.rank()) + 0.5]);
                    let column = display[0].floor() as u8;
                    let row = (display[1].floor() as u8).min(4) + index as u8;
                    click(&ctx, &mut review, column, row);
                    assert!(review.promotion.is_none());
                    assert_eq!(review.game.position().piece_at(to).unwrap().kind, kind);
                    assert_eq!(review.record.moves.len(), 1);
                }
            }
        }
    }

    #[test]
    fn audit_replacement_requires_explicit_decision_and_invalid_import_preserves_work() {
        let mut review = animated_opening();
        let before = review.record.pgn();
        review.finish_file(Err("Invalid PGN".into()));
        assert_eq!(review.record.pgn(), before);
        assert!(review.error.is_some());
        for action in [
            SessionAction::NewGame,
            SessionAction::Select(0),
            SessionAction::Fork,
            SessionAction::Return,
            SessionAction::Import(crate::notation::parse_pgn("1.d4 *").unwrap()),
            SessionAction::Install(Box::default(), false),
        ] {
            review.request_action(action);
            assert!(review.pending_action.is_some());
            assert_eq!(review.record.pgn(), before);
            assert!(!review.can_play());
            review.pending_action = None; // Keep current session
        }
        review.request_action(SessionAction::NewGame);
        let action = review.pending_action.take().unwrap();
        review.apply_action(action);
        assert_eq!(review.cursor, 0);
        assert!(review.record.moves.is_empty());
        assert!(!review.local_work);
        assert_eq!(review.selected_game, None);
    }
    #[test]
    fn audit_claimed_draw_survives_replay_and_final_result_blocks_input() {
        let mut review = GameReview::default();
        review.import_records(
            crate::notation::parse_pgn("1.Nf3 Nf6 2.Ng1 Ng8 3.Nf3 Nf6 4.Ng1 Ng8 *").unwrap(),
        );
        review.navigate(8, 0.0);
        review.apply_action(SessionAction::Fork);
        review.claim_next_move = true;
        // Current-position claim uses the same persisted adjudication as intended-move claims.
        assert!(review.game.claim_draw());
        review.record.claimed_draw = review.game.claimed_draw();
        review.record.result = "1/2-1/2".into();
        review.navigate(0, 1.0);
        assert!(!review.game.is_over());
        review.navigate(8, 2.0);
        assert!(review.game.is_over());
        assert!(!review.can_play());
        assert!(review.status_text().contains("1/2-1/2"));
        assert!(review.record.pgn().ends_with("1/2-1/2\n"));
    }

    #[test]
    fn audit_claim_at_root_can_be_undone_and_replayed() {
        let mut review = GameReview::default();
        review.install(
            crate::notation::GameRecord::new(
                crate::notation::parse_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 100 51").unwrap(),
            ),
            false,
        );
        assert!(review.game.claim_draw());
        review.record.result = "1/2-1/2".into();
        review.record.claimed_draw = review.game.claimed_draw();
        review.navigate(0, 1.0);
        assert!(review.game.is_over(), "navigation lost adjudication");
        review.undo();
        assert_eq!(review.record.result, "*");
    }

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
            review.attempt(from, to, None, 0.0, None);
            assert!(review.error.is_none());
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
    #[test]
    fn stored_game_replacement_preserves_identity_until_confirmed() {
        let mut review = GameReview {
            local_work: true,
            ..Default::default()
        };
        let record = crate::notation::parse_pgn("1.e4 *").unwrap().remove(0);
        review.open_stored(7, 42, record.clone());
        assert!(review.pending_action.is_some());
        assert!(review.stored_identity.is_none());
        review.pending_action = None; // Keep current session.
        assert!(review.record.moves.is_empty());
        review.open_stored(7, 42, record);
        let action = review.pending_action.take().unwrap();
        review.apply_action(action);
        assert!(review.is_stored_mainline());
        assert_eq!(review.stored_identity.as_ref().unwrap().1, 42);
    }
    #[test]
    fn import_retains_profile_that_started_the_file_operation() {
        let mut review = GameReview {
            current_profile: 8,
            file_profile: Some(7),
            ..Default::default()
        };
        review.finish_file(Ok(crate::file_dialog::FileReply::Imported(
            crate::notation::parse_pgn("1.d4 *").unwrap(),
        )));
        assert_eq!(review.persistence_import.as_ref().unwrap().0, 7);
    }
    fn saved_fixture() -> GameReview {
        use crate::analysis::{
            MoveResult as AnalyzedMove, Summary, classify::Category as C, phase::Phase,
        };
        let mut review = GameReview {
            current_profile: 7,
            ..Default::default()
        };
        let record = crate::notation::parse_pgn("1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 *")
            .unwrap()
            .remove(0);
        review.open_stored(7, 42, record.clone());
        let classes = [C::Best, C::Mistake, C::Great, C::Good, C::Blunder, C::Miss];
        let moves = record
            .moves
            .iter()
            .enumerate()
            .map(|(i, san)| AnalyzedMove {
                depth: 12,
                ply: i + 1,
                white: i % 2 == 0,
                san: san.clone(),
                phase: Phase::Opening,
                classification: classes[i],
                confidence: "fixture".into(),
                tags: vec![],
                loss: Some(0.0),
                centipawn_loss: Some(0),
                best: None,
                pv: vec![],
            })
            .collect();
        let summary = Summary::new(crate::analysis::Config::default(), moves);
        let data = crate::storage::review::ReviewData {
            key: Some(crate::storage::review::ReviewKey {
                profile: 7,
                game: 42,
                fingerprint: crate::storage::games::fingerprint(&record),
                ply: 0,
            }),
            summary: Some(std::sync::Arc::new(summary)),
            status: "Completed fixture".into(),
            evaluation: None,
        };
        review.set_saved_review(data);
        review
    }
    #[test]
    fn saved_classifications_map_to_selected_moves_and_reuse_animated_navigation() {
        let mut review = saved_fixture();
        review.navigate(1, 1.0);
        assert!(review.animations.is_active());
        review.navigate(2, 2.0);
        assert_eq!(review.saved.move_at(review.cursor).unwrap().san, "e5");
        assert_eq!(
            review.saved.move_at(2).unwrap().classification,
            crate::analysis::classify::Category::Mistake
        );
        assert_eq!(
            crate::notation::fen(review.game.authoritative()),
            crate::notation::fen(&review.record.positions[2])
        );
    }
    #[test]
    fn critical_navigation_filters_and_boundaries_use_saved_primary_categories() {
        let mut review = saved_fixture();
        let target = super::super::review_layout::critical_target;
        assert_eq!(target(review.saved_summary(), 0, true, true), Some(2));
        assert_eq!(target(review.saved_summary(), 2, true, true), Some(5));
        assert_eq!(target(review.saved_summary(), 2, true, false), Some(3));
        assert_eq!(target(review.saved_summary(), 6, false, false), Some(5));
        assert_eq!(target(review.saved_summary(), 0, false, false), None);
        assert_eq!(target(review.saved_summary(), 6, true, false), None);
        assert_eq!(target(None, 0, true, false), None);
        review.navigate(target(review.saved_summary(), 0, true, true).unwrap(), 1.0);
        assert_eq!(review.cursor, 2);
    }
    #[test]
    fn layout_resize_and_saved_refresh_preserve_game_orientation_cursor_and_live_settings() {
        let mut review = saved_fixture();
        review.navigate(3, 1.0);
        review.flip();
        let orientation = review.state.orientation;
        let fen = crate::notation::fen(review.game.authoritative());
        let pgn = review.record.pgn();
        let settings = review.engine_settings().clone();
        for (w, h) in [
            (1632.0, 840.0),
            (1152.0, 660.0),
            (1078.0, 528.0),
            (992.0, 480.0),
        ] {
            let _ = super::super::review_layout::Layout::calculate(w, h);
            review.set_saved_review(review.saved.clone());
        }
        assert_eq!(review.cursor, 3);
        assert_eq!(review.state.orientation, orientation);
        assert_eq!(crate::notation::fen(review.game.authoritative()), fen);
        assert_eq!(review.record.pgn(), pgn);
        assert_eq!(review.engine_settings(), &settings);
        assert!(
            super::super::review_layout::saved_label(review.saved_summary().unwrap())
                .contains("depth 12")
        );
        assert_eq!(review.engine_settings().depth, 16);
    }
    #[test]
    fn missing_stale_profile_and_local_branch_never_invent_classifications() {
        let mut review = saved_fixture();
        review.current_profile = 8;
        assert!(review.saved_summary().is_none());
        review.current_profile = 7;
        review.saved.key.as_mut().unwrap().fingerprint = "stale".into();
        assert!(review.saved_summary().is_none());
        review.set_saved_review(Default::default());
        assert!(review.saved_summary().is_none());
        let mut review = saved_fixture();
        review.apply_action(SessionAction::Fork);
        assert!(review.saved_summary().is_none());
    }
    #[test]
    fn cpu_only_review_layout_stays_within_requested_desktop_sizes() {
        for (w, h) in [
            (1920.0, 1080.0),
            (1440.0, 900.0),
            (1366.0, 768.0),
            (1280.0, 720.0),
        ] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            let mut review = saved_fixture();
            review.navigate(3, 0.0);
            let size = egui::vec2(w - 288.0, h - 110.0);
            for open in [false, true] {
                review.live_open = open;
                let mut bounds = egui::Rect::NOTHING;
                ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |ui| {
                        review.contents(ui);
                        bounds = ui.min_rect();
                    },
                )
                .drop_without_applying_deltas();
                assert!(
                    bounds.right() <= size.x + 1.0,
                    "horizontal overflow at {w}x{h}: {bounds:?}"
                );
                assert!(
                    bounds.bottom() <= size.y + 1.0,
                    "vertical overflow at {w}x{h}: {bounds:?}"
                );
            }
        }
    }
}
