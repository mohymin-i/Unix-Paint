//! Hidden test driver: `unix-paint --script FILE` replays pointer/keyboard input and commands
//! through eframe's raw-input hook, so the real UI can be exercised end-to-end and screenshotted.
//!
//! One command per line (`#` comments). Coordinates are document pixels.
//!   tool pencil | brush:<Kind> | shape:<Kind> | select | freeselect | fill | text | eraser | picker | magnifier
//!   color1 #RRGGBB / color2 #RRGGBB / size N / outline <Media> / fill <Media>
//!   down X Y [right] / move X Y / up X Y / drag X0 Y0 X1 Y1 [steps] [right] / click X Y [right]
//!   key Ctrl+Shift+Z / type some text / cmd <name> / wait N / shot PATH / save PATH / open PATH / quit

use crate::app::{PaintApp, Tool};
use crate::brush::{BrushKind, Media};
use crate::color::Rgba;
use crate::shapes::ShapeKind;
use egui::{Event, Key, Modifiers, PointerButton, RawInput};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Script {
    lines: VecDeque<String>,
    wait: u32,
    shot_pending: Option<Arc<AtomicBool>>,
    mods_set: bool,
    pub finished: bool,
}

impl Script {
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut lines = VecDeque::new();
        for l in text.lines() {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            // Expand drags into individual frames.
            let parts: Vec<&str> = l.split_whitespace().collect();
            if parts[0] == "drag" && parts.len() >= 5 {
                let n: Vec<f32> = parts[1..5].iter().map(|s| s.parse().unwrap_or(0.0)).collect();
                let steps: usize = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(12);
                let right = parts.contains(&"right");
                let r = if right { " right" } else { "" };
                lines.push_back(format!("down {} {}{r}", n[0], n[1]));
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    lines.push_back(format!("move {} {}", n[0] + (n[2] - n[0]) * t, n[1] + (n[3] - n[1]) * t));
                }
                lines.push_back(format!("up {} {}{r}", n[2], n[3]));
            } else if parts[0] == "click" && parts.len() >= 3 {
                let r = if parts.contains(&"right") { " right" } else { "" };
                lines.push_back(format!("down {} {}{r}", parts[1], parts[2]));
                lines.push_back(format!("up {} {}{r}", parts[1], parts[2]));
            } else {
                lines.push_back(l.to_string());
            }
        }
        // Let the window settle first.
        Ok(Script { lines, wait: 8, shot_pending: None, mods_set: false, finished: false })
    }
}

fn parse_key(s: &str) -> Option<(Modifiers, Key)> {
    let mut m = Modifiers::NONE;
    let mut key = None;
    for part in s.split('+') {
        match part.to_lowercase().as_str() {
            "ctrl" | "cmd" => m = m | Modifiers::COMMAND,
            "shift" => m = m | Modifiers::SHIFT,
            "alt" => m = m | Modifiers::ALT,
            other => key = Key::from_name(other).or_else(|| Key::from_name(&other.to_uppercase())).or_else(|| Key::from_name(part)),
        }
    }
    key.map(|k| (m, k))
}

impl PaintApp {
    pub fn drive_script(&mut self, ctx: &egui::Context, raw: &mut RawInput) {
        let Some(mut sc) = self.script.take() else { return };
        ctx.request_repaint();
        if sc.mods_set {
            raw.events.push(Event::ModifiersChanged(Modifiers::NONE));
            sc.mods_set = false;
        }
        if let Some(flag) = &sc.shot_pending {
            if flag.load(Ordering::SeqCst) {
                sc.shot_pending = None;
            } else {
                self.script = Some(sc);
                return;
            }
        }
        if sc.wait > 0 {
            sc.wait -= 1;
            self.script = Some(sc);
            return;
        }
        let Some(line) = sc.lines.pop_front() else {
            sc.finished = true;
            self.script = Some(sc);
            return;
        };
        if std::env::var_os("UNIX_PAINT_TRACE").is_some() {
            eprintln!("script: {line}");
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| parts.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        let button = if parts.contains(&"right") { PointerButton::Secondary } else { PointerButton::Primary };
        let pos = || self.to_screen(num(1), num(2));
        match parts[0] {
            "tool" => {
                let arg = parts.get(1).copied().unwrap_or("");
                let (kind, sub) = arg.split_once(':').unwrap_or((arg, ""));
                match kind {
                    "pencil" => self.set_tool(Tool::Pencil),
                    "fill" => self.set_tool(Tool::Fill),
                    "text" => self.set_tool(Tool::Text),
                    "eraser" => self.set_tool(Tool::Eraser),
                    "picker" => self.set_tool(Tool::Picker),
                    "magnifier" => self.set_tool(Tool::Magnifier),
                    "select" => {
                        self.settings.free_form_select = false;
                        self.set_tool(Tool::Select)
                    }
                    "freeselect" => {
                        self.settings.free_form_select = true;
                        self.set_tool(Tool::Select)
                    }
                    "brush" => {
                        if let Some(k) = BrushKind::BRUSHES.iter().find(|k| format!("{k:?}").eq_ignore_ascii_case(sub)) {
                            self.settings.brush = *k;
                        }
                        self.set_tool(Tool::Brush)
                    }
                    "shape" => {
                        if let Some(k) = ShapeKind::ALL.iter().find(|k| format!("{k:?}").eq_ignore_ascii_case(sub)) {
                            self.settings.shape = *k;
                        }
                        self.set_tool(Tool::Shape)
                    }
                    _ => eprintln!("script: unknown tool {arg}"),
                }
            }
            "color1" | "color2" => {
                if let Some(c) = parts.get(1).and_then(|s| Rgba::parse_hex(s)) {
                    if parts[0] == "color1" { self.settings.primary = c } else { self.settings.secondary = c }
                }
            }
            "size" => {
                let v = num(1);
                match self.tool.size_kind(&self.settings) {
                    Some(k) => self.settings.set_size(k, v),
                    None => self.settings.shape_size = v,
                }
            }
            "opacity" => {
                if let Some(k) = self.tool.size_kind(&self.settings) {
                    self.settings.set_opacity(k, num(1) / 100.0);
                }
            }
            "outline" | "fill" => {
                let arg = parts.get(1).copied().unwrap_or("");
                if let Some(m) = Media::ALL.iter().find(|m| format!("{m:?}").eq_ignore_ascii_case(arg)) {
                    if parts[0] == "outline" { self.settings.outline = *m } else { self.settings.fill = *m }
                }
            }
            "move" => raw.events.push(Event::PointerMoved(pos())),
            // Screen-space (logical points) press+release, for clicking UI chrome.
            "sclick" => {
                let p = egui::pos2(num(1), num(2));
                raw.events.push(Event::PointerMoved(p));
                raw.events.push(Event::PointerButton { pos: p, button, pressed: true, modifiers: Modifiers::NONE });
                sc.lines.push_front(format!("sup {} {}", num(1), num(2)));
            }
            "sup" => {
                let p = egui::pos2(num(1), num(2));
                raw.events.push(Event::PointerButton { pos: p, button, pressed: false, modifiers: Modifiers::NONE });
            }
            "smove" => raw.events.push(Event::PointerMoved(egui::pos2(num(1), num(2)))),
            "down" | "up" => {
                let p = pos();
                raw.events.push(Event::PointerMoved(p));
                raw.events.push(Event::PointerButton { pos: p, button, pressed: parts[0] == "down", modifiers: Modifiers::NONE });
            }
            "key" => {
                if let Some((m, k)) = parts.get(1).and_then(|s| parse_key(s)) {
                    raw.events.push(Event::ModifiersChanged(m));
                    sc.mods_set = true;
                    raw.events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
                    raw.events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
                }
            }
            "type" => {
                let text = line.splitn(2, ' ').nth(1).unwrap_or("").replace("\\n", "\n");
                raw.events.push(Event::Text(text));
            }
            "cmd" => self.script_command(parts.get(1).copied().unwrap_or(""), &parts[2..]),
            "wait" => sc.wait = num(1) as u32,
            "shot" => {
                let path = std::path::PathBuf::from(parts.get(1).copied().unwrap_or("shot.png"));
                let flag = Arc::new(AtomicBool::new(false));
                let f2 = flag.clone();
                let cb = egui::ScreenshotCallback::new(move |img| {
                    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                    if let Some(buf) = image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba) {
                        let _ = buf.save(&path);
                    }
                    f2.store(true, Ordering::SeqCst);
                });
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
                sc.shot_pending = Some(flag);
                sc.wait = 2;
            }
            "save" => {
                let p = std::path::PathBuf::from(parts.get(1).copied().unwrap_or("out.png"));
                self.commit_pending();
                let f = crate::io::Format::from_path(&p).unwrap_or(crate::io::Format::Png);
                if !self.save_to(&p, f) {
                    eprintln!("script: save failed");
                }
            }
            "open" => {
                if let Some(p) = parts.get(1) {
                    self.open_path(std::path::Path::new(p));
                }
            }
            "quit" => sc.lines.clear(),
            other => eprintln!("script: unknown command {other}"),
        }
        self.script = Some(sc);
    }

    fn script_command(&mut self, name: &str, args: &[&str]) {
        let n = |i: usize| args.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        match name {
            "rotate" => self.rotate(n(0)),
            "fliph" => self.flip(true),
            "flipv" => self.flip(false),
            "invert" => self.invert_colors(),
            "crop" => self.crop(),
            "selectall" => self.select_all(),
            "invertselection" => self.invert_selection(),
            "delete" => self.delete_selection(),
            "undo" => self.undo(),
            "copy" => self.copy(),
            "cut" => self.cut(),
            "paste" => self.paste(),
            "redo" => self.redo(),
            "resize" => self.resize_image(n(0) as u32, n(1) as u32, n(2), n(3)),
            "addlayer" => self.add_layer(),
            "duplicatelayer" => self.duplicate_layer(),
            "mergedown" => self.merge_down(),
            "layers" => self.settings.show_layers = !self.settings.show_layers,
            "rulers" => self.settings.show_rulers = !self.settings.show_rulers,
            "grid" => self.settings.show_grid = !self.settings.show_grid,
            "thumbnail" => self.show_thumbnail = !self.show_thumbnail,
            "zoom" => self.zoom_request = Some(crate::app::ZoomRequest::To(n(0) / 100.0)),
            "fit" => self.zoom_request = Some(crate::app::ZoomRequest::Fit),
            "transparent" => self.settings.transparent_selection = !self.settings.transparent_selection,
            "background" => {
                let v = !self.doc.background_visible;
                self.set_background_visible(v)
            }
            "dark" => self.pending_theme = Some(crate::settings::ThemePref::Dark),
            "light" => self.pending_theme = Some(crate::settings::ThemePref::Light),
            "commit" => self.commit_pending(),
            "new" => self.new_document(),
            "dialog" => {
                use crate::ui::dialogs::Dialog;
                self.dialog = match args.first().copied().unwrap_or("") {
                    "properties" => Some(Dialog::properties(self)),
                    "resize" => Some(Dialog::resize(self)),
                    "colors" => Some(Dialog::edit_colors(self, false)),
                    "print" => Some(Dialog::print(self)),
                    "about" => Some(Dialog::About),
                    "shortcuts" => Some(Dialog::Shortcuts),
                    "unsaved" => Some(Dialog::Unsaved { after: crate::app::After::New }),
                    _ => None,
                }
            }
            "textstyle" => {
                for a in args {
                    match *a {
                        "bold" => self.settings.text.bold = true,
                        "italic" => self.settings.text.italic = true,
                        "underline" => self.settings.text.underline = true,
                        "center" => self.settings.text.align = 1,
                        s => {
                            if let Ok(v) = s.parse::<f32>() {
                                self.settings.text.size_pt = v;
                            }
                        }
                    }
                }
            }
            other => eprintln!("script: unknown cmd {other}"),
        }
    }
}
