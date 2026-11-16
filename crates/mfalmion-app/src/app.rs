use crate::{
    theme,
    ui::{self, Page},
};
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct MfalmionApp {
    page: Page,
    closing: bool,
    library: ui::library::Library,
    stored: Option<(i64, i64, String)>,
    pending_cache: Option<(
        crate::storage::analysis::Key,
        crate::engine::protocol::Snapshot,
    )>,
    saved_settings: Option<crate::engine::Settings>,
    cache_engine: String,
    review_tools_open: bool,
    game_review: ui::game_review::GameReview,
}

impl MfalmionApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(state) = &cc.wgpu_render_state {
            eprintln!(
                "level=INFO event=graphics_adapter info={:?}",
                state.adapter.get_info()
            );
        }
        theme::apply(&cc.egui_ctx);
        let mut app = Self {
            cache_engine: "Stockfish 17.1".into(),
            ..Default::default()
        };
        app.library.start(&cc.egui_ctx);
        app
    }
}

impl eframe::App for MfalmionApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.library.poll();
        self.library.tick(ui.ctx());
        let batch_owned = self.library.batch.busy();
        self.game_review.set_batch_owned(batch_owned);
        if batch_owned {
            self.game_review.shutdown_engine();
        }
        if self.library.batch.requested.is_some() && self.game_review.engine_stopped() {
            self.library.batch.dispatch();
        }
        if self.library.batch.changed {
            self.library.batch.changed = false;
            self.library.request_refresh();
        }
        self.game_review.current_profile = self.library.active;
        if self.library.loaded_preferences {
            self.library.loaded_preferences = false;
            self.game_review
                .set_engine_settings(self.library.preferences.engine.clone());
            self.saved_settings = Some(self.library.preferences.engine.clone());
        }
        if self.library.switched {
            self.library.switched = false;
            self.stored = None;
            self.pending_cache = None;
        }
        if let Some((profile, id, record)) = self.library.opened.take() {
            self.game_review.open_stored(profile, id, record);
            self.page = Page::GameReview;
        }
        self.stored = self
            .game_review
            .stored_identity
            .as_ref()
            .filter(|(profile, _, _)| *profile == self.library.active)
            .cloned();
        if self.library.available() {
            if let Some((profile, records)) = self.game_review.persistence_import.take() {
                self.library
                    .send(crate::storage::worker::Command::ImportRecords(
                        profile, records,
                    ));
            } else if let Some((key, snapshot)) = self.pending_cache.take() {
                self.library.cache(key, snapshot);
            } else if self.saved_settings.as_ref() != Some(self.game_review.engine_settings())
                && self.game_review.engine_settings().validate().is_ok()
            {
                let mut preferences = self.library.preferences.clone();
                preferences.engine = self.game_review.engine_settings().clone();
                if self
                    .library
                    .send(crate::storage::worker::Command::Preferences(
                        preferences.clone(),
                    ))
                {
                    self.saved_settings = Some(preferences.engine.clone());
                    self.library.preferences = preferences;
                }
            }
        }
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            self.closing = true;
            self.game_review.shutdown_engine();
            if !self.game_review.engine_stopped() || !self.library.stopped() {
                self.closing = true;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.closing {
            if self.library.idle()
                && self.game_review.persistence_import.is_none()
                && self.pending_cache.is_none()
            {
                self.library.shutdown();
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(20));
            if self.game_review.engine_stopped() && self.library.stopped() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("● Desktop shell ready").color(theme::MINT));
                ui.separator();
                ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                ui.separator();
                ui.label("Milestone 06.1 · Game Review");
            });
        });
        let previous = self.page;
        ui::sidebar::show(ui, &mut self.page);
        if previous != self.page {
            eprintln!("level=INFO event=navigation page={:?}", self.page);
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BACKGROUND)
                    .inner_margin(28.0),
            )
            .show(ui, |ui| {
                if self.closing {
                    ui.label("Stopping engine and closing…");
                } else if self.page == Page::GameReview {
                    if ui
                        .small_button("Library storage / position cache…")
                        .clicked()
                    {
                        self.review_tools_open = true;
                    }
                    let mut open = self.review_tools_open;
                    egui::Window::new("Library storage and saved position cache")
                        .open(&mut open)
                        .default_width(520.0)
                        .max_height(360.0)
                        .vscroll(true)
                        .show(ui.ctx(), |ui| {
                            ui.label(&self.library.notice);
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add_enabled(
                                        self.library.available(),
                                        egui::Button::new("Save current game to library"),
                                    )
                                    .clicked()
                                {
                                    self.library.send(
                                        crate::storage::worker::Command::ImportRecords(
                                            self.library.active,
                                            vec![self.game_review.record().clone()],
                                        ),
                                    );
                                }
                                ui.label("Cache engine identity:");
                                ui.text_edit_singleline(&mut self.cache_engine);
                                let stored = self
                                    .stored
                                    .as_ref()
                                    .filter(|_| self.game_review.is_stored_mainline());
                                if ui
                                    .add_enabled(
                                        self.library.available() && stored.is_some(),
                                        egui::Button::new("Read saved position analysis"),
                                    )
                                    .clicked()
                                    && let Some((_, id, _)) = stored
                                    && let Ok(key) = crate::storage::analysis::Key::new(
                                        *id,
                                        self.game_review.record(),
                                        self.game_review.cursor(),
                                        &self.cache_engine,
                                        self.game_review.engine_settings(),
                                    )
                                {
                                    self.library.lookup(key);
                                }
                            });
                            if let Some((cached_key, lines)) = &self.library.cached {
                                let compatible = self.game_review.is_stored_mainline()
                                    && self
                                        .stored
                                        .as_ref()
                                        .is_some_and(|(_, id, _)| *id == cached_key.game)
                                    && self.game_review.cursor() == cached_key.ply
                                    && self.cache_engine == cached_key.engine
                                    && cached_key
                                        .matches_settings(self.game_review.engine_settings());
                                if compatible {
                                    egui::CollapsingHeader::new(
                                    "Saved completed analysis (FEN-only engine context; not live)",
                                )
                                .show(ui, |ui| {
                                    ui.label(format!(
                                        "{} · depth {} · MultiPV {}",
                                        cached_key.engine, cached_key.depth, cached_key.multipv
                                    ));
                                    for line in lines {
                                        let score = match line.score {
                                            crate::storage::analysis::CachedScore::Cp(cp) => {
                                                crate::engine::protocol::Score::Cp(cp)
                                            }
                                            crate::storage::analysis::CachedScore::Mate {
                                                white,
                                                moves,
                                            } => crate::engine::protocol::Score::Mate {
                                                winner: if white {
                                                    shakmaty::Color::White
                                                } else {
                                                    shakmaty::Color::Black
                                                },
                                                moves,
                                            },
                                        };
                                        let bound = match line.bound.as_str() {
                                            "lower" => crate::engine::protocol::Bound::Lower,
                                            "upper" => crate::engine::protocol::Bound::Upper,
                                            _ => crate::engine::protocol::Bound::Exact,
                                        };
                                        let pv = crate::engine::protocol::decode_pv(
                                            &self.game_review.record().positions
                                                [self.game_review.cursor()],
                                            &line.pv,
                                        );
                                        ui.label(format!(
                                            "#{} {}: {}",
                                            line.rank,
                                            score.label(bound),
                                            pv.text
                                        ));
                                    }
                                });
                                }
                            }
                        });
                    self.review_tools_open = open;
                    let identity = self
                        .game_review
                        .stored_identity
                        .as_ref()
                        .filter(|(p, _, _)| *p == self.library.active)
                        .filter(|_| self.game_review.is_stored_mainline())
                        .cloned();
                    self.library
                        .batch
                        .load_review(identity, self.game_review.cursor());
                    self.game_review
                        .set_saved_review(self.library.batch.review_data());
                    self.game_review.show(ui);
                    self.stored = self
                        .game_review
                        .stored_identity
                        .as_ref()
                        .filter(|(p, _, _)| *p == self.library.active)
                        .cloned();
                    if let Some((name, settings, snapshot)) = self.game_review.completed_analysis()
                        && let Some((_, id, _)) = &self.stored
                        && self.game_review.is_stored_mainline()
                        && let Ok(key) = crate::storage::analysis::Key::new(
                            *id,
                            self.game_review.record(),
                            self.game_review.cursor(),
                            &name,
                            &settings,
                        )
                    {
                        self.pending_cache = Some((key, snapshot));
                    }
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if self.page == Page::GameLibrary {
                            self.library.show(ui);
                        } else {
                            ui::pages::show(ui, self.page);
                        }
                    });
                }
            });
    }
}
