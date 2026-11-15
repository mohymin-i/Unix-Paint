//! Window chrome: menu bar (File / Edit / View), status bar, layers panel, toasts, thumbnail.

use crate::app::{After, PaintApp, ZOOM_LEVELS, ZoomRequest};
use crate::icons as ic;
use crate::io::Format;
use crate::settings::ThemePref;
use crate::ui::dialogs::Dialog;
use crate::ui::toolbar::icon_button;
use egui::{Color32, FontId, Sense, Stroke as EStroke, Ui, pos2, vec2};

fn item(ui: &mut Ui, icon: &str, label: &str, shortcut: &str) -> bool {
    let b = egui::Button::new(format!("{icon}  {label}")).shortcut_text(crate::ui::shortcut_label(shortcut)).min_size(vec2(230.0, 26.0));
    ui.add(b).clicked()
}

fn check_item(ui: &mut Ui, on: bool, label: &str, shortcut: &str) -> bool {
    let icon = if on { ic::CHECK } else { " " };
    item(ui, icon, label, shortcut)
}

pub fn menu_bar(app: &mut PaintApp, ui: &mut Ui) {
    let pal = app.palette;
    egui::Panel::top("menubar")
        .exact_size(38.0)
        .frame(egui::Frame::new().fill(pal.window).inner_margin(egui::Margin::symmetric(8, 4)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                // App mark
                let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                for (i, c) in [Color32::from_rgb(237, 28, 36), Color32::from_rgb(255, 201, 14), Color32::from_rgb(34, 177, 76), Color32::from_rgb(0, 162, 232)]
                    .iter()
                    .enumerate()
                {
                    let (dx, dy) = ((i % 2) as f32 * 9.0, (i / 2) as f32 * 9.0);
                    ui.painter().circle_filled(r.min + vec2(6.5 + dx, 6.5 + dy), 4.5, *c);
                }
                ui.add_space(4.0);
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| file_menu(app, ui));
                    ui.menu_button("Edit", |ui| edit_menu(app, ui));
                    ui.menu_button("View", |ui| view_menu(app, ui));
                    ui.add_space(8.0);
                    ui.separator();
                    quick_buttons(app, ui);
                });
            });
        });
}

fn quick_buttons(app: &mut PaintApp, ui: &mut Ui) {
    let pal = app.palette;
    {
        {
                if icon_button(ui, ic::FLOPPY_DISK, false, 28.0, "Save (Ctrl+S)").clicked() {
                    app.save();
                }
                let can_undo = app.history.can_undo() || app.shape.is_some() || app.text.is_some();
                ui.add_enabled_ui(can_undo, |ui| {
                    let tip = app.history.undo_label().map(|l| format!("Undo {l} (Ctrl+Z)")).unwrap_or_else(|| "Undo (Ctrl+Z)".into());
                    if icon_button(ui, ic::ARROW_U_UP_LEFT, false, 28.0, &tip).clicked() {
                        app.undo();
                    }
                });
                ui.add_enabled_ui(app.history.can_redo(), |ui| {
                    let tip = app.history.redo_label().map(|l| format!("Redo {l} (Ctrl+Y)")).unwrap_or_else(|| "Redo (Ctrl+Y)".into());
                    if icon_button(ui, ic::ARROW_U_UP_RIGHT, false, 28.0, &tip).clicked() {
                        app.redo();
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let theme_icon = if pal.dark { ic::SUN } else { ic::MOON };
                    if icon_button(ui, theme_icon, false, 28.0, "Switch light / dark theme").clicked() {
                        let ctx = ui.ctx().clone();
                        app.set_theme(&ctx, if pal.dark { ThemePref::Light } else { ThemePref::Dark });
                    }
                });
        }
    }
}

fn file_menu(app: &mut PaintApp, ui: &mut Ui) {
    if item(ui, ic::FILE_PLUS, "New", "Ctrl+N") {
        app.request(After::New);
    }
    if item(ui, ic::FOLDER_OPEN, "Open", "Ctrl+O") {
        app.request(After::Open(None));
    }
    ui.menu_button(format!("{}  Import to canvas", ic::IMAGE_SQUARE), |ui| {
        if item(ui, ic::FILE_IMAGE, "From a file", "") {
            app.import_file();
        }
    });
    ui.menu_button(format!("{}  Recent", ic::CLOCK_COUNTER_CLOCKWISE), |ui| {
        if app.settings.recent.is_empty() {
            ui.label("No recent files");
        }
        for p in app.settings.recent.clone() {
            let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if ui.button(&name).on_hover_text(p.display().to_string()).clicked() {
                app.request(After::Open(Some(p)));
            }
        }
    });
    ui.separator();
    if item(ui, ic::FLOPPY_DISK, "Save", "Ctrl+S") {
        app.save();
    }
    ui.menu_button(format!("{}  Save as", ic::FLOPPY_DISK_BACK), |ui| {
        for f in Format::SAVE_CHOICES {
            if item(ui, ic::FILE_IMAGE, f.label(), if f == Format::Png { "Ctrl+Shift+S" } else { "" }) {
                app.save_as(Some(f));
            }
        }
    });
    ui.separator();
    ui.menu_button(format!("{}  Print", ic::PRINTER), |ui| {
        if item(ui, ic::PRINTER, "Print", "Ctrl+P") {
            app.dialog = Some(Dialog::print(app));
        }
        if item(ui, ic::FILE_TEXT, "Page setup", "") {
            app.dialog = Some(Dialog::print(app));
        }
    });
    ui.menu_button(format!("{}  Set as desktop background", ic::DESKTOP), |ui| {
        for (label, mode) in [("Fill", crate::platform::WallpaperMode::Fill), ("Tile", crate::platform::WallpaperMode::Tile), ("Center", crate::platform::WallpaperMode::Center)] {
            if item(ui, ic::MONITOR, label, "") {
                app.commit_pending();
                match crate::platform::set_wallpaper(&app.doc.flatten(), mode) {
                    Ok(()) => app.notify("Desktop background updated."),
                    Err(e) => app.error(format!("Couldn't set the desktop background.\n{e:#}")),
                }
            }
        }
    });
    ui.separator();
    if item(ui, ic::SLIDERS_HORIZONTAL, "Image properties", "Ctrl+E") {
        app.dialog = Some(Dialog::properties(app));
    }
    if item(ui, ic::INFO, "About Paint", "") {
        app.dialog = Some(Dialog::About);
    }
    ui.separator();
    if item(ui, ic::SIGN_OUT, "Exit", "Alt+F4") {
        app.request(After::Exit);
    }
}

fn edit_menu(app: &mut PaintApp, ui: &mut Ui) {
    let undo = app.history.undo_label().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into());
    let redo = app.history.redo_label().map(|l| format!("Redo {l}")).unwrap_or_else(|| "Redo".into());
    if item(ui, ic::ARROW_U_UP_LEFT, &undo, "Ctrl+Z") {
        app.undo();
    }
    if item(ui, ic::ARROW_U_UP_RIGHT, &redo, "Ctrl+Y") {
        app.redo();
    }
    ui.separator();
    let has_sel = app.selection.is_some();
    ui.add_enabled_ui(has_sel, |ui| {
        if item(ui, ic::SCISSORS, "Cut", "Ctrl+X") {
            app.cut();
        }
        if item(ui, ic::COPY, "Copy", "Ctrl+C") {
            app.copy();
        }
    });
    if item(ui, ic::CLIPBOARD_TEXT, "Paste", "Ctrl+V") {
        app.paste();
    }
    if item(ui, ic::FILE_IMAGE, "Paste from file…", "") {
        app.import_file();
    }
    ui.separator();
    if item(ui, ic::SELECTION_ALL, "Select all", "Ctrl+A") {
        app.select_all();
    }
    if item(ui, ic::SELECTION_INVERSE, "Invert selection", "") {
        app.invert_selection();
    }
    ui.add_enabled_ui(has_sel, |ui| {
        if item(ui, ic::TRASH, "Delete selection", "Del") {
            app.delete_selection();
        }
        if item(ui, ic::CROP, "Crop to selection", "Ctrl+Shift+X") {
            app.crop();
        }
    });
    ui.separator();
    if item(ui, ic::ARROWS_OUT_SIMPLE, "Resize and skew…", "Ctrl+W") {
        app.dialog = Some(Dialog::resize(app));
    }
    if item(ui, ic::CIRCLE_HALF, "Invert colors", "Ctrl+Shift+I") {
        app.invert_colors();
    }
    if item(ui, ic::BROOM, "Clear image", "Ctrl+Shift+N") {
        app.clear_image();
    }
}

fn view_menu(app: &mut PaintApp, ui: &mut Ui) {
    if item(ui, ic::MAGNIFYING_GLASS_PLUS, "Zoom in", "Ctrl++") {
        app.zoom_in(None);
    }
    if item(ui, ic::MAGNIFYING_GLASS_MINUS, "Zoom out", "Ctrl+-") {
        app.zoom_out(None);
    }
    if item(ui, ic::FRAME_CORNERS, "Fit to window", "Ctrl+0") {
        app.zoom_request = Some(ZoomRequest::Fit);
    }
    if item(ui, ic::SQUARE, "Actual size (100%)", "Ctrl+1") {
        app.zoom_request = Some(ZoomRequest::To(1.0));
    }
    ui.separator();
    if check_item(ui, app.settings.show_rulers, "Rulers", "Ctrl+R") {
        app.settings.show_rulers = !app.settings.show_rulers;
    }
    if check_item(ui, app.settings.show_grid, "Gridlines", "Ctrl+G") {
        app.settings.show_grid = !app.settings.show_grid;
    }
    if check_item(ui, app.settings.show_status_bar, "Status bar", "") {
        app.settings.show_status_bar = !app.settings.show_status_bar;
    }
    if check_item(ui, app.show_thumbnail, "Thumbnail", "") {
        app.show_thumbnail = !app.show_thumbnail;
    }
    if check_item(ui, app.settings.show_layers, "Layers panel", "") {
        app.settings.show_layers = !app.settings.show_layers;
    }
    if item(ui, ic::CORNERS_OUT, "Full screen", "F11") {
        app.fullscreen = true;
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
    }
    ui.separator();
    ui.menu_button(format!("{}  App theme", ic::CIRCLE_HALF), |ui| {
        for (label, pref) in [("Light", ThemePref::Light), ("Dark", ThemePref::Dark), ("Use system setting", ThemePref::System)] {
            if check_item(ui, app.settings.theme == pref, label, "") {
                let ctx = ui.ctx().clone();
                app.set_theme(&ctx, pref);
            }
        }
    });
    if item(ui, ic::KEYBOARD, "Keyboard shortcuts", "F1") {
        app.dialog = Some(Dialog::Shortcuts);
    }
}

fn fmt_size(b: u64) -> String {
    if b >= 1024 * 1024 {
        format!("{:.1}MB", b as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}KB", b as f64 / 1024.0)
    }
}

pub fn status_bar(app: &mut PaintApp, ui: &mut Ui) {
    let pal = app.palette;
    egui::Panel::bottom("statusbar")
        .exact_size(32.0)
        .frame(egui::Frame::new().fill(pal.window).inner_margin(egui::Margin::symmetric(12, 4)).stroke(EStroke::new(1.0, pal.border)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let dim = |s: String| egui::RichText::new(s).color(pal.text_dim).size(12.5);
                let cell = |ui: &mut Ui, icon: &str, text: String, w: f32| {
                    ui.allocate_ui(vec2(w, 22.0), |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.label(egui::RichText::new(icon).color(pal.text_dim).size(14.0));
                            ui.label(dim(text));
                        });
                    });
                };
                cell(ui, ic::CURSOR, app.cursor_doc.map(|(x, y)| format!("{x}, {y}px")).unwrap_or_default(), 110.0);
                let sel = app.selection.as_ref().map(|s| format!("{} × {}px", s.dest.w, s.dest.h)).unwrap_or_default();
                cell(ui, ic::SELECTION, sel, 110.0);
                cell(ui, ic::FRAME_CORNERS, format!("{} × {}px", app.doc.width, app.doc.height), 130.0);
                cell(ui, ic::FLOPPY_DISK, app.doc.file_size.map(fmt_size).map(|s| format!("Size: {s}")).unwrap_or_default(), 120.0);
                if let Some(l) = app.doc.layers.get(app.doc.active) {
                    if app.doc.layers.len() > 1 {
                        cell(ui, ic::STACK, l.name.clone(), 120.0);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if icon_button(ui, ic::FRAME_CORNERS, false, 24.0, "Fit to window (Ctrl+0)").clicked() {
                        app.zoom_request = Some(ZoomRequest::Fit);
                    }
                    if icon_button(ui, ic::MAGNIFYING_GLASS_PLUS, false, 24.0, "Zoom in").clicked() {
                        app.zoom_in(None);
                    }
                    let mut lz = app.zoom.log2();
                    let r = ui.add_sized([140.0, 20.0], egui::Slider::new(&mut lz, (0.125f32).log2()..=(8.0f32).log2()).show_value(false));
                    if r.changed() {
                        app.zoom_request = Some(ZoomRequest::To(2f32.powf(lz)));
                    }
                    if icon_button(ui, ic::MAGNIFYING_GLASS_MINUS, false, 24.0, "Zoom out").clicked() {
                        app.zoom_out(None);
                    }
                    egui::ComboBox::from_id_salt("zoom-combo")
                        .selected_text(format!("{:.0}%", app.zoom * 100.0))
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            for z in ZOOM_LEVELS.iter().rev() {
                                if ui.selectable_label((app.zoom - z).abs() < 1e-3, format!("{:.0}%", z * 100.0)).clicked() {
                                    app.zoom_request = Some(ZoomRequest::To(*z));
                                }
                            }
                            if ui.selectable_label(false, "Fit to window").clicked() {
                                app.zoom_request = Some(ZoomRequest::Fit);
                            }
                        });
                });
            });
        });
}

pub fn layers_panel(app: &mut PaintApp, ui: &mut Ui) {
    let pal = app.palette;
    let ctx = ui.ctx().clone();
    app.layer_thumbnails(&ctx);
    egui::Panel::right("layers")
        .exact_size(250.0)
        .resizable(false)
        .frame(egui::Frame::new().fill(pal.panel).inner_margin(egui::Margin::same(10)).stroke(EStroke::new(1.0, pal.border)))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Layers").strong().size(15.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, ic::X, false, 24.0, "Close").clicked() {
                        app.settings.show_layers = false;
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if icon_button(ui, ic::PLUS, false, 28.0, "Add layer").clicked() {
                    app.add_layer();
                }
                if icon_button(ui, ic::COPY, false, 28.0, "Duplicate layer").clicked() {
                    app.duplicate_layer();
                }
                if icon_button(ui, ic::ARROWS_MERGE, false, 28.0, "Merge down").clicked() {
                    app.merge_down();
                }
                if icon_button(ui, ic::ARROW_UP, false, 28.0, "Move up").clicked() {
                    app.move_layer(true);
                }
                if icon_button(ui, ic::ARROW_DOWN, false, 28.0, "Move down").clicked() {
                    app.move_layer(false);
                }
                if icon_button(ui, ic::TRASH, false, 28.0, "Delete layer").clicked() {
                    app.delete_layer();
                }
            });
            ui.separator();
            let checker = app.checker_texture(&ctx);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let n = app.doc.layers.len();
                for i in (0..n).rev() {
                    let active = i == app.doc.active;
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
                    let bg = if active { pal.accent_soft } else if resp.hovered() { pal.hover } else { Color32::TRANSPARENT };
                    ui.painter().rect_filled(r, 6.0, bg);
                    if active {
                        ui.painter().rect_stroke(r, 6.0, EStroke::new(1.0, pal.accent), egui::StrokeKind::Inside);
                    }
                    let thumb_r = egui::Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(60.0, 44.0));
                    ui.painter().image(checker.id(), thumb_r, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(4.0, 3.0)), Color32::WHITE);
                    if let Some(Some(t)) = app.layer_thumbs.get(i) {
                        let s = t.size_vec2();
                        let k = (60.0 / s.x).min(44.0 / s.y);
                        let ir = egui::Rect::from_center_size(thumb_r.center(), s * k);
                        ui.painter().image(t.id(), ir, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    ui.painter().rect_stroke(thumb_r, 2.0, EStroke::new(1.0, pal.border), egui::StrokeKind::Outside);
                    let name = app.doc.layers[i].name.clone();
                    ui.painter().text(pos2(thumb_r.right() + 10.0, r.center().y), egui::Align2::LEFT_CENTER, name, FontId::proportional(13.5), pal.text);
                    let eye_r = egui::Rect::from_center_size(pos2(r.right() - 20.0, r.center().y), vec2(28.0, 28.0));
                    let eye = ui.interact(eye_r, ui.scope_id().with(("eye", i)), Sense::click());
                    let vis = app.doc.layers[i].visible;
                    ui.painter().text(eye_r.center(), egui::Align2::CENTER_CENTER, if vis { ic::EYE } else { ic::EYE_SLASH }, FontId::proportional(17.0), if eye.hovered() { pal.accent } else { pal.text_dim });
                    if eye.on_hover_text(if vis { "Hide layer" } else { "Show layer" }).clicked() {
                        app.toggle_layer_visibility(i);
                    } else if resp.clicked() {
                        app.select_layer(i);
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Duplicate").clicked() {
                            app.select_layer(i);
                            app.duplicate_layer();
                        }
                        if ui.button("Merge down").clicked() {
                            app.select_layer(i);
                            app.merge_down();
                        }
                        if ui.button("Rename…").clicked() {
                            app.dialog = Some(Dialog::RenameLayer { index: i, name: app.doc.layers[i].name.clone() });
                        }
                        if ui.button("Delete").clicked() {
                            app.select_layer(i);
                            app.delete_layer();
                        }
                    });
                    ui.add_space(4.0);
                }
                // Background row
                ui.separator();
                ui.horizontal(|ui| {
                    let mut c = app.doc.background.to_egui();
                    let r = ui.color_edit_button_srgba(&mut c);
                    if r.changed() {
                        app.commit_pending();
                        app.doc.background = crate::color::Rgba::from_egui(c);
                        app.invalidate_all();
                        app.history.mark_dirty();
                    }
                    ui.label("Background");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let vis = app.doc.background_visible;
                        if icon_button(ui, if vis { ic::EYE } else { ic::EYE_SLASH }, false, 28.0, "Show / hide background").clicked() {
                            app.set_background_visible(!vis);
                        }
                    });
                });
                if app.doc.layers.len() > 1 && ui.button(format!("{}  Flatten image", ic::STACK_SIMPLE)).clicked() {
                    app.flatten_layers();
                }
            });
        });
}

pub fn toast(app: &mut PaintApp, ctx: &egui::Context) {
    let Some((msg, _)) = app.toast.clone() else { return };
    let pal = app.palette;
    egui::Area::new(egui::Id::unique("toast"))
        .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -48.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(if pal.dark { Color32::from_rgb(60, 60, 60) } else { Color32::from_rgb(40, 40, 40) })
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(msg).color(Color32::WHITE));
                });
        });
}

pub fn thumbnail_window(app: &mut PaintApp, ctx: &egui::Context) {
    let tex = app.thumbnail_texture(ctx);
    let mut open = true;
    egui::Window::new("Thumbnail").open(&mut open).resizable(false).default_pos(pos2(ctx.content_rect().right() - 300.0, 160.0)).show(ctx, |ui| {
        let s = tex.size_vec2();
        let k = (240.0 / s.x).min(180.0 / s.y).min(1.0);
        ui.image((tex.id(), s * k));
    });
    if !open {
        app.show_thumbnail = false;
    }
}
