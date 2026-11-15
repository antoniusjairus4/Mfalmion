use super::Page;
use crate::theme;
use eframe::egui::{self, RichText};

pub fn show(ui: &mut egui::Ui, selected: &mut Page) {
    let compact = ui.ctx().content_rect().width() < 1000.0;
    egui::Panel::left("navigation")
        .exact_size(if compact { 180.0 } else { 232.0 })
        .resizable(false)
        .frame(egui::Frame::new().fill(theme::SURFACE).inner_margin(16.0))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(16.0);
                ui.label(
                    RichText::new("MFALMION")
                        .size(if compact { 20.0 } else { 25.0 })
                        .color(theme::CYAN)
                        .strong(),
                );
                ui.label(
                    RichText::new("CHESS • LOCAL FIRST")
                        .size(10.0)
                        .color(theme::MUTED),
                );
                ui.add_space(36.0);
                ui.label(RichText::new("WORKSPACE").size(10.0).color(theme::MUTED));
                for page in Page::ALL {
                    let active = *selected == page;
                    let text = RichText::new(page.title()).size(14.0).color(if active {
                        theme::CYAN
                    } else {
                        theme::TEXT
                    });
                    if ui
                        .add_sized(
                            [ui.available_width(), 44.0],
                            egui::Button::new(text).selected(active),
                        )
                        .clicked()
                    {
                        *selected = page;
                    }
                }
                ui.add_space(40.0);
                ui.separator();
                ui.label(
                    RichText::new("Built for deeper understanding.")
                        .small()
                        .color(theme::MUTED),
                );
            });
        });
}
