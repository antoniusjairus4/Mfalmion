use crate::{
    theme,
    ui::{self, Page},
};
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct MfalmionApp {
    page: Page,
    closing: bool,
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
        Self::default()
    }
}

impl eframe::App for MfalmionApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            self.game_review.shutdown_engine();
            if !self.game_review.engine_stopped() {
                self.closing = true;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.closing {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(20));
            if self.game_review.engine_stopped() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("● Desktop shell ready").color(theme::MINT));
                ui.separator();
                ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                ui.separator();
                ui.label("Milestone 03 · Stockfish analysis");
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.closing {
                        ui.label("Stopping engine and closing…");
                    } else if self.page == Page::GameReview {
                        self.game_review.show(ui);
                    } else {
                        ui::pages::show(ui, self.page);
                    }
                });
            });
    }
}
