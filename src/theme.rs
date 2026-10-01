use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, RichText, Shadow, Stroke, TextStyle, Ui, Vec2};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(14, 16, 22);
pub const BAR: Color32 = Color32::from_rgb(18, 21, 29);
pub const SURFACE: Color32 = Color32::from_rgb(24, 28, 38);
pub const RAISED: Color32 = Color32::from_rgb(34, 39, 52);
pub const INPUT: Color32 = Color32::from_rgb(11, 13, 18);
pub const BORDER: Color32 = Color32::from_rgb(42, 48, 64);
pub const EDGE: Color32 = Color32::from_rgb(64, 72, 94);
pub const TEXT: Color32 = Color32::from_rgb(228, 231, 240);
pub const MUTED: Color32 = Color32::from_rgb(140, 146, 164);
pub const ACCENT: Color32 = Color32::from_rgb(124, 108, 246);
pub const ACCENT_HI: Color32 = Color32::from_rgb(154, 140, 255);
pub const RED: Color32 = Color32::from_rgb(242, 84, 91);
pub const AMBER: Color32 = Color32::from_rgb(245, 165, 36);
pub const GREEN: Color32 = Color32::from_rgb(61, 214, 140);

const RADIUS: u8 = 5;
const BOLD: &str = "semibold";

fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    for (name, bytes) in [("inter", &include_bytes!("../assets/fonts/Inter-Regular.otf")[..]), (BOLD, &include_bytes!("../assets/fonts/Inter-SemiBold.otf")[..])] {
        defs.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let fallback = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    defs.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    defs.families.insert(FontFamily::Name(BOLD.into()), std::iter::once(BOLD.to_string()).chain(fallback).collect());
    defs
}

fn widget(v: &mut egui::style::WidgetVisuals, fill: Color32, stroke: Color32, fg: Color32) {
    v.bg_fill = fill;
    v.weak_bg_fill = fill;
    v.bg_stroke = Stroke::new(1.0, stroke);
    v.fg_stroke = Stroke::new(1.0, fg);
    v.corner_radius = CornerRadius::same(RADIUS);
    v.expansion = 0.0;
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.popup_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(120) };
    visuals.extreme_bg_color = INPUT;
    visuals.faint_bg_color = SURFACE;
    visuals.override_text_color = None;
    visuals.hyperlink_color = ACCENT_HI;
    visuals.error_fg_color = RED;
    visuals.warn_fg_color = AMBER;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.slider_trailing_fill = true;
    visuals.collapsing_header_frame = false;
    visuals.indent_has_left_vline = false;
    let w = &mut visuals.widgets;
    widget(&mut w.noninteractive, SURFACE, BORDER, TEXT);
    widget(&mut w.inactive, RAISED, EDGE, TEXT);
    widget(&mut w.hovered, Color32::from_rgb(44, 50, 68), ACCENT, Color32::WHITE);
    widget(&mut w.active, ACCENT, ACCENT_HI, Color32::WHITE);
    widget(&mut w.open, RAISED, ACCENT, TEXT);
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        s.spacing.button_padding = Vec2::new(11.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.combo_width = 220.0;
        let sizes = [
            (TextStyle::Small, 12.0, FontFamily::Proportional),
            (TextStyle::Body, 14.0, FontFamily::Proportional),
            (TextStyle::Button, 14.0, FontFamily::Proportional),
            (TextStyle::Heading, 20.0, FontFamily::Name(BOLD.into())),
            (TextStyle::Monospace, 13.0, FontFamily::Monospace),
        ];
        for (style, size, family) in sizes {
            s.text_styles.insert(style, FontId::new(size, family));
        }
    });
}

pub fn bold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(FontFamily::Name(BOLD.into())).size(size)
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(MUTED)
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn section_title(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(bold(title, 20.0).color(TEXT));
    if !subtitle.is_empty() {
        ui.label(muted(subtitle));
    }
    ui.add_space(4.0);
}
