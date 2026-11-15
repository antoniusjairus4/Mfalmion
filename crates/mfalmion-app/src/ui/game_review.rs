use crate::{
    board::{self, BoardState, Position},
    theme,
};
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct GameReview {
    position: Position,
    state: BoardState,
}

impl GameReview {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("MILESTONE 02A · BOARD UI")
                .small()
                .color(theme::CYAN),
        );
        ui.heading(RichText::new("Game Review").size(32.0));
        ui.label(
            RichText::new(
                "Explore the starting position. Select a square to inspect its coordinate.",
            )
            .color(theme::MUTED),
        );
        ui.add_space(16.0);
        let available = ui.available_width();
        let max_side = (ui.ctx().content_rect().height() - 240.0).clamp(240.0, 640.0);
        if available >= 820.0 {
            ui.horizontal_top(|ui| {
                let width = (available - 280.0).min(max_side + 44.0);
                ui.allocate_ui(egui::vec2(width, width), |ui| {
                    board::show(ui, &self.position, &mut self.state, max_side);
                });
                ui.vertical(|ui| {
                    ui.set_width(240.0);
                    self.details(ui);
                });
            });
        } else {
            board::show(ui, &self.position, &mut self.state, max_side);
            ui.add_space(16.0);
            self.details(ui);
        }
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(theme::SURFACE)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(8)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("BOARD INSPECTOR")
                        .small()
                        .color(theme::PURPLE),
                );
                ui.add_space(8.0);
                ui.label("Selected square");
                ui.label(
                    RichText::new(
                        self.state
                            .selected
                            .map_or_else(|| "None".into(), |s| s.to_string()),
                    )
                    .size(26.0)
                    .color(theme::CYAN),
                );
                ui.label(RichText::new(self.state.orientation.label()).color(theme::MUTED));
                if ui.button("Flip Board").clicked() {
                    self.state.flip();
                    ui.ctx().request_repaint();
                }
                ui.label(
                    RichText::new(
                        "Click the selected square again to clear it. Pieces stay in place.",
                    )
                    .small()
                    .color(theme::MUTED),
                );
                ui.separator();
                ui.label("Move history");
                ui.label(
                    RichText::new("Planned for a future milestone.")
                        .small()
                        .color(theme::MUTED),
                );
                ui.separator();
                ui.label("Engine evaluation");
                ui.label(
                    RichText::new("Not connected. No evaluation available.")
                        .small()
                        .color(theme::MUTED),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Standard starting position")
                        .small()
                        .color(theme::MINT),
                );
            });
    }
}
