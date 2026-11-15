use eframe::egui::{self, Color32};

pub const BACKGROUND: Color32 = Color32::from_rgb(12, 16, 23);
pub const SURFACE: Color32 = Color32::from_rgb(20, 27, 38);
pub const BORDER: Color32 = Color32::from_rgb(42, 54, 70);
pub const CYAN: Color32 = Color32::from_rgb(82, 211, 235);
pub const PURPLE: Color32 = Color32::from_rgb(175, 151, 245);
pub const MINT: Color32 = Color32::from_rgb(115, 223, 180);
pub const TEXT: Color32 = Color32::from_rgb(227, 234, 243);
pub const MUTED: Color32 = Color32::from_rgb(147, 163, 183);
pub const DANGER: Color32 = Color32::from_rgb(246, 103, 119);
pub const BOARD_DARK: Color32 = Color32::from_rgb(0x76, 0x96, 0x56);
pub const BOARD_LIGHT: Color32 = Color32::from_rgb(0xEE, 0xEE, 0xD2);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.selection.bg_fill = BORDER;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, CYAN);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    style.spacing.item_spacing = egui::vec2(12.0, 12.0);
    style.spacing.button_padding = egui::vec2(14.0, 12.0);
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}
