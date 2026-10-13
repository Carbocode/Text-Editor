use eframe::egui::{self, Color32, FontId, TextStyle};
pub const BG: Color32 = Color32::from_rgb(11, 16, 24);
pub const PANEL: Color32 = Color32::from_rgb(17, 24, 35);
pub const BORDER: Color32 = Color32::from_rgb(38, 49, 66);
pub const TEXT: Color32 = Color32::from_rgb(220, 228, 240);
pub const MUTED: Color32 = Color32::from_rgb(140, 155, 178);
pub const ACCENT: Color32 = Color32::from_rgb(111, 151, 255);
pub const GREEN: Color32 = Color32::from_rgb(95, 213, 168);
pub const AMBER: Color32 = Color32::from_rgb(235, 182, 95);
pub const RED: Color32 = Color32::from_rgb(242, 121, 137);
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = BG;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.selection.bg_fill = Color32::from_rgb(38, 63, 105);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(27, 37, 53);
    style.visuals.widgets.inactive.weak_bg_fill = PANEL;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(41, 56, 79);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(50, 73, 111);
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(22.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(12.0));
    ctx.set_style(style);
}
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(10)
        .inner_margin(18)
}
pub fn muted(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).color(MUTED)
}
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(12.0).strong().color(color));
        });
}
