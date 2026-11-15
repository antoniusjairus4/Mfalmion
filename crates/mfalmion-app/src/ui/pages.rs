use super::Page;
use crate::theme;
use eframe::egui::{self, RichText};

pub fn show(ui: &mut egui::Ui, page: Page) {
    ui.label(
        RichText::new("YOUR CHESS WORKSPACE")
            .size(11.0)
            .color(theme::CYAN),
    );
    ui.add_space(8.0);
    ui.heading(RichText::new(page.title()).size(32.0));
    ui.label(RichText::new(page.description()).color(theme::MUTED));
    ui.add_space(28.0);
    egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(12)
        .inner_margin(28.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("MILESTONE 01").small().color(theme::PURPLE));
            ui.add_space(20.0);
            ui.label(RichText::new(if page == Page::Dashboard { "Your workspace is ready." } else { "Room for what comes next." }).size(23.0).strong());
            ui.add_space(8.0);
            ui.label("This section is planned for a future milestone.");
            ui.label(RichText::new("Chess analysis, engine play, account sync, and AI coaching are not implemented yet.").color(theme::MUTED));
            ui.add_space(20.0);
            ui.separator();
            ui.label(RichText::new("LOCAL FIRST  /  BUILT WITH RUST").small().color(theme::MINT));
        });
    if page == Page::Settings {
        ui.add_space(24.0);
        ui.heading("Planned preferences");
        for label in [
            "Stockfish engine configuration",
            "AI model configuration",
            "GPU offloading",
            "Database location",
            "Chess.com account management",
            "Appearance",
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.label(label);
                ui.label(
                    RichText::new("Planned · read-only preview")
                        .small()
                        .color(theme::MUTED),
                );
            });
            ui.separator();
        }
    }
}
