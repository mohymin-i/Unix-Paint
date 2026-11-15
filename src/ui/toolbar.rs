//! The tool ribbon: Selection, Image, Tools, Brushes, Shapes, Size, Colors, Layers — the same
//! groups and order as Windows 11 Paint — plus the contextual Text formatting bar.

use crate::app::{PaintApp, Tool};
use crate::brush::{BrushKind, Media, Stroke};
use crate::color::{DEFAULT_PALETTE, Rgba};
use crate::icons as ic;
use crate::shapes::{self, ShapeKind};
use crate::ui::dialogs::Dialog;
use egui::{Color32, FontId, Pos2, Rect, Response, Sense, Stroke as EStroke, Ui, Vec2, pos2, vec2};
use std::collections::HashMap;
use std::sync::Mutex;

const BIG: f32 = 44.0;
const SMALL: f32 = 28.0;

/// Which colour slot the palette writes to (Paint's "ActiveColorSlot").
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Slot {
    #[default]
    Primary,
    Secondary,
}

static ACTIVE_SLOT: Mutex<Slot> = Mutex::new(Slot::Primary);

pub fn active_slot() -> Slot {
    *ACTIVE_SLOT.lock().unwrap()
}

fn set_slot(s: Slot) {
    *ACTIVE_SLOT.lock().unwrap() = s;
}

pub fn icon_button(ui: &mut Ui, icon: &str, selected: bool, size: f32, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let pal = crate::ui::palette_of(ui);
    let bg = if selected {
        pal.accent_soft
    } else if resp.hovered() {
        pal.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5.0, bg);
    if selected {
        let bar = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 2.0), vec2(size * 0.4, 2.5));
        ui.painter().rect_filled(bar, 1.5, pal.accent);
    }
    let fs = if size >= BIG { 22.0 } else { 17.0 };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, FontId::proportional(fs), pal.text);
    resp.on_hover_text(tip)
}

/// A button with a drop-down caret; returns the response to anchor a popup on.
fn drop_button(ui: &mut Ui, size: Vec2, selected: bool, tip: &str, content: impl FnOnce(&egui::Painter, Rect)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let pal = crate::ui::palette_of(ui);
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let bg = if selected || open {
        pal.accent_soft
    } else if resp.hovered() {
        pal.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5.0, bg);
    let inner = Rect::from_min_max(rect.min + vec2(3.0, 3.0), rect.max - vec2(3.0, 14.0));
    content(ui.painter(), inner);
    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - 7.0),
        egui::Align2::CENTER_CENTER,
        ic::CARET_DOWN,
        FontId::proportional(10.0),
        pal.text_dim,
    );
    resp.on_hover_text(tip)
}

fn group(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    let pal = crate::ui::palette_of(ui);
    ui.vertical(|ui| {
        let content = ui
            .horizontal(|ui| {
                ui.set_height(66.0);
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                add(ui);
            })
            .response;
        let label_w = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), FontId::proportional(11.5), Color32::WHITE).size().x);
        let w = content.rect.width().max(label_w + 8.0);
        let (r, _) = ui.allocate_exact_size(vec2(w, 16.0), Sense::hover());
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(11.5), pal.text_dim);
    });
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 80.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 8.0), pos2(r.center().x, r.bottom() - 6.0)], EStroke::new(1.0, pal.border));
}

fn menu_item(ui: &mut Ui, icon: &str, label: &str, shortcut: &str) -> bool {
    let text = format!("{icon}  {label}");
    let b = egui::Button::new(text).shortcut_text(crate::ui::shortcut_label(shortcut)).min_size(vec2(200.0, 26.0));
    ui.add(b).clicked()
}

pub fn toolbar(app: &mut PaintApp, ui: &mut Ui) {
    let pal = app.palette;
    egui::Panel::top("toolbar")
        .exact_size(100.0)
        .frame(egui::Frame::new().fill(pal.toolbar).inner_margin(egui::Margin::symmetric(10, 4)).stroke(EStroke::new(1.0, pal.border)))
        .show(ui, |ui| {
            egui::ScrollArea::horizontal().id_salt("toolbar-scroll").show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 16.0;
                ui.horizontal(|ui| {
                    selection_group(app, ui);
                    image_group(app, ui);
                    tools_group(app, ui);
                    brushes_group(app, ui);
                    let compact = ui.available_width() < 1090.0;
                    shapes_group(app, ui, compact);
                    size_group(app, ui);
                    colors_group(app, ui);
                    layers_group(app, ui);
                });
            });
        });
    if app.tool == Tool::Text || app.text.is_some() {
        text_bar(app, ui);
    }
}

fn selection_group(app: &mut PaintApp, ui: &mut Ui) {
    group(ui, "Selection", |ui| {
        let pal = app.palette;
        let icon = if app.settings.free_form_select { ic::LASSO } else { ic::SELECTION };
        let resp = drop_button(ui, vec2(BIG, 62.0), app.tool == Tool::Select, "Selection", |p, r| {
            p.text(r.center(), egui::Align2::CENTER_CENTER, icon, FontId::proportional(24.0), pal.text);
        });
        if resp.clicked() {
            app.set_tool(Tool::Select);
        }
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(230.0);
            ui.label(egui::RichText::new("Selection shapes").small().color(pal.text_dim));
            if menu_item(ui, ic::SELECTION, &format!("Rectangle{}", if !app.settings.free_form_select { "  ✓" } else { "" }), "") {
                app.settings.free_form_select = false;
                app.set_tool(Tool::Select);
            }
            if menu_item(ui, ic::LASSO, &format!("Free-form{}", if app.settings.free_form_select { "  ✓" } else { "" }), "") {
                app.settings.free_form_select = true;
                app.set_tool(Tool::Select);
            }
            ui.separator();
            ui.label(egui::RichText::new("Selection options").small().color(pal.text_dim));
            if menu_item(ui, ic::SELECTION_ALL, "Select all", "Ctrl+A") {
                app.select_all();
            }
            if menu_item(ui, ic::SELECTION_INVERSE, "Invert selection", "") {
                app.invert_selection();
            }
            if menu_item(ui, ic::TRASH, "Delete", "Del") {
                app.delete_selection();
            }
            let label = format!("Transparent selection{}", if app.settings.transparent_selection { "  ✓" } else { "" });
            if menu_item(ui, ic::SELECTION_BACKGROUND, &label, "") {
                app.settings.transparent_selection = !app.settings.transparent_selection;
                if let Some(s) = app.selection.as_mut() {
                    s.invalidate_key();
                }
                app.invalidate_all();
            }
        });
    });
}

fn image_group(app: &mut PaintApp, ui: &mut Ui) {
    group(ui, "Image", |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            ui.horizontal(|ui| {
                if icon_button(ui, ic::CROP, false, SMALL + 2.0, "Crop (Ctrl+Shift+X)").clicked() {
                    app.crop();
                }
                if icon_button(ui, ic::ARROWS_OUT_SIMPLE, false, SMALL + 2.0, "Resize and skew (Ctrl+W)").clicked() {
                    app.dialog = Some(Dialog::resize(app));
                }
            });
            ui.horizontal(|ui| {
                let pal = app.palette;
                let resp = drop_button(ui, vec2(SMALL + 2.0, SMALL + 2.0), false, "Rotate and flip", |p, r| {
                    p.text(r.center() + vec2(0.0, 4.0), egui::Align2::CENTER_CENTER, ic::ARROW_CLOCKWISE, FontId::proportional(15.0), pal.text);
                });
                rotate_menu(app, &resp);
                if icon_button(ui, ic::CIRCLE_HALF, false, SMALL + 2.0, "Invert colors (Ctrl+Shift+I)").clicked() {
                    app.invert_colors();
                }
            });
        });
    });
}

pub fn rotate_menu(app: &mut PaintApp, resp: &Response) {
    egui::Popup::menu(resp).show(|ui| {
        ui.set_min_width(220.0);
        if menu_item(ui, ic::ARROW_CLOCKWISE, "Rotate right 90°", "") {
            app.rotate(90.0);
        }
        if menu_item(ui, ic::ARROW_COUNTER_CLOCKWISE, "Rotate left 90°", "") {
            app.rotate(270.0);
        }
        if menu_item(ui, ic::ARROWS_CLOCKWISE, "Rotate 180°", "") {
            app.rotate(180.0);
        }
        if menu_item(ui, ic::SLIDERS_HORIZONTAL, "Custom rotation…", "") {
            app.dialog = Some(Dialog::RotateCustom { angle: 15.0 });
        }
        ui.separator();
        if menu_item(ui, ic::FLIP_VERTICAL, "Flip vertical", "") {
            app.flip(false);
        }
        if menu_item(ui, ic::FLIP_HORIZONTAL, "Flip horizontal", "") {
            app.flip(true);
        }
    });
}

fn tools_group(app: &mut PaintApp, ui: &mut Ui) {
    group(ui, "Tools", |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            let row1 = [(Tool::Pencil, ic::PENCIL_SIMPLE, "Pencil (P)"), (Tool::Fill, ic::PAINT_BUCKET, "Fill with color (F)"), (Tool::Text, ic::TEXT_T, "Text (T)")];
            let row2 = [(Tool::Eraser, ic::ERASER, "Eraser (E)"), (Tool::Picker, ic::EYEDROPPER, "Color picker (I)"), (Tool::Magnifier, ic::MAGNIFYING_GLASS, "Magnifier (M)")];
            for row in [row1, row2] {
                ui.horizontal(|ui| {
                    for (t, icon, tip) in row {
                        if icon_button(ui, icon, app.tool == t, SMALL + 2.0, tip).clicked() {
                            app.set_tool(t);
                        }
                    }
                });
            }
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Brush / media previews rendered with the real brush engine.

thread_local! {
    static PREVIEWS: std::cell::RefCell<HashMap<(String, bool), egui::TextureHandle>> = Default::default();
}

fn preview_texture(ctx: &egui::Context, kind: BrushKind, dark: bool) -> egui::TextureHandle {
    let key = (format!("{kind:?}"), dark);
    PREVIEWS.with(|m| {
        if let Some(t) = m.borrow().get(&key) {
            return t.clone();
        }
        let (w, h) = (96u32, 36u32);
        let ink = if dark { Rgba::rgb(235, 235, 235) } else { Rgba::rgb(30, 30, 30) };
        let size = match kind {
            BrushKind::Calligraphy45 | BrushKind::Calligraphy135 => 9.0,
            BrushKind::Brush | BrushKind::Pencil => 4.0,
            BrushKind::Airbrush => 18.0,
            BrushKind::NaturalPencil => 6.0,
            _ => 14.0,
        };
        let mut s = Stroke::new(kind, ink, size, 1.0, w, h);
        for i in 0..=40 {
            let t = i as f32 / 40.0;
            let x = 10.0 + t * 76.0;
            let y = 18.0 + (t * std::f32::consts::TAU).sin() * 7.0;
            s.add_point(x, y);
            if kind == BrushKind::Airbrush {
                s.tick(0.02);
            }
        }
        s.finish();
        let img = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], s.out.data());
        let t = ctx.load_texture(format!("brush-{kind:?}-{dark}"), img, egui::TextureOptions::LINEAR);
        m.borrow_mut().insert(key, t.clone());
        t
    })
}

fn brush_icon(kind: BrushKind) -> &'static str {
    match kind {
        BrushKind::Brush => ic::PAINT_BRUSH,
        BrushKind::Calligraphy45 => ic::PEN_NIB,
        BrushKind::Calligraphy135 => ic::PEN_NIB_STRAIGHT,
        BrushKind::Airbrush => ic::SPRAY_BOTTLE,
        BrushKind::Oil => ic::PAINT_BRUSH_BROAD,
        BrushKind::Crayon => ic::SCRIBBLE,
        BrushKind::Marker => ic::HIGHLIGHTER,
        BrushKind::NaturalPencil => ic::PENCIL_LINE,
        BrushKind::Watercolor => ic::DROP_HALF,
        BrushKind::Pencil => ic::PENCIL_SIMPLE,
        BrushKind::Eraser => ic::ERASER,
    }
}

fn brushes_group(app: &mut PaintApp, ui: &mut Ui) {
    group(ui, "Brushes", |ui| {
        let pal = app.palette;
        let tex = preview_texture(ui.ctx(), app.settings.brush, pal.dark);
        let icon = brush_icon(app.settings.brush);
        let resp = drop_button(ui, vec2(58.0, 62.0), app.tool == Tool::Brush, app.settings.brush.name(), |p, r| {
            p.text(r.center() - vec2(0.0, 6.0), egui::Align2::CENTER_CENTER, icon, FontId::proportional(22.0), pal.text);
            let pr = Rect::from_center_size(pos2(r.center().x, r.bottom() - 4.0), vec2(48.0, 14.0));
            p.image(tex.id(), pr, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        });
        if resp.clicked() {
            app.set_tool(Tool::Brush);
        }
        egui::Popup::menu(&resp).show(|ui| {
            ui.label(egui::RichText::new("Brushes").small().color(pal.text_dim));
            egui::Grid::new("brush-grid").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                for (i, k) in BrushKind::BRUSHES.iter().enumerate() {
                    let tex = preview_texture(ui.ctx(), *k, pal.dark);
                    let sel = app.settings.brush == *k && app.tool == Tool::Brush;
                    let (r, resp) = ui.allocate_exact_size(vec2(104.0, 56.0), Sense::click());
                    let bg = if sel { pal.accent_soft } else if resp.hovered() { pal.hover } else { Color32::TRANSPARENT };
                    ui.painter().rect_filled(r, 6.0, bg);
                    if sel {
                        ui.painter().rect_stroke(r, 6.0, EStroke::new(1.0, pal.accent), egui::StrokeKind::Inside);
                    }
                    let img_r = Rect::from_center_size(pos2(r.center().x, r.top() + 20.0), vec2(96.0, 36.0));
                    ui.painter().image(tex.id(), img_r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                    ui.painter().text(pos2(r.center().x, r.bottom() - 9.0), egui::Align2::CENTER_CENTER, k.name(), FontId::proportional(11.0), pal.text_dim);
                    if resp.clicked() {
                        app.settings.brush = *k;
                        app.set_tool(Tool::Brush);
                        ui.close();
                    }
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
        });
    });
}

// ---------------------------------------------------------------------------------------------

pub fn paint_shape_icon(p: &egui::Painter, r: Rect, kind: ShapeKind, col: Color32) {
    let s = EStroke::new(1.3, col);
    let r = r.shrink(3.0);
    match kind {
        ShapeKind::Line => {
            p.line_segment([r.left_bottom(), r.right_top()], s);
        }
        ShapeKind::Curve => {
            let pts = [r.left_bottom(), pos2(r.left() + r.width() * 0.2, r.top() - 2.0), pos2(r.right() - r.width() * 0.2, r.bottom() + 2.0), r.right_top()];
            p.add(egui::epaint::CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, s));
        }
        ShapeKind::Polygon => {
            let pts = vec![
                pos2(r.left() + r.width() * 0.3, r.top()),
                pos2(r.right(), r.top() + r.height() * 0.35),
                pos2(r.right() - r.width() * 0.15, r.bottom()),
                pos2(r.left() + r.width() * 0.4, r.bottom() - r.height() * 0.3),
                pos2(r.left(), r.bottom()),
                pos2(r.left(), r.top() + r.height() * 0.25),
            ];
            p.add(egui::Shape::closed_line(pts, s));
        }
        k => {
            if let Some(path) = k.path(r.left(), r.top(), r.right(), r.bottom()) {
                for (poly, closed) in shapes::flatten(&path, 0.5) {
                    let pts: Vec<Pos2> = poly.iter().map(|&(x, y)| pos2(x, y)).collect();
                    if closed {
                        p.add(egui::Shape::closed_line(pts, s));
                    } else {
                        p.add(egui::Shape::line(pts, s));
                    }
                }
            }
        }
    }
}

fn media_preview(ui: &mut Ui, m: Media, fill: bool, pal: &crate::ui::theme::Palette) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(200.0, 30.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 5.0, pal.hover);
    }
    let sw = Rect::from_min_size(r.min + vec2(8.0, 5.0), vec2(40.0, 20.0));
    match m {
        Media::None => {
            ui.painter().rect_stroke(sw, 3.0, EStroke::new(1.0, pal.text_dim), egui::StrokeKind::Inside);
            ui.painter().line_segment([sw.left_bottom(), sw.right_top()], EStroke::new(1.0, Color32::from_rgb(220, 50, 50)));
        }
        _ => {
            let tex = preview_texture(ui.ctx(), if m == Media::Solid { BrushKind::Brush } else { m.brush() }, pal.dark);
            ui.painter().image(tex.id(), sw.expand2(vec2(4.0, 2.0)), Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
    let name = if fill && m == Media::None { "No fill" } else { m.name() };
    ui.painter().text(pos2(sw.right() + 14.0, r.center().y), egui::Align2::LEFT_CENTER, name, FontId::proportional(13.0), pal.text);
    resp
}

fn shapes_group(app: &mut PaintApp, ui: &mut Ui, compact: bool) {
    group(ui, "Shapes", |ui| {
        let pal = app.palette;
        if compact {
            let kind = app.settings.shape;
            let resp = drop_button(ui, vec2(BIG, 62.0), app.tool == Tool::Shape, "Shapes", |p, r| {
                paint_shape_icon(p, Rect::from_center_size(r.center(), vec2(26.0, 26.0)), kind, pal.text);
            });
            if resp.clicked() {
                app.set_tool(Tool::Shape);
            }
            egui::Popup::menu(&resp).show(|ui| {
                ui.label(egui::RichText::new("Shapes").small().color(pal.text_dim));
                egui::Grid::new("shape-pop").spacing(vec2(2.0, 2.0)).min_col_width(0.0).show(ui, |ui| {
                    for (i, k) in ShapeKind::ALL.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                        let sel = app.settings.shape == *k;
                        if sel || resp.hovered() {
                            ui.painter().rect_filled(r, 4.0, if sel { pal.accent_soft } else { pal.hover });
                        }
                        paint_shape_icon(ui.painter(), r.shrink(5.0), *k, if sel { pal.accent } else { pal.text });
                        if resp.on_hover_text(k.name()).clicked() {
                            app.settings.shape = *k;
                            app.set_tool(Tool::Shape);
                            ui.close();
                        }
                        if i % 6 == 5 {
                            ui.end_row();
                        }
                    }
                });
            });
            shape_media_buttons(app, ui);
            return;
        }
        let cols = 8;
        let cell = 21.0;
        let (rect, _) = ui.allocate_exact_size(vec2(cols as f32 * cell + 4.0, 3.0 * cell + 2.0), Sense::hover());
        ui.painter().rect_stroke(rect, 5.0, EStroke::new(1.0, pal.border), egui::StrokeKind::Inside);
        for (i, k) in ShapeKind::ALL.iter().enumerate() {
            let (c, row) = (i % cols, i / cols);
            let r = Rect::from_min_size(rect.min + vec2(2.0 + c as f32 * cell, 1.0 + row as f32 * cell), vec2(cell, cell));
            let resp = ui.interact(r, ui.scope_id().with(("shape", i)), Sense::click()).on_hover_text(k.name());
            let sel = app.tool == Tool::Shape && app.settings.shape == *k;
            if sel || resp.hovered() {
                ui.painter().rect_filled(r.shrink(1.0), 3.0, if sel { pal.accent_soft } else { pal.hover });
            }
            paint_shape_icon(ui.painter(), r.shrink(2.0), *k, if sel { pal.accent } else { pal.text });
            if resp.clicked() {
                app.settings.shape = *k;
                app.set_tool(Tool::Shape);
            }
        }
        shape_media_buttons(app, ui);
    });
}

fn shape_media_buttons(app: &mut PaintApp, ui: &mut Ui) {
    let pal = app.palette;
    {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            let outline = app.settings.outline;
            let resp = drop_button(ui, vec2(SMALL + 6.0, SMALL + 2.0), false, "Shape outline", |p, r| {
                let rr = Rect::from_center_size(r.center() + vec2(0.0, 3.0), vec2(14.0, 11.0));
                p.rect_stroke(rr, 2.0, EStroke::new(if outline == Media::None { 1.0 } else { 2.0 }, if outline == Media::None { pal.text_dim } else { pal.text }), egui::StrokeKind::Middle);
            });
            egui::Popup::menu(&resp).show(|ui| {
                ui.label(egui::RichText::new("Shape outline").small().color(pal.text_dim));
                for m in Media::ALL {
                    if media_preview(ui, m, false, &pal).clicked() {
                        app.settings.outline = m;
                        if let Some(s) = app.shape.as_mut() {
                            s.params.outline = m;
                        }
                        ui.close();
                    }
                }
            });
            let fill = app.settings.fill;
            let fill_col = app.settings.secondary.to_egui();
            let resp = drop_button(ui, vec2(SMALL + 6.0, SMALL + 2.0), false, "Shape fill", |p, r| {
                let rr = Rect::from_center_size(r.center() + vec2(0.0, 3.0), vec2(14.0, 11.0));
                if fill != Media::None {
                    p.rect_filled(rr, 2.0, fill_col);
                }
                p.rect_stroke(rr, 2.0, EStroke::new(1.0, pal.text_dim), egui::StrokeKind::Middle);
            });
            egui::Popup::menu(&resp).show(|ui| {
                ui.label(egui::RichText::new("Shape fill").small().color(pal.text_dim));
                for m in Media::ALL {
                    if media_preview(ui, m, true, &pal).clicked() {
                        app.settings.fill = m;
                        if let Some(s) = app.shape.as_mut() {
                            s.params.fill = m;
                        }
                        ui.close();
                    }
                }
            });
        });
    }
}

fn size_group(app: &mut PaintApp, ui: &mut Ui) {
    group(ui, "Size", |ui| {
        let pal = app.palette;
        let kind = app.tool.size_kind(&app.settings);
        let shape_mode = app.tool == Tool::Shape || app.shape.is_some();
        let (cur, lo, hi) = match kind {
            Some(k) => {
                let (lo, hi) = k.size_range();
                (app.settings.size(k), lo, hi)
            }
            None => (app.settings.shape_size, 1.0, 50.0),
        };
        let enabled = kind.is_some() || shape_mode;
        let resp = drop_button(ui, vec2(BIG, 62.0), false, "Size", |p, r| {
            let col = if enabled { pal.text } else { pal.text_dim.gamma_multiply(0.5) };
            for (i, w) in [1.0f32, 2.5, 4.0, 6.0].iter().enumerate() {
                let y = r.top() + 6.0 + i as f32 * 10.0;
                p.line_segment([pos2(r.left() + 6.0, y), pos2(r.right() - 6.0, y)], EStroke::new(*w, col));
            }
        });
        if !enabled {
            return;
        }
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(240.0);
            ui.label(egui::RichText::new("Size").small().color(pal.text_dim));
            let mut v = cur;
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut v, lo..=hi).logarithmic(true).show_value(false));
                ui.add(egui::DragValue::new(&mut v).range(lo..=hi).suffix(" px").speed(0.5));
            });
            ui.horizontal_wrapped(|ui| {
                for preset in [1.0, 3.0, 5.0, 8.0, 12.0, 20.0, 40.0, 80.0] {
                    if preset >= lo && preset <= hi && ui.selectable_label((v - preset).abs() < 0.5, format!("{preset:.0}")).clicked() {
                        v = preset;
                    }
                }
            });
            if (v - cur).abs() > f32::EPSILON {
                match kind {
                    Some(k) => app.settings.set_size(k, v),
                    None => {
                        app.settings.shape_size = v.round().clamp(1.0, 50.0);
                        if let Some(s) = app.shape.as_mut() {
                            s.params.size = app.settings.shape_size;
                        }
                    }
                }
            }
            if let Some(k) = kind.filter(|k| *k != BrushKind::Eraser) {
                ui.separator();
                ui.label(egui::RichText::new("Opacity").small().color(pal.text_dim));
                let mut o = app.settings.opacity(k) * 100.0;
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut o, 1.0..=100.0).show_value(false));
                    ui.add(egui::DragValue::new(&mut o).range(1.0..=100.0).suffix(" %"));
                });
                app.settings.set_opacity(k, o / 100.0);
            }
        });
    });
}

fn swatch(ui: &mut Ui, c: Rgba, size: f32, ring: bool, tip: &str) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let pal = crate::ui::palette_of(ui);
    let p = ui.painter();
    if c.a < 255 {
        p.rect_filled(r, size / 2.0, Color32::WHITE);
    }
    p.circle_filled(r.center(), size / 2.0 - 1.0, c.to_egui());
    p.circle_stroke(r.center(), size / 2.0 - 1.0, EStroke::new(1.0, if pal.dark { Color32::from_white_alpha(50) } else { Color32::from_black_alpha(45) }));
    if ring {
        p.circle_stroke(r.center(), size / 2.0 + 1.5, EStroke::new(2.0, pal.accent));
    } else if resp.hovered() {
        p.circle_stroke(r.center(), size / 2.0 + 1.0, EStroke::new(1.5, pal.text_dim));
    }
    if tip.is_empty() { resp } else { resp.on_hover_text(tip) }
}

fn colors_group(app: &mut PaintApp, ui: &mut Ui) {
    group(ui, "Colors", |ui| {
        let pal = app.palette;
        let slot = active_slot();
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            ui.add_space(2.0);
            if swatch(ui, app.settings.primary, 30.0, slot == Slot::Primary, "Color 1 (primary): click, then pick a color").clicked() {
                set_slot(Slot::Primary);
            }
            ui.add_space(2.0);
            if swatch(ui, app.settings.secondary, 24.0, slot == Slot::Secondary, "Color 2 (secondary)").clicked() {
                set_slot(Slot::Secondary);
            }
        });
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 3.0);
            let pick = |app: &mut PaintApp, c: Rgba, secondary: bool| {
                let to_secondary = secondary ^ (active_slot() == Slot::Secondary);
                if to_secondary {
                    app.settings.secondary = c;
                } else {
                    app.settings.primary = c;
                }
                if let Some(s) = app.shape.as_mut() {
                    s.params.outline_color = app.settings.primary;
                    s.params.fill_color = app.settings.secondary;
                }
                if let Some(s) = app.selection.as_mut() {
                    s.invalidate_key();
                }
            };
            for row in 0..2 {
                ui.horizontal(|ui| {
                    for col in 0..10 {
                        let c = DEFAULT_PALETTE[row * 10 + col];
                        let r = swatch(ui, c, 18.0, false, &c.hex());
                        if r.clicked() {
                            pick(app, c, false);
                        }
                        if r.secondary_clicked() {
                            pick(app, c, true);
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                for i in 0..10 {
                    match app.settings.custom_colors.get(i).copied() {
                        Some(c) => {
                            let r = swatch(ui, c, 18.0, false, &c.hex());
                            if r.clicked() {
                                pick(app, c, false);
                            }
                            if r.secondary_clicked() {
                                pick(app, c, true);
                            }
                        }
                        None => {
                            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            ui.painter().circle_stroke(r.center(), 7.5, EStroke::new(1.0, pal.border));
                        }
                    }
                }
            });
        });
        ui.add_space(4.0);
        let (r, resp) = ui.allocate_exact_size(vec2(36.0, 62.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 5.0, pal.hover);
        }
        // A colour wheel drawn from hue segments.
        let c = pos2(r.center().x, r.top() + 22.0);
        for i in 0..36 {
            let a0 = i as f32 * 10.0;
            let col = Rgba::from_hsv(a0, 0.85, 0.95).to_egui();
            let (s0, s1) = ((a0 - 1.0).to_radians(), (a0 + 11.0).to_radians());
            let pts = vec![c, c + vec2(s0.cos(), s0.sin()) * 12.0, c + vec2(s1.cos(), s1.sin()) * 12.0];
            ui.painter().add(egui::Shape::convex_polygon(pts, col, EStroke::NONE));
        }
        ui.painter().circle_filled(c, 4.0, pal.toolbar);
        ui.painter().text(pos2(r.center().x, r.bottom() - 10.0), egui::Align2::CENTER_CENTER, ic::PLUS, FontId::proportional(13.0), pal.text_dim);
        if resp.on_hover_text("Edit colors").clicked() {
            app.dialog = Some(Dialog::edit_colors(app, active_slot() == Slot::Secondary));
        }
    });
}

fn layers_group(app: &mut PaintApp, ui: &mut Ui) {
    group(ui, "Layers", |ui| {
        if icon_button(ui, ic::STACK, app.settings.show_layers, BIG, "Layers").clicked() {
            app.settings.show_layers = !app.settings.show_layers;
        }
    });
}

fn text_bar(app: &mut PaintApp, ui: &mut Ui) {
    let pal = app.palette;
    egui::Panel::top("textbar")
        .exact_size(40.0)
        .frame(egui::Frame::new().fill(pal.toolbar).inner_margin(egui::Margin::symmetric(12, 6)).stroke(EStroke::new(1.0, pal.border)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let before = app.settings.text.clone();
                let ts = &mut app.settings.text;
                let fam_label = if ts.family.is_empty() { app.text_engine.default_family.clone() } else { ts.family.clone() };
                ui.label(egui::RichText::new(ic::TEXT_T).size(16.0));
                egui::ComboBox::from_id_salt("font-family").selected_text(fam_label).width(200.0).height(400.0).show_ui(ui, |ui| {
                    for f in &app.text_engine.families {
                        ui.selectable_value(&mut ts.family, f.clone(), f.as_str());
                    }
                });
                egui::ComboBox::from_id_salt("font-size").selected_text(format!("{}", ts.size_pt)).width(56.0).show_ui(ui, |ui| {
                    for s in [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0] {
                        ui.selectable_value(&mut ts.size_pt, s, format!("{s}"));
                    }
                });
                ui.add(egui::DragValue::new(&mut ts.size_pt).range(1.0..=999.0).speed(0.25));
                ui.separator();
                for (flag, icon, tip) in [
                    (&mut ts.bold, ic::TEXT_B, "Bold (Ctrl+B)"),
                    (&mut ts.italic, ic::TEXT_ITALIC, "Italic (Ctrl+I)"),
                    (&mut ts.underline, ic::TEXT_UNDERLINE, "Underline (Ctrl+U)"),
                    (&mut ts.strikeout, ic::TEXT_STRIKETHROUGH, "Strikethrough"),
                ] {
                    if icon_button(ui, icon, *flag, 28.0, tip).clicked() {
                        *flag = !*flag;
                    }
                }
                ui.separator();
                for (a, icon, tip) in [(0u8, ic::TEXT_ALIGN_LEFT, "Align left"), (1, ic::TEXT_ALIGN_CENTER, "Center"), (2, ic::TEXT_ALIGN_RIGHT, "Align right")] {
                    if icon_button(ui, icon, ts.align == a, 28.0, tip).clicked() {
                        ts.align = a;
                    }
                }
                ui.separator();
                ui.checkbox(&mut ts.fill_background, "Fill background");
                if *ts != before {
                    if let Some(t) = app.text.as_mut() {
                        t.layout(&mut app.text_engine, &app.settings.text, app.doc.dpi);
                    }
                    app.invalidate_all();
                }
            });
        });
}
