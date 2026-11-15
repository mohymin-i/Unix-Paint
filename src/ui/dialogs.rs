//! Modal dialogs: save prompt, image properties, resize & skew, custom rotation, edit colours,
//! print / page setup, about, keyboard shortcuts, layer rename and messages.

use crate::app::{After, PaintApp};
use crate::color::Rgba;
use crate::icons as ic;
use crate::settings::Units;
use egui::{Color32, Pos2, Sense, Stroke as EStroke, Ui, pos2, vec2};

pub enum Dialog {
    Message { title: String, text: String },
    Unsaved { after: After },
    About,
    Shortcuts,
    Properties(Props),
    Resize(ResizeState),
    RotateCustom { angle: f32 },
    EditColors(ColorState),
    Print(PrintState),
    RenameLayer { index: usize, name: String },
}

pub struct Props {
    pub w_px: f32,
    pub h_px: f32,
    pub units: Units,
    pub dpi: f32,
    pub mono: bool,
    pub transparent: bool,
}

pub struct ResizeState {
    pub by_percent: bool,
    pub w: f32,
    pub h: f32,
    pub keep: bool,
    pub skew_h: f32,
    pub skew_v: f32,
    base: (f32, f32),
}

pub struct ColorState {
    pub h: f32,
    pub s: f32,
    pub v: f32,
    pub hex: String,
    pub secondary: bool,
    original: Rgba,
}

pub struct PrintState {
    pub printers: Vec<String>,
    pub printer: usize,
    pub copies: u32,
    pub landscape: bool,
    pub fit: bool,
    pub scale: f32,
    pub center_h: bool,
    pub center_v: bool,
    pub margin_mm: f32,
}

impl Dialog {
    pub fn properties(app: &PaintApp) -> Dialog {
        Dialog::Properties(Props {
            w_px: app.doc.width as f32,
            h_px: app.doc.height as f32,
            units: app.settings.units,
            dpi: app.doc.dpi,
            mono: app.doc.monochrome,
            transparent: !app.doc.background_visible,
        })
    }

    pub fn resize(app: &PaintApp) -> Dialog {
        let base = match &app.selection {
            Some(s) => (s.dest.w as f32, s.dest.h as f32),
            None => (app.doc.width as f32, app.doc.height as f32),
        };
        Dialog::Resize(ResizeState { by_percent: true, w: 100.0, h: 100.0, keep: true, skew_h: 0.0, skew_v: 0.0, base })
    }

    pub fn edit_colors(app: &PaintApp, secondary: bool) -> Dialog {
        let c = if secondary { app.settings.secondary } else { app.settings.primary };
        let (h, s, v) = c.to_hsv();
        Dialog::EditColors(ColorState { h, s, v, hex: c.hex(), secondary, original: c })
    }

    pub fn print(app: &PaintApp) -> Dialog {
        let printers = crate::platform::list_printers();
        let w = app.doc.width as f32;
        let h = app.doc.height as f32;
        Dialog::Print(PrintState {
            printers,
            printer: 0,
            copies: 1,
            landscape: w > h,
            fit: true,
            scale: 100.0,
            center_h: true,
            center_v: true,
            margin_mm: 19.0,
        })
    }
}

fn units_per_px(u: Units, dpi: f32) -> f32 {
    match u {
        Units::Pixels => 1.0,
        Units::Inches => 1.0 / dpi,
        Units::Centimeters => 2.54 / dpi,
    }
}

fn footer(ui: &mut Ui, primary: &str) -> (bool, bool) {
    ui.add_space(10.0);
    let mut ok = false;
    let mut cancel = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.add(egui::Button::new("Cancel").min_size(vec2(96.0, 30.0))).clicked() {
            cancel = true;
        }
        let pal = crate::ui::palette_of(ui);
        let b = egui::Button::new(egui::RichText::new(primary).color(on_accent(&pal))).fill(pal.accent).min_size(vec2(96.0, 30.0));
        if ui.add(b).clicked() {
            ok = true;
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ok = true;
    }
    (ok, cancel)
}

/// Text colour on an accent-filled button (dark text on the light dark-mode accent).
fn on_accent(p: &crate::ui::theme::Palette) -> Color32 {
    if p.dark { Color32::BLACK } else { Color32::WHITE }
}

fn title(ui: &mut Ui, t: &str) {
    ui.label(egui::RichText::new(t).heading());
    ui.add_space(8.0);
}

pub fn show(app: &mut PaintApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.dialog.take() else { return };
    let mut keep = true;
    let pal = app.palette;
    let frame = egui::Frame::new().fill(pal.panel).corner_radius(10.0).inner_margin(egui::Margin::same(22)).stroke(EStroke::new(1.0, pal.border));
    let modal = egui::Modal::new(egui::Id::unique("dialog")).frame(frame);
    let resp = modal.show(ctx, |ui| {
        ui.set_min_width(340.0);
        ui.set_max_width(if matches!(dialog, Dialog::EditColors(_)) { 560.0 } else { 440.0 });
        match &mut dialog {
            Dialog::Message { title: t, text } => {
                title(ui, t);
                ui.label(text.as_str());
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("OK").min_size(vec2(96.0, 30.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        keep = false;
                    }
                });
            }
            Dialog::Unsaved { after } => {
                title(ui, "Paint");
                ui.label(format!("Do you want to save changes to {}?", app.doc.title()));
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(90.0, 30.0))).clicked() {
                        keep = false;
                    }
                    if ui.add(egui::Button::new("Don't save").min_size(vec2(90.0, 30.0))).clicked() {
                        keep = false;
                        let a = after.clone();
                        app.history.mark_saved();
                        app.proceed(a);
                    }
                    let b = egui::Button::new(egui::RichText::new("Save").color(on_accent(&pal))).fill(pal.accent).min_size(vec2(90.0, 30.0));
                    if ui.add(b).clicked() {
                        keep = false;
                        let a = after.clone();
                        if app.save() {
                            app.proceed(a);
                        }
                    }
                });
            }
            Dialog::About => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ic::PALETTE).size(40.0).color(pal.accent));
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Paint for Unix").heading());
                        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    });
                });
                ui.add_space(8.0);
                ui.label("A from-scratch rebuild of Windows Paint for Linux and macOS, written in Rust.");
                ui.label(egui::RichText::new("Not affiliated with Microsoft. Icons: Phosphor (MIT).").small().color(pal.text_dim));
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("OK").min_size(vec2(96.0, 30.0))).clicked() {
                        keep = false;
                    }
                });
            }
            Dialog::Shortcuts => {
                title(ui, "Keyboard shortcuts");
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("shortcuts").striped(true).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
                        for (k, d) in crate::ui::shortcuts::LIST {
                            ui.label(egui::RichText::new(crate::ui::shortcut_label(k)).monospace());
                            ui.label(*d);
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("Close").min_size(vec2(96.0, 30.0))).clicked() {
                        keep = false;
                    }
                });
            }
            Dialog::Properties(p) => {
                title(ui, "Image properties");
                ui.label(egui::RichText::new(format!("Resolution: {:.0} DPI", p.dpi)).color(pal.text_dim));
                ui.add_space(6.0);
                egui::Grid::new("props").spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    ui.label("Units");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut p.units, Units::Inches, "Inches");
                        ui.radio_value(&mut p.units, Units::Centimeters, "Centimeters");
                        ui.radio_value(&mut p.units, Units::Pixels, "Pixels");
                    });
                    ui.end_row();
                    let k = units_per_px(p.units, p.dpi);
                    let dec = if p.units == Units::Pixels { 0 } else { 2 };
                    let mut w = p.w_px * k;
                    let mut h = p.h_px * k;
                    ui.label("Width");
                    if ui.add(egui::DragValue::new(&mut w).range(k..=crate::doc::MAX_DIM as f32 * k).fixed_decimals(dec).speed(k)).changed() {
                        p.w_px = (w / k).round().max(1.0);
                    }
                    ui.end_row();
                    ui.label("Height");
                    if ui.add(egui::DragValue::new(&mut h).range(k..=crate::doc::MAX_DIM as f32 * k).fixed_decimals(dec).speed(k)).changed() {
                        p.h_px = (h / k).round().max(1.0);
                    }
                    ui.end_row();
                    ui.label("Colors");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut p.mono, true, "Black and white");
                        ui.radio_value(&mut p.mono, false, "Color");
                    });
                    ui.end_row();
                    ui.label("Transparency");
                    ui.checkbox(&mut p.transparent, "Transparent background");
                    ui.end_row();
                });
                ui.add_space(4.0);
                if ui.button("Default").clicked() {
                    p.w_px = crate::doc::DEFAULT_WIDTH as f32;
                    p.h_px = crate::doc::DEFAULT_HEIGHT as f32;
                }
                let (ok, cancel) = footer(ui, "OK");
                if ok {
                    keep = false;
                    app.commit_pending();
                    app.settings.units = p.units;
                    let (w, h) = (p.w_px as u32, p.h_px as u32);
                    let mono = p.mono;
                    let transparent = p.transparent;
                    if (w, h) != (app.doc.width, app.doc.height) || mono != app.doc.monochrome || transparent == app.doc.background_visible {
                        app.history.doc_edit(&mut app.doc, "Properties", |d| {
                            d.resize_canvas(w, h);
                            d.background_visible = !transparent;
                            if mono && !d.monochrome {
                                for l in &mut d.layers {
                                    crate::raster::to_monochrome(l.pixels_mut());
                                }
                            }
                            d.monochrome = mono;
                        });
                        app.invalidate_all();
                    }
                    app.settings.new_width = w;
                    app.settings.new_height = h;
                } else if cancel {
                    keep = false;
                }
            }
            Dialog::Resize(r) => {
                title(ui, "Resize and skew");
                ui.label(egui::RichText::new("Resize").strong());
                ui.horizontal(|ui| {
                    let by_pct = r.by_percent;
                    if ui.radio(by_pct, "Percentage").clicked() && !by_pct {
                        r.by_percent = true;
                        r.w = r.w / r.base.0 * 100.0;
                        r.h = r.h / r.base.1 * 100.0;
                    }
                    if ui.radio(!by_pct, "Pixels").clicked() && by_pct {
                        r.by_percent = false;
                        r.w = (r.base.0 * r.w / 100.0).round();
                        r.h = (r.base.1 * r.h / 100.0).round();
                    }
                });
                let (unit, max_w, max_h) = if r.by_percent {
                    ("%", crate::doc::MAX_DIM as f32 / r.base.0 * 100.0, crate::doc::MAX_DIM as f32 / r.base.1 * 100.0)
                } else {
                    ("px", crate::doc::MAX_DIM as f32, crate::doc::MAX_DIM as f32)
                };
                egui::Grid::new("resize").spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    ui.label(format!("{}  Horizontal", ic::ARROWS_HORIZONTAL));
                    let ch = ui.add(egui::DragValue::new(&mut r.w).range(1.0..=max_w).suffix(unit).fixed_decimals(0)).changed();
                    ui.end_row();
                    ui.label(format!("{}  Vertical", ic::ARROWS_VERTICAL));
                    let cv = ui.add(egui::DragValue::new(&mut r.h).range(1.0..=max_h).suffix(unit).fixed_decimals(0)).changed();
                    ui.end_row();
                    if r.keep {
                        let aspect = if r.by_percent { 1.0 } else { r.base.0 / r.base.1 };
                        if ch {
                            r.h = (r.w / aspect).round().max(1.0);
                        } else if cv {
                            r.w = (r.h * aspect).round().max(1.0);
                        }
                    }
                });
                ui.checkbox(&mut r.keep, "Maintain aspect ratio");
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Skew (degrees)").strong());
                egui::Grid::new("skew").spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    ui.label("Horizontal");
                    ui.add(egui::DragValue::new(&mut r.skew_h).range(-89.0..=89.0).suffix("°"));
                    ui.end_row();
                    ui.label("Vertical");
                    ui.add(egui::DragValue::new(&mut r.skew_v).range(-89.0..=89.0).suffix("°"));
                    ui.end_row();
                });
                let (ok, cancel) = footer(ui, "OK");
                if ok {
                    keep = false;
                    let (w, h) = if r.by_percent { (r.base.0 * r.w / 100.0, r.base.1 * r.h / 100.0) } else { (r.w, r.h) };
                    app.resize_image(w.round().max(1.0) as u32, h.round().max(1.0) as u32, r.skew_h, r.skew_v);
                } else if cancel {
                    keep = false;
                }
            }
            Dialog::RotateCustom { angle } => {
                title(ui, "Custom rotation");
                ui.horizontal(|ui| {
                    ui.label("Angle");
                    ui.add(egui::Slider::new(angle, -180.0..=180.0).suffix("°"));
                });
                ui.label(egui::RichText::new("Positive values rotate clockwise. The canvas grows to fit.").small().color(pal.text_dim));
                let (ok, cancel) = footer(ui, "Rotate");
                if ok {
                    keep = false;
                    let a = *angle;
                    app.rotate(a);
                } else if cancel {
                    keep = false;
                }
            }
            Dialog::EditColors(c) => {
                title(ui, "Edit colors");
                ui.horizontal(|ui| {
                    sv_square(ui, c);
                    hue_bar(ui, c);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        let cur = Rgba::from_hsv(c.h, c.s, c.v);
                        let (r, _) = ui.allocate_exact_size(vec2(120.0, 44.0), Sense::hover());
                        let half = r.width() / 2.0;
                        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, vec2(half, r.height())), egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, c.original.to_egui());
                        ui.painter().rect_filled(egui::Rect::from_min_size(r.min + vec2(half, 0.0), vec2(half, r.height())), egui::CornerRadius { nw: 0, sw: 0, ne: 6, se: 6 }, cur.to_egui());
                        ui.add_space(6.0);
                        let mut rgb = [cur.r as i32, cur.g as i32, cur.b as i32];
                        let mut changed = false;
                        egui::Grid::new("rgb").spacing(vec2(8.0, 6.0)).show(ui, |ui| {
                            for (i, n) in ["Red", "Green", "Blue"].iter().enumerate() {
                                ui.label(*n);
                                changed |= ui.add(egui::DragValue::new(&mut rgb[i]).range(0..=255)).changed();
                                ui.end_row();
                            }
                            ui.label("Hex");
                            let r = ui.add(egui::TextEdit::singleline(&mut c.hex).desired_width(84.0));
                            if r.changed() {
                                if let Some(p) = Rgba::parse_hex(&c.hex) {
                                    let (h, s, v) = p.to_hsv();
                                    c.h = h;
                                    c.s = s;
                                    c.v = v;
                                }
                            } else if !r.has_focus() {
                                c.hex = Rgba::from_hsv(c.h, c.s, c.v).hex();
                            }
                            ui.end_row();
                        });
                        if changed {
                            let p = Rgba::rgb(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8);
                            let (h, s, v) = p.to_hsv();
                            if s > 0.0 && v > 0.0 {
                                c.h = h;
                            }
                            c.s = s;
                            c.v = v;
                        }
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut c.secondary, false, "Color 1");
                            ui.radio_value(&mut c.secondary, true, "Color 2");
                        });
                    });
                });
                let (ok, cancel) = footer(ui, "Done");
                if ok {
                    keep = false;
                    let col = Rgba::from_hsv(c.h, c.s, c.v);
                    if c.secondary {
                        app.settings.secondary = col;
                    } else {
                        app.settings.primary = col;
                    }
                    app.settings.add_custom_color(col);
                    if let Some(s) = app.shape.as_mut() {
                        s.params.outline_color = app.settings.primary;
                        s.params.fill_color = app.settings.secondary;
                    }
                } else if cancel {
                    keep = false;
                }
            }
            Dialog::Print(p) => {
                title(ui, "Print");
                egui::Grid::new("print").spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                    ui.label("Printer");
                    if p.printers.is_empty() {
                        ui.label(egui::RichText::new("No printers found (CUPS)").color(pal.text_dim));
                    } else {
                        egui::ComboBox::from_id_salt("printer").selected_text(p.printers[p.printer].clone()).show_ui(ui, |ui| {
                            for (i, n) in p.printers.iter().enumerate() {
                                ui.selectable_value(&mut p.printer, i, n);
                            }
                        });
                    }
                    ui.end_row();
                    ui.label("Copies");
                    ui.add(egui::DragValue::new(&mut p.copies).range(1..=99));
                    ui.end_row();
                    ui.label("Orientation");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut p.landscape, false, "Portrait");
                        ui.radio_value(&mut p.landscape, true, "Landscape");
                    });
                    ui.end_row();
                    ui.label("Scaling");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut p.fit, true, "Fit to page");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut p.fit, false, "Adjust to");
                            ui.add_enabled(!p.fit, egui::DragValue::new(&mut p.scale).range(10.0..=400.0).suffix("% normal size"));
                        });
                    });
                    ui.end_row();
                    ui.label("Centering");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut p.center_h, "Horizontal");
                        ui.checkbox(&mut p.center_v, "Vertical");
                    });
                    ui.end_row();
                    ui.label("Margins");
                    ui.add(egui::DragValue::new(&mut p.margin_mm).range(0.0..=50.0).suffix(" mm"));
                    ui.end_row();
                });
                ui.add_space(6.0);
                print_preview(ui, app, p);
                let (ok, cancel) = footer(ui, "Print");
                if ok {
                    keep = false;
                    let printer = p.printers.get(p.printer).cloned();
                    let opts = crate::platform::PrintOptions {
                        printer,
                        copies: p.copies,
                        landscape: p.landscape,
                        fit: p.fit,
                        scale: p.scale,
                        center: p.center_h || p.center_v,
                        margin_mm: p.margin_mm,
                    };
                    app.commit_pending();
                    match crate::platform::print(&app.doc.flatten(), &app.doc.title(), app.doc.dpi, &opts) {
                        Ok(()) => app.notify("Sent to the printer."),
                        Err(e) => app.error(format!("Couldn't print.\n{e:#}")),
                    }
                } else if cancel {
                    keep = false;
                }
            }
            Dialog::RenameLayer { index, name } => {
                title(ui, "Rename layer");
                let r = ui.add(egui::TextEdit::singleline(name).desired_width(280.0));
                r.request_focus();
                let (ok, cancel) = footer(ui, "Rename");
                if ok {
                    keep = false;
                    let (i, n) = (*index, name.clone());
                    if i < app.doc.layers.len() && !n.trim().is_empty() {
                        app.history.doc_edit(&mut app.doc, "Rename layer", |d| d.layers[i].name = n);
                    }
                } else if cancel {
                    keep = false;
                }
            }
        }
    });
    if resp.should_close() && !matches!(dialog, Dialog::Unsaved { .. }) {
        keep = false;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        keep = false;
    }
    if keep && app.dialog.is_none() {
        app.dialog = Some(dialog);
    }
}

fn print_preview(ui: &mut Ui, app: &mut PaintApp, p: &PrintState) {
    let pal = app.palette;
    let (pw, ph): (f32, f32) = if p.landscape { (297.0, 210.0) } else { (210.0, 297.0) };
    let k = 150.0 / pw.max(ph);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 160.0), Sense::hover());
    let page = egui::Rect::from_center_size(r.center(), vec2(pw * k, ph * k));
    ui.painter().rect_filled(page.translate(vec2(2.0, 3.0)), 2.0, Color32::from_black_alpha(40));
    ui.painter().rect_filled(page, 2.0, Color32::WHITE);
    ui.painter().rect_stroke(page, 2.0, EStroke::new(1.0, pal.border), egui::StrokeKind::Outside);
    let m = p.margin_mm * k;
    let area = page.shrink(m);
    let (iw, ih) = (app.doc.width as f32, app.doc.height as f32);
    let s = if p.fit {
        (area.width() / iw).min(area.height() / ih)
    } else {
        (iw / app.doc.dpi * 25.4 * k * p.scale / 100.0) / iw
    };
    let size = vec2(iw * s, ih * s);
    let x = if p.center_h { area.center().x - size.x / 2.0 } else { area.left() };
    let y = if p.center_v { area.center().y - size.y / 2.0 } else { area.top() };
    let img = egui::Rect::from_min_size(pos2(x, y), size).intersect(area);
    let tex = app.thumbnail_texture(ui.ctx());
    ui.painter().with_clip_rect(area).image(tex.id(), egui::Rect::from_min_size(pos2(x, y), size), egui::Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
    let _ = img;
}

fn sv_square(ui: &mut Ui, c: &mut ColorState) {
    let size = vec2(200.0, 200.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let n = 24;
    let mut mesh = egui::Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            let (s, v) = (i as f32 / n as f32, 1.0 - j as f32 / n as f32);
            let col = Rgba::from_hsv(c.h, s, v).to_egui();
            mesh.colored_vertex(pos2(r.left() + s * r.width(), r.top() + (1.0 - v) * r.height()), col);
        }
    }
    for j in 0..n {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            let b = a + 1;
            let cc = a + (n + 1) as u32;
            let d = cc + 1;
            mesh.add_triangle(a, b, d);
            mesh.add_triangle(a, d, cc);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    if let Some(p) = resp.interact_pointer_pos() {
        c.s = ((p.x - r.left()) / r.width()).clamp(0.0, 1.0);
        c.v = 1.0 - ((p.y - r.top()) / r.height()).clamp(0.0, 1.0);
        c.hex = Rgba::from_hsv(c.h, c.s, c.v).hex();
    }
    let p = pos2(r.left() + c.s * r.width(), r.top() + (1.0 - c.v) * r.height());
    ui.painter().circle_stroke(p, 6.0, EStroke::new(2.0, Color32::WHITE));
    ui.painter().circle_stroke(p, 7.0, EStroke::new(1.0, Color32::BLACK));
}

fn hue_bar(ui: &mut Ui, c: &mut ColorState) {
    let (r, resp) = ui.allocate_exact_size(vec2(22.0, 200.0), Sense::click_and_drag());
    let n = 36;
    let mut mesh = egui::Mesh::default();
    for j in 0..=n {
        let h = j as f32 / n as f32 * 360.0;
        let col = Rgba::from_hsv(h, 1.0, 1.0).to_egui();
        let y = r.top() + j as f32 / n as f32 * r.height();
        mesh.colored_vertex(pos2(r.left(), y), col);
        mesh.colored_vertex(pos2(r.right(), y), col);
    }
    for j in 0..n as u32 {
        let a = j * 2;
        mesh.add_triangle(a, a + 1, a + 3);
        mesh.add_triangle(a, a + 3, a + 2);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    if let Some(p) = resp.interact_pointer_pos() {
        c.h = ((p.y - r.top()) / r.height()).clamp(0.0, 0.9999) * 360.0;
        c.hex = Rgba::from_hsv(c.h, c.s, c.v).hex();
    }
    let y = r.top() + c.h / 360.0 * r.height();
    let m = egui::Rect::from_center_size(pos2(r.center().x, y), vec2(r.width() + 4.0, 5.0));
    ui.painter().rect_stroke(m, 2.0, EStroke::new(2.0, Color32::WHITE), egui::StrokeKind::Middle);
}
