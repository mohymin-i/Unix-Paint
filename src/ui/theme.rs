//! Visual style: a Fluent-inspired light/dark look with rounded controls.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub window: Color32,
    pub toolbar: Color32,
    pub workspace: Color32,
    pub panel: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub hover: Color32,
    pub selection: Color32,
}

pub const ACCENT: Color32 = Color32::from_rgb(0, 103, 192);

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            dark,
            window: Color32::from_rgb(32, 32, 32),
            toolbar: Color32::from_rgb(43, 43, 43),
            workspace: Color32::from_rgb(25, 25, 25),
            panel: Color32::from_rgb(39, 39, 39),
            border: Color32::from_rgb(62, 62, 62),
            text: Color32::from_rgb(240, 240, 240),
            text_dim: Color32::from_rgb(170, 170, 170),
            accent: Color32::from_rgb(76, 194, 255),
            accent_soft: Color32::from_rgba_unmultiplied(76, 194, 255, 40),
            hover: Color32::from_rgb(55, 55, 55),
            selection: Color32::from_rgb(62, 62, 62),
        }
    } else {
        Palette {
            dark,
            window: Color32::from_rgb(243, 243, 243),
            toolbar: Color32::from_rgb(251, 251, 251),
            workspace: Color32::from_rgb(222, 226, 233),
            panel: Color32::from_rgb(249, 249, 249),
            border: Color32::from_rgb(222, 222, 222),
            text: Color32::from_rgb(26, 26, 26),
            text_dim: Color32::from_rgb(96, 96, 96),
            accent: ACCENT,
            accent_soft: Color32::from_rgba_unmultiplied(0, 103, 192, 30),
            hover: Color32::from_rgb(234, 234, 234),
            selection: Color32::from_rgb(224, 224, 224),
        }
    }
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    let r = CornerRadius::same(5);
    v.window_fill = p.panel;
    v.panel_fill = p.window;
    v.extreme_bg_color = if p.dark { Color32::from_rgb(28, 28, 28) } else { Color32::WHITE };
    v.faint_bg_color = p.hover;
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.hyperlink_color = p.accent;
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = if p.dark { Color32::from_rgb(55, 55, 55) } else { Color32::from_rgb(230, 230, 230) };
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.active.weak_bg_fill = p.selection;
    v.widgets.active.bg_fill = p.selection;
    v.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.open.weak_bg_fill = p.hover;
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 120 } else { 40 }) };
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if p.dark { 140 } else { 50 }) };
    ctx.set_visuals(v);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("palette"), *p));

    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.menu_margin = egui::Margin::same(6);
        // The edge fade would tint the canvas.
        s.spacing.scroll.fade.strength = 0.0;
        s.text_styles = [
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(19.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
    });
}

/// Fonts: a modern system UI font when available, plus the Phosphor icon fonts.
pub fn install_fonts(ctx: &egui::Context, ui_font: Option<Vec<u8>>) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "phosphor".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Phosphor.ttf"))
            .tweak(egui::FontTweak { y_offset_factor: 0.08, ..Default::default() })
            .into(),
    );
    fonts.font_data.insert(
        "phosphor-fill".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Phosphor-Fill.ttf"))
            .tweak(egui::FontTweak { y_offset_factor: 0.08, ..Default::default() })
            .into(),
    );
    if let Some(bytes) = ui_font {
        fonts.font_data.insert("system-ui".into(), egui::FontData::from_owned(bytes).into());
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "system-ui".into());
    }
    fonts.families.entry(FontFamily::Proportional).or_default().push("phosphor".into());
    fonts.families.insert(FontFamily::Name("fill".into()), vec!["phosphor-fill".into(), "phosphor".into()]);
    ctx.set_fonts(fonts);
}
