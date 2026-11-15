//! Keyboard handling: Paint's shortcuts plus text-box editing.

use crate::app::{After, PaintApp, Tool, ZoomRequest};
use crate::ui::dialogs::Dialog;
use egui::{Key, KeyboardShortcut, Modifiers};

pub const LIST: &[(&str, &str)] = &[
    ("Ctrl+N", "New"),
    ("Ctrl+O", "Open"),
    ("Ctrl+S", "Save"),
    ("Ctrl+Shift+S / F12", "Save as"),
    ("Ctrl+P", "Print"),
    ("Ctrl+E", "Image properties"),
    ("Ctrl+Z", "Undo"),
    ("Ctrl+Y / Ctrl+Shift+Z", "Redo"),
    ("Ctrl+X / C / V", "Cut / Copy / Paste"),
    ("Ctrl+A", "Select all"),
    ("Delete", "Delete selection"),
    ("Esc", "Finish / cancel the current object"),
    ("Arrow keys", "Nudge selection (Shift: 10 px)"),
    ("Ctrl+Shift+X", "Crop to selection"),
    ("Ctrl+W", "Resize and skew"),
    ("Ctrl+Shift+I", "Invert colors"),
    ("Ctrl+Shift+N", "Clear image"),
    ("Ctrl+R", "Rulers"),
    ("Ctrl+G", "Gridlines"),
    ("Ctrl++ / Ctrl+PgUp", "Zoom in"),
    ("Ctrl+- / Ctrl+PgDn", "Zoom out"),
    ("Ctrl+0", "Fit to window"),
    ("Ctrl+1", "Actual size"),
    ("Ctrl+wheel", "Zoom at pointer"),
    ("Space+drag / middle drag", "Pan"),
    ("[ / ]", "Decrease / increase size"),
    ("F11", "Full screen"),
    ("Ctrl+B / I / U", "Bold / italic / underline (text)"),
    ("Shift while drawing", "Constrain lines to 45°, shapes to squares"),
    ("Right mouse button", "Draw with color 2"),
    ("Ctrl+drag selection", "Stamp a copy"),
    ("P B T E I M S", "Pencil, Brush, Text, Eraser, Picker, Magnifier, Select"),
    ("F1", "This list"),
];

fn sc(ctx: &egui::Context, m: Modifiers, k: Key) -> bool {
    ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)))
}

pub fn handle(app: &mut PaintApp, ctx: &egui::Context) {
    if app.dialog.is_some() {
        return;
    }
    // Leave keys alone while an egui text field (font size box, etc.) has focus.
    if ctx.memory(|m| m.focused().is_some()) {
        return;
    }
    let cmd = Modifiers::COMMAND;
    let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

    if app.fullscreen && (sc(ctx, Modifiers::NONE, Key::Escape) || sc(ctx, Modifiers::NONE, Key::F11)) {
        app.fullscreen = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        return;
    }

    if app.text.is_some() {
        text_keys(app, ctx);
    }

    // `consume_shortcut` ignores extra Shift/Alt, so every Ctrl+Shift chord has to be tested
    // before the plain Ctrl chord on the same key (Ctrl+Shift+N must not run "New").
    if sc(ctx, cmd_shift, Key::S) || sc(ctx, Modifiers::NONE, Key::F12) {
        app.save_as(None);
    } else if sc(ctx, cmd_shift, Key::Z) {
        app.redo();
    } else if sc(ctx, cmd_shift, Key::X) {
        app.crop();
    } else if sc(ctx, cmd_shift, Key::I) {
        app.invert_colors();
    } else if sc(ctx, cmd_shift, Key::N) {
        app.clear_image();
    } else if sc(ctx, cmd, Key::N) {
        app.request(After::New);
    } else if sc(ctx, cmd, Key::O) {
        app.request(After::Open(None));
    } else if sc(ctx, cmd, Key::S) {
        app.save();
    } else if sc(ctx, cmd, Key::P) {
        app.dialog = Some(Dialog::print(app));
    } else if sc(ctx, cmd, Key::E) {
        app.dialog = Some(Dialog::properties(app));
    } else if sc(ctx, cmd, Key::Y) {
        app.redo();
    } else if sc(ctx, cmd, Key::Z) {
        app.undo();
    } else if sc(ctx, cmd, Key::A) {
        app.select_all();
    } else if sc(ctx, cmd, Key::W) {
        app.dialog = Some(Dialog::resize(app));
    } else if sc(ctx, cmd, Key::R) {
        app.settings.show_rulers = !app.settings.show_rulers;
    } else if sc(ctx, cmd, Key::G) {
        app.settings.show_grid = !app.settings.show_grid;
    } else if sc(ctx, cmd, Key::PageUp) || sc(ctx, cmd, Key::Plus) || sc(ctx, cmd, Key::Equals) {
        app.zoom_in(None);
    } else if sc(ctx, cmd, Key::PageDown) || sc(ctx, cmd, Key::Minus) {
        app.zoom_out(None);
    } else if sc(ctx, cmd, Key::Num0) {
        app.zoom_request = Some(ZoomRequest::Fit);
    } else if sc(ctx, cmd, Key::Num1) {
        app.zoom_request = Some(ZoomRequest::To(1.0));
    } else if sc(ctx, Modifiers::NONE, Key::F11) {
        app.fullscreen = !app.fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(app.fullscreen));
    } else if sc(ctx, Modifiers::NONE, Key::F1) {
        app.dialog = Some(Dialog::Shortcuts);
    } else if sc(ctx, Modifiers::NONE, Key::Delete) {
        app.delete_selection();
    } else if sc(ctx, Modifiers::NONE, Key::Escape) {
        if app.stroke.is_some() {
            app.stroke = None;
            app.history.cancel_patch();
            app.invalidate_all();
        } else if let Some(s) = app.shape.as_mut() {
            if s.kind == crate::shapes::ShapeKind::Polygon && !s.closed {
                s.pts.pop();
            }
            app.commit_shape();
        } else {
            app.deselect();
        }
    } else if sc(ctx, Modifiers::NONE, Key::OpenBracket) {
        app.adjust_size(-1.0);
    } else if sc(ctx, Modifiers::NONE, Key::CloseBracket) {
        app.adjust_size(1.0);
    }

    // Clipboard arrives as events (Copy/Cut/Paste) on most platforms.
    let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
    for e in events {
        match e {
            egui::Event::Copy => app.copy(),
            egui::Event::Cut => app.cut(),
            egui::Event::Paste(_) => app.paste(),
            // Sent instead of `Paste` when the clipboard holds a picture but no text.
            egui::Event::PasteImage(img) => app.paste_color_image(&img),
            _ => {}
        }
    }

    // Nudge a selection with the arrow keys.
    if app.selection.is_some() {
        let step = if ctx.input(|i| i.modifiers.shift) { 10 } else { 1 };
        let mut d = (0, 0);
        for (k, v) in [(Key::ArrowLeft, (-step, 0)), (Key::ArrowRight, (step, 0)), (Key::ArrowUp, (0, -step)), (Key::ArrowDown, (0, step))] {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k) || i.consume_key(Modifiers::SHIFT, k)) {
                d = v;
            }
        }
        if d != (0, 0) {
            app.lift_selection();
            if let Some(s) = app.selection.as_mut() {
                let before = s.dest;
                s.dest.x += d.0;
                s.dest.y += d.1;
                let after = s.dest;
                app.invalidate(before.union(&after).inflate(2));
            }
        }
    }

    // Single-letter tool keys (only without modifiers).
    let letters = [
        (Key::P, Tool::Pencil),
        (Key::B, Tool::Brush),
        (Key::T, Tool::Text),
        (Key::E, Tool::Eraser),
        (Key::I, Tool::Picker),
        (Key::M, Tool::Magnifier),
        (Key::S, Tool::Select),
        (Key::F, Tool::Fill),
    ];
    for (k, t) in letters {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k)) {
            app.set_tool(t);
        }
    }
}

/// Feed typing to the text box. Plain keys and text are consumed; Ctrl/Cmd chords other than
/// the text-formatting ones are left for the global shortcuts (save, new, ...).
fn text_keys(app: &mut PaintApp, ctx: &egui::Context) {
    let cmd = Modifiers::COMMAND;
    // Ctrl+Shift chords (redo, invert colours, ...) belong to the global handler.
    if ctx.input(|i| i.modifiers.command && i.modifiers.shift) {
        return;
    }
    if sc(ctx, cmd, Key::B) {
        app.settings.text.bold = !app.settings.text.bold;
    }
    if sc(ctx, cmd, Key::I) {
        app.settings.text.italic = !app.settings.text.italic;
    }
    if sc(ctx, cmd, Key::U) {
        app.settings.text.underline = !app.settings.text.underline;
    }
    if sc(ctx, cmd, Key::A) {
        if let Some(t) = app.text.as_mut() {
            t.select_all();
        }
    }
    if sc(ctx, cmd, Key::Z) {
        app.undo();
        return;
    }
    let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
    let mut consumed = false;
    for e in events {
        let Some(t) = app.text.as_mut() else { break };
        match e {
            // Composed input (dead keys, CJK input methods) arrives as an IME commit.
            egui::Event::Text(s) | egui::Event::Ime(egui::ImeEvent::Commit(s)) => {
                t.insert(&s);
                consumed = true;
            }
            egui::Event::Copy => {
                app.copy();
                consumed = true;
            }
            egui::Event::Cut => {
                if let Some(s) = t.selected_text() {
                    let _ = crate::io::clipboard_set_text(s);
                    t.backspace();
                }
                consumed = true;
            }
            egui::Event::Paste(s) => {
                t.insert(&s);
                consumed = true;
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                let ext = modifiers.shift;
                consumed = true;
                match key {
                    Key::Backspace => t.backspace(),
                    Key::Delete => t.delete(),
                    Key::Enter => t.insert("\n"),
                    Key::ArrowLeft => t.left(ext),
                    Key::ArrowRight => t.right(ext),
                    Key::ArrowUp => t.vertical(-1, ext),
                    Key::ArrowDown => t.vertical(1, ext),
                    Key::Home => t.home(ext),
                    Key::End => t.end(ext),
                    Key::Escape => {
                        app.commit_text();
                    }
                    _ => consumed = false,
                }
            }
            _ => {}
        }
    }
    if consumed {
        app.invalidate_all();
    }
    // While typing, plain keys (tool letters, Delete, arrows, Esc) must not reach the global
    // handler; clipboard events were handled above.
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::Text(_) | egui::Event::Ime(_) | egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) | egui::Event::PasteImage(_) => false,
            egui::Event::Key { modifiers, .. } => modifiers.command,
            _ => true,
        });
    });
}
