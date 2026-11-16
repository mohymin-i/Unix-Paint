//! Application state, commands and the per-frame driver.

use crate::brush::{BrushKind, Stroke};
use crate::color::Rgba;
use crate::doc::{Document, Overlay};
use crate::history::History;
use crate::io::{self, Format};
use crate::raster::{self, Rect};
use crate::selection::Selection;
use crate::settings::{Settings, ThemePref};
use crate::shape_edit::{PendingShape, ShapeParams};
use crate::text::{TextBox, TextEngine};
use crate::ui::dialogs::Dialog;
use crate::ui::theme::{self, Palette};
use egui::{ColorImage, TextureHandle, TextureOptions};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pencil,
    Fill,
    Text,
    Eraser,
    Picker,
    Magnifier,
    Brush,
    Shape,
}

impl Tool {
    pub fn name(self, s: &Settings) -> String {
        match self {
            Tool::Select => if s.free_form_select { "Free-form selection" } else { "Rectangular selection" }.into(),
            Tool::Pencil => "Pencil".into(),
            Tool::Fill => "Fill".into(),
            Tool::Text => "Text".into(),
            Tool::Eraser => "Eraser".into(),
            Tool::Picker => "Color picker".into(),
            Tool::Magnifier => "Magnifier".into(),
            Tool::Brush => s.brush.name().into(),
            Tool::Shape => s.shape.name().into(),
        }
    }

    /// The brush kind whose size/opacity the Size control edits for this tool.
    pub fn size_kind(self, s: &Settings) -> Option<BrushKind> {
        match self {
            Tool::Pencil => Some(BrushKind::Pencil),
            Tool::Eraser => Some(BrushKind::Eraser),
            Tool::Brush => Some(s.brush),
            _ => None,
        }
    }
}

/// What to do after the "save changes?" prompt.
#[derive(Clone, Debug)]
pub enum After {
    New,
    Open(Option<PathBuf>),
    Exit,
}

/// One texture of the canvas. A document larger than the GPU's texture limit is split into
/// several; normally there is exactly one.
struct Tile {
    rect: Rect,
    tex: TextureHandle,
}

pub struct ActiveStroke {
    pub stroke: Stroke,
    pub last_tick: Instant,
}

pub struct PaintApp {
    pub doc: Document,
    pub history: History,
    pub settings: Settings,
    pub tool: Tool,
    pub prev_tool: Tool,
    pub zoom: f32,
    pub stroke: Option<ActiveStroke>,
    pub shape: Option<PendingShape>,
    pub selection: Option<Selection>,
    pub text: Option<TextBox>,
    pub text_engine: TextEngine,
    pub dialog: Option<Dialog>,
    pub toast: Option<(String, Instant)>,
    pub cursor_doc: Option<(i32, i32)>,
    pub fullscreen: bool,
    pub show_thumbnail: bool,
    pub palette: Palette,
    pub canvas: crate::ui::canvas::CanvasState,
    // texture management
    tiles: Vec<Tile>,
    tex_size: (u32, u32),
    /// Largest texture side the GPU accepts (updated every frame).
    max_side: u32,
    last_title: String,
    dirty: Rect,
    full_dirty: bool,
    prev_overlay: Rect,
    pub thumb_tex: Option<TextureHandle>,
    thumb_gen: u64,
    pub checker: Option<TextureHandle>,
    sel_tex: Option<(u64, TextureHandle)>,
    /// Request scroll/zoom changes from commands (applied by the canvas view).
    pub zoom_request: Option<ZoomRequest>,
    allow_close: bool,
    screenshot: Option<(PathBuf, u32)>,
    screenshot_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub layer_thumbs: Vec<Option<TextureHandle>>,
    layer_thumbs_gen: u64,
    pub script: Option<crate::script::Script>,
    pub pending_theme: Option<ThemePref>,
    /// Don't persist settings (scripted runs).
    ephemeral: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum ZoomRequest {
    Fit,
    To(f32),
    /// Zoom keeping a screen point fixed.
    At(f32, egui::Pos2),
}

pub const ZOOM_LEVELS: [f32; 15] =
    [0.125, 0.25, 0.333, 0.5, 0.667, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 12.0];
pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 32.0;

impl PaintApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        file: Option<PathBuf>,
        screenshot: Option<(PathBuf, u32)>,
        script: Option<crate::script::Script>,
    ) -> Self {
        let ephemeral = script.is_some() || screenshot.is_some();
        let settings = if ephemeral { Settings::default() } else { Settings::load() };
        let text_engine = TextEngine::new();
        let ui_font = ["Inter", "Segoe UI", "Noto Sans", "Cantarell", "Ubuntu"]
            .iter()
            .find_map(|f| text_engine.family_bytes(f));
        theme::install_fonts(&cc.egui_ctx, ui_font);
        let dark = match settings.theme {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => cc.egui_ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(false),
        };
        let palette = theme::palette(dark);
        theme::apply(&cc.egui_ctx, &palette);
        let mut app = PaintApp {
            doc: Document::new(settings.new_width, settings.new_height),
            history: History::default(),
            settings,
            tool: Tool::Pencil,
            prev_tool: Tool::Pencil,
            zoom: 1.0,
            stroke: None,
            shape: None,
            selection: None,
            text: None,
            text_engine,
            dialog: None,
            toast: None,
            cursor_doc: None,
            fullscreen: false,
            show_thumbnail: false,
            palette,
            canvas: Default::default(),
            tiles: Vec::new(),
            tex_size: (0, 0),
            max_side: 2048,
            last_title: String::new(),
            dirty: Rect::new(0, 0, 0, 0),
            full_dirty: true,
            prev_overlay: Rect::new(0, 0, 0, 0),
            thumb_tex: None,
            thumb_gen: u64::MAX,
            checker: None,
            sel_tex: None,
            zoom_request: Some(ZoomRequest::Fit),
            allow_close: false,
            screenshot,
            screenshot_done: Default::default(),
            layer_thumbs: Vec::new(),
            layer_thumbs_gen: u64::MAX,
            script,
            pending_theme: None,
            ephemeral,
        };
        if let Some(f) = file {
            app.open_path(&f);
        }
        app
    }

    // ------------------------------------------------------------------ helpers

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.dialog = Some(Dialog::Message { title: "Paint".into(), text: msg.into() });
    }

    pub fn invalidate(&mut self, r: Rect) {
        self.dirty = self.dirty.union(&r);
    }

    pub fn invalidate_all(&mut self) {
        self.full_dirty = true;
    }

    pub fn set_theme(&mut self, ctx: &egui::Context, pref: ThemePref) {
        self.settings.theme = pref;
        let dark = match pref {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(false),
        };
        self.palette = theme::palette(dark);
        theme::apply(ctx, &self.palette);
        self.checker = None;
    }

    pub fn shape_params(&self) -> ShapeParams {
        ShapeParams {
            outline: self.settings.outline,
            fill: self.settings.fill,
            size: self.settings.shape_size,
            outline_color: self.settings.primary,
            fill_color: self.settings.secondary,
        }
    }

    pub fn transparent_key(&self) -> Option<Rgba> {
        self.settings.transparent_selection.then_some(self.settings.secondary)
    }

    pub fn set_tool(&mut self, t: Tool) {
        if t != self.tool {
            self.commit_pending();
            if t != Tool::Picker && t != Tool::Magnifier {
                self.prev_tool = t;
            } else if self.tool != Tool::Picker && self.tool != Tool::Magnifier {
                self.prev_tool = self.tool;
            }
            self.tool = t;
        }
    }

    // ------------------------------------------------------------------ commit / cancel

    /// Commit any floating shape, text box or selection to the active layer.
    pub fn commit_pending(&mut self) {
        self.commit_shape();
        self.commit_text();
        self.commit_selection();
    }

    pub fn commit_shape(&mut self) {
        if let Some(mut s) = self.shape.take() {
            if s.is_degenerate() {
                self.invalidate(s.bounds());
                return;
            }
            self.history.begin_patch(&self.doc);
            let r = s.commit(self.doc.active_pixels_mut());
            let label = s.kind.name().to_string();
            self.history.end_patch(&self.doc, &label, r);
            self.invalidate(r);
        }
    }

    pub fn commit_text(&mut self) {
        if let Some(mut t) = self.text.take() {
            let r = Rect::new(t.x.floor() as i32, t.y.floor() as i32, t.w.ceil() as i32 + 2, t.h.ceil() as i32 + 2);
            self.invalidate(r);
            if t.text.trim().is_empty() {
                return;
            }
            let bg = self.settings.text.fill_background.then_some(self.settings.secondary);
            let style = self.settings.text.clone();
            let p = t.render(&mut self.text_engine, &style, self.settings.primary, bg, self.doc.dpi).clone();
            self.history.begin_patch(&self.doc);
            raster::draw_over(self.doc.active_pixels_mut(), &p, t.x.round() as i32, t.y.round() as i32, 1.0);
            self.history.end_patch(&self.doc, "Draw text", r);
        }
    }

    pub fn commit_selection(&mut self) {
        if let Some(mut s) = self.selection.take() {
            self.invalidate(s.dest.union(&s.src).inflate(2));
            if s.is_lifted() {
                let key = self.transparent_key();
                if let Some(r) = s.commit(&mut self.doc, key) {
                    self.invalidate(r);
                }
                self.history.end_selection(&self.doc, "Move selection");
            }
        }
    }

    /// Lift the current selection off the canvas (starting an undoable move).
    pub fn lift_selection(&mut self) {
        if let Some(s) = self.selection.as_mut() {
            if !s.is_lifted() {
                self.history.begin_selection(&self.doc);
                s.lift(&mut self.doc);
                let r = s.src;
                self.invalidate(r);
            }
        }
    }

    pub fn deselect(&mut self) {
        self.commit_selection();
    }

    // ------------------------------------------------------------------ file commands

    pub fn request(&mut self, after: After) {
        self.commit_pending();
        if self.history.is_dirty() {
            self.dialog = Some(Dialog::Unsaved { after });
        } else {
            self.proceed(after);
        }
    }

    pub fn proceed(&mut self, after: After) {
        match after {
            After::New => self.new_document(),
            After::Open(Some(p)) => self.open_path(&p),
            After::Open(None) => {
                if let Some(p) = self.pick_open_file() {
                    self.open_path(&p);
                }
            }
            After::Exit => {
                self.allow_close = true;
            }
        }
    }

    /// Drop every uncommitted object and in-flight pointer interaction.
    fn reset_interaction(&mut self) {
        self.selection = None;
        self.shape = None;
        self.text = None;
        self.stroke = None;
        self.canvas.cancel_interaction();
    }

    pub fn new_document(&mut self) {
        self.reset_interaction();
        self.doc = Document::new(self.settings.new_width, self.settings.new_height);
        self.history.clear();
        self.invalidate_all();
        self.zoom_request = Some(ZoomRequest::Fit);
    }

    fn pick_open_file(&self) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new()
            .set_title("Open")
            .add_filter("All picture files", io::OPEN_EXTENSIONS)
            .add_filter("Bitmap files", &["bmp", "dib", "rle"])
            .add_filter("JPEG", &["jpg", "jpeg", "jpe", "jfif"])
            .add_filter("PNG", &["png"])
            .add_filter("GIF", &["gif"])
            .add_filter("TIFF", &["tif", "tiff"])
            .add_filter("WEBP", &["webp"])
            .add_filter("ICO", &["ico"])
            .add_filter("Paint project", &["paint"])
            .add_filter("All files", &["*"]);
        if let Some(dir) = self.doc.path.as_ref().and_then(|p| p.parent()) {
            d = d.set_directory(dir);
        }
        d.pick_file()
    }

    pub fn open_path(&mut self, p: &Path) {
        match io::load(p) {
            Ok(doc) => {
                self.reset_interaction();
                self.doc = doc;
                self.history.clear();
                self.settings.add_recent(p.to_path_buf());
                self.invalidate_all();
                self.zoom_request = Some(ZoomRequest::Fit);
            }
            Err(e) => {
                self.settings.recent.retain(|r| r != p);
                self.error(format!("{e:#}"));
            }
        }
    }

    pub fn save(&mut self) -> bool {
        self.commit_pending();
        match self.doc.path.clone() {
            // A picture opened from a read-only format (TGA, PNM, QOI) can't be written back
            // under its own name, so ask for a new one instead of mislabelling a PNG.
            Some(p) => match Format::from_path(&p) {
                Some(f) => self.save_to(&p, f),
                None => self.save_as(None),
            },
            None => self.save_as(None),
        }
    }

    pub fn save_as(&mut self, format: Option<Format>) -> bool {
        self.commit_pending();
        let fmt = format.unwrap_or_else(|| {
            self.doc.path.as_deref().and_then(Format::from_path).unwrap_or(if self.doc.layers.len() > 1 { Format::Project } else { Format::Png })
        });
        let stem = self.doc.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
        let mut d = rfd::FileDialog::new().set_title("Save as").set_file_name(format!("{stem}.{}", fmt.extensions()[0]));
        d = d.add_filter(fmt.label(), fmt.extensions());
        for f in Format::SAVE_CHOICES {
            if f != fmt {
                d = d.add_filter(f.label(), f.extensions());
            }
        }
        if let Some(dir) = self.doc.path.as_ref().and_then(|p| p.parent()) {
            d = d.set_directory(dir);
        }
        let Some(mut path) = d.save_file() else { return false };
        let f = match Format::from_path(&path) {
            Some(f) => f,
            None => {
                // Append rather than replace, so "v1.2" becomes "v1.2.png" and not "v1.png".
                let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
                name.push(".");
                name.push(fmt.extensions()[0]);
                path.set_file_name(name);
                fmt
            }
        };
        self.save_to(&path, f)
    }

    pub fn save_to(&mut self, path: &Path, f: Format) -> bool {
        let flattened = !f.supports_layers() && self.doc.layers.len() > 1;
        let lost_alpha = !f.supports_alpha() && !self.doc.background_visible;
        match io::save(&self.doc, path, f) {
            Ok(()) => {
                match (flattened, lost_alpha) {
                    (true, true) => self.notify("Layers were flattened and transparency was replaced with white in the saved file."),
                    (true, false) => self.notify("Layers were flattened in the saved file. Save as a Paint project to keep them."),
                    (false, true) => self.notify("Transparency is not supported by this format and was replaced with white."),
                    (false, false) => {}
                }
                self.doc.path = Some(path.to_path_buf());
                self.doc.file_size = std::fs::metadata(path).ok().map(|m| m.len());
                self.history.mark_saved();
                self.settings.add_recent(path.to_path_buf());
                self.persist_settings();
                true
            }
            Err(e) => {
                self.error(format!("Couldn't save {}.\n{e:#}", path.display()));
                false
            }
        }
    }

    /// Paste a picture from a file as a floating selection ("Import to canvas").
    pub fn import_file(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("All picture files", io::OPEN_EXTENSIONS).pick_file() else { return };
        match io::load_pixmap(&p) {
            Ok(px) => self.paste_pixmap(px),
            Err(e) => self.error(format!("{e:#}")),
        }
    }

    // ------------------------------------------------------------------ edit commands

    pub fn undo(&mut self) {
        if self.stroke.is_some() {
            return; // mid-stroke: the pending patch would no longer match the layer
        }
        if self.shape.is_some() || self.text.is_some() {
            // Discard the uncommitted object, like Paint's first Ctrl+Z.
            if let Some(s) = self.shape.take() {
                self.invalidate(s.bounds());
            }
            if let Some(t) = self.text.take() {
                self.invalidate(Rect::new(t.x as i32, t.y as i32, t.w as i32 + 2, t.h as i32 + 2));
            }
            return;
        }
        if let Some(s) = self.selection.take() {
            if s.is_lifted() {
                // Put the pixels back by finishing the patch, then undo it.
                self.selection = Some(s);
                self.commit_selection();
            }
        }
        let before = (self.doc.width, self.doc.height);
        if self.history.undo(&mut self.doc).is_some() {
            self.after_history(before);
        }
    }

    pub fn redo(&mut self) {
        if self.stroke.is_some() {
            return;
        }
        self.commit_pending();
        let before = (self.doc.width, self.doc.height);
        if self.history.redo(&mut self.doc).is_some() {
            self.after_history(before);
        }
    }

    fn after_history(&mut self, before: (u32, u32)) {
        self.invalidate_all();
        if before != (self.doc.width, self.doc.height) {
            self.canvas.resize_drag = None;
        }
    }

    pub fn copy(&mut self) {
        let img = if let Some(t) = &self.text {
            if let Some(s) = t.selected_text()
                && let Err(e) = io::clipboard_set_text(s)
            {
                self.error(format!("Couldn't copy to the clipboard.\n{e}"));
            }
            return;
        } else if let Some(s) = &self.selection {
            match &s.content {
                Some(c) => {
                    if s.is_transformed() {
                        raster::resize_nearest(c, s.dest.w as u32, s.dest.h as u32)
                    } else {
                        c.clone()
                    }
                }
                None => s.extract(&self.doc.active_layer().pixels),
            }
        } else {
            return;
        };
        if let Err(e) = io::clipboard_set_image(&img) {
            self.error(format!("Couldn't copy to the clipboard.\n{e}"));
        }
    }

    pub fn cut(&mut self) {
        if self.selection.is_none() {
            return;
        }
        self.copy();
        self.delete_selection();
    }

    pub fn delete_selection(&mut self) {
        if let Some(s) = self.selection.take() {
            if s.is_lifted() {
                // Content already off the layer: just drop it.
                self.history.end_selection(&self.doc, "Delete");
            } else {
                self.history.begin_patch(&self.doc);
                crate::selection::clear_region(self.doc.active_pixels_mut(), s.src, s.mask.as_ref());
                self.history.end_patch(&self.doc, "Delete", s.src);
            }
            self.invalidate(s.dest.union(&s.src).inflate(2));
        }
    }

    pub fn paste(&mut self) {
        if let Some(t) = self.text.as_mut() {
            if let Ok(s) = io::clipboard_get_text() {
                t.insert(&s);
            }
            return;
        }
        match io::clipboard_get_image() {
            Ok(Some(p)) => self.paste_pixmap(p),
            Ok(None) => self.notify("There's no picture on the clipboard."),
            Err(e) => self.error(format!("Couldn't read the clipboard.\n{e}")),
        }
    }

    /// Paste as a floating selection at the top-left of the visible canvas, growing the canvas
    /// if the picture is larger (as Paint does).
    pub fn paste_pixmap(&mut self, p: tiny_skia::Pixmap) {
        let max = crate::doc::MAX_DIM;
        if p.width() > max || p.height() > max {
            self.error(format!("The picture is too large to paste ({} × {}).", p.width(), p.height()));
            return;
        }
        self.commit_pending();
        self.set_tool(Tool::Select);
        if p.width() > self.doc.width || p.height() > self.doc.height {
            let (w, h) = (p.width().max(self.doc.width), p.height().max(self.doc.height));
            self.history.doc_edit(&mut self.doc, "Resize", |d| d.resize_canvas(w, h));
            self.invalidate_all();
        }
        let (vx, vy) = self.canvas.visible_origin;
        let x = vx.clamp(0, self.doc.width.saturating_sub(p.width()) as i32);
        let y = vy.clamp(0, self.doc.height.saturating_sub(p.height()) as i32);
        self.history.begin_selection(&self.doc);
        let s = Selection::floating(p, x, y);
        self.invalidate(s.dest);
        self.selection = Some(s);
    }

    /// Paste a picture that egui already read from the clipboard (`Event::PasteImage`).
    pub fn paste_color_image(&mut self, img: &ColorImage) {
        let (w, h) = (img.size[0] as u32, img.size[1] as u32);
        if w == 0 || h == 0 || img.pixels.len() != (w as usize) * (h as usize) {
            return;
        }
        let mut p = raster::new_pixmap(w, h);
        for (d, s) in p.pixels_mut().iter_mut().zip(&img.pixels) {
            let [r, g, b, a] = s.to_array(); // already premultiplied
            *d = tiny_skia::PremultipliedColorU8::from_rgba(r.min(a), g.min(a), b.min(a), a).unwrap();
        }
        self.paste_pixmap(p);
    }

    pub fn select_all(&mut self) {
        if let Some(t) = self.text.as_mut() {
            t.select_all();
            return;
        }
        self.commit_pending();
        self.set_tool(Tool::Select);
        self.selection = Some(Selection::rect(self.doc.bounds()));
        self.invalidate_all();
    }

    pub fn invert_selection(&mut self) {
        self.commit_shape();
        self.commit_text();
        let Some(s) = self.selection.take() else {
            self.select_all();
            return;
        };
        let mut s = s;
        if s.is_lifted() {
            let key = self.transparent_key();
            s.commit(&mut self.doc, key);
            self.history.end_selection(&self.doc, "Move selection");
        }
        self.selection = s.inverted(self.doc.width, self.doc.height);
        self.invalidate_all();
    }

    pub fn crop(&mut self) {
        self.commit_shape();
        self.commit_text();
        let Some(mut s) = self.selection.take() else { return };
        // Crop keeps the selected pixels of every layer; floating content goes on the active one.
        if s.is_lifted() {
            let key = self.transparent_key();
            s.commit(&mut self.doc, key);
            self.history.end_selection(&self.doc, "Move selection");
        }
        let r = s.dest.clamp_to(self.doc.width, self.doc.height);
        if r.is_empty() {
            return;
        }
        let mask = if s.is_transformed() { None } else { s.mask.clone() };
        self.history.doc_edit(&mut self.doc, "Crop", |d| {
            d.transform_all(|p| {
                let mut c = raster::copy_rect(p, r);
                if let Some(m) = &mask {
                    if m.width() == r.w as u32 && m.height() == r.h as u32 {
                        crate::selection::apply_mask(&mut c, m);
                    }
                }
                c
            });
        });
        self.invalidate_all();
    }

    /// Apply a pixel transform to the selection if there is one, otherwise the whole image.
    fn image_op(&mut self, label: &str, f: impl Fn(&tiny_skia::Pixmap) -> tiny_skia::Pixmap) {
        self.commit_shape();
        self.commit_text();
        if self.selection.is_some() {
            self.lift_selection();
            if let Some(s) = self.selection.as_mut() {
                let before = s.dest;
                s.map_content(&f);
                let after = s.dest;
                self.invalidate(before.union(&after).inflate(2));
            }
            return;
        }
        self.history.doc_edit(&mut self.doc, label, |d| d.transform_all(&f));
        self.invalidate_all();
    }

    pub fn rotate(&mut self, degrees: f32) {
        let q = degrees.rem_euclid(360.0);
        if self.selection.is_none() {
            // The canvas grows to fit the rotated picture; keep it within the size limit.
            let (w, h) = (self.doc.width as f32, self.doc.height as f32);
            let (s, c) = (q.to_radians().sin().abs(), q.to_radians().cos().abs());
            if (w * c + h * s).max(w * s + h * c) > crate::doc::MAX_DIM as f32 + 0.5 {
                self.error("The rotated picture would be too large.");
                return;
            }
        }
        if (q % 90.0).abs() < 1e-3 {
            let quarters = (q / 90.0).round() as u32;
            self.image_op("Rotate", move |p| raster::rotate_quarter(p, quarters));
        } else {
            self.image_op("Rotate", move |p| raster::rotate_any(p, q));
        }
    }

    pub fn flip(&mut self, horizontal: bool) {
        if horizontal {
            self.image_op("Horizontal flip", raster::flip_h);
        } else {
            self.image_op("Vertical flip", raster::flip_v);
        }
    }

    pub fn invert_colors(&mut self) {
        self.image_op("Invert colors", |p| {
            let mut q = p.clone();
            raster::invert(&mut q);
            q
        });
    }

    pub fn resize_image(&mut self, w: u32, h: u32, skew_h: f32, skew_v: f32) {
        let (w, h) = (w.clamp(1, crate::doc::MAX_DIM), h.clamp(1, crate::doc::MAX_DIM));
        let (sw, sh) = raster::skewed_size(w, h, skew_h, skew_v);
        if sw.max(sh) > crate::doc::MAX_DIM as f32 {
            self.error("The skewed picture would be too large. Try a smaller angle.");
            return;
        }
        if self.selection.as_ref().is_some_and(|s| s.is_lifted() || s.src != self.doc.bounds()) {
            self.lift_selection();
            if let Some(s) = self.selection.as_mut() {
                let before = s.dest;
                s.map_content(|p| {
                    let r = raster::resize(p, w, h);
                    if skew_h != 0.0 || skew_v != 0.0 { raster::skew(&r, skew_h, skew_v) } else { r }
                });
                let after = s.dest;
                self.invalidate(before.union(&after).inflate(2));
            }
            return;
        }
        self.selection = None;
        self.commit_shape();
        self.commit_text();
        let label = if skew_h != 0.0 || skew_v != 0.0 { "Skew" } else { "Resize" };
        self.history.doc_edit(&mut self.doc, label, |d| {
            d.transform_all(|p| {
                let r = raster::resize(p, w, h);
                if skew_h != 0.0 || skew_v != 0.0 { raster::skew(&r, skew_h, skew_v) } else { r }
            })
        });
        self.invalidate_all();
    }

    pub fn clear_image(&mut self) {
        self.commit_pending();
        self.history.doc_edit(&mut self.doc, "Clear", |d| {
            for l in &mut d.layers {
                l.pixels_mut().fill(tiny_skia::Color::TRANSPARENT);
            }
        });
        self.invalidate_all();
    }

    // ------------------------------------------------------------------ layers

    pub fn add_layer(&mut self) {
        self.commit_pending();
        let name = self.doc.next_layer_name();
        self.history.doc_edit(&mut self.doc, "Insert layer", |d| {
            let l = crate::doc::Layer::new(name, d.width, d.height);
            d.layers.insert(d.active + 1, l);
            d.active += 1;
        });
        self.invalidate_all();
    }

    pub fn duplicate_layer(&mut self) {
        self.commit_pending();
        self.history.doc_edit(&mut self.doc, "Duplicate layer", |d| {
            let mut l = d.layers[d.active].clone();
            l.name = format!("{} copy", l.name);
            d.layers.insert(d.active + 1, l);
            d.active += 1;
        });
        self.invalidate_all();
    }

    pub fn delete_layer(&mut self) {
        self.commit_pending();
        if self.doc.layers.len() <= 1 {
            self.history.doc_edit(&mut self.doc, "Remove layer", |d| d.layers[0].pixels_mut().fill(tiny_skia::Color::TRANSPARENT));
        } else {
            self.history.doc_edit(&mut self.doc, "Remove layer", |d| {
                d.layers.remove(d.active);
                d.active = d.active.saturating_sub(1).min(d.layers.len() - 1);
            });
        }
        self.invalidate_all();
    }

    pub fn merge_down(&mut self) {
        self.commit_pending();
        if self.doc.active == 0 {
            return;
        }
        self.history.doc_edit(&mut self.doc, "Collapse layer", |d| d.merge_down(d.active));
        self.invalidate_all();
    }

    pub fn move_layer(&mut self, up: bool) {
        self.commit_pending();
        let i = self.doc.active;
        let j = if up { i + 1 } else { i.wrapping_sub(1) };
        if j >= self.doc.layers.len() {
            return;
        }
        self.history.doc_edit(&mut self.doc, "Move layer", |d| {
            d.layers.swap(i, j);
            d.active = j;
        });
        self.invalidate_all();
    }

    pub fn toggle_layer_visibility(&mut self, i: usize) {
        self.commit_pending();
        self.history.doc_edit(&mut self.doc, "Change layer visibility", |d| d.layers[i].visible = !d.layers[i].visible);
        self.invalidate_all();
    }

    pub fn select_layer(&mut self, i: usize) {
        if i != self.doc.active && i < self.doc.layers.len() {
            self.commit_pending();
            self.doc.active = i;
        }
    }

    pub fn flatten_layers(&mut self) {
        self.commit_pending();
        if self.doc.layers.len() < 2 {
            return;
        }
        self.history.doc_edit(&mut self.doc, "Flatten", |d| {
            let mut base = raster::new_pixmap(d.width, d.height);
            for l in d.layers.iter().filter(|l| l.visible) {
                raster::draw_over(&mut base, &l.pixels, 0, 0, 1.0);
            }
            d.layers = vec![crate::doc::Layer { name: "Layer 1".into(), visible: true, pixels: base.into() }];
            d.active = 0;
        });
        self.invalidate_all();
    }

    pub fn set_background_visible(&mut self, v: bool) {
        // A floating selection must land first, or the snapshot would record its hole.
        self.commit_pending();
        self.history.doc_edit(&mut self.doc, "Change background visibility", |d| d.background_visible = v);
        self.invalidate_all();
    }

    // ------------------------------------------------------------------ view

    pub fn zoom_in(&mut self, at: Option<egui::Pos2>) {
        let z = ZOOM_LEVELS.iter().copied().find(|&z| z > self.zoom * 1.001).unwrap_or(self.zoom);
        self.zoom_request = Some(match at {
            Some(p) => ZoomRequest::At(z, p),
            None => ZoomRequest::To(z),
        });
    }

    pub fn zoom_out(&mut self, at: Option<egui::Pos2>) {
        let z = ZOOM_LEVELS.iter().rev().copied().find(|&z| z < self.zoom * 0.999).unwrap_or(self.zoom);
        self.zoom_request = Some(match at {
            Some(p) => ZoomRequest::At(z, p),
            None => ZoomRequest::To(z),
        });
    }

    pub fn adjust_size(&mut self, delta: f32) {
        if let Some(k) = self.tool.size_kind(&self.settings) {
            let s = self.settings.size(k);
            let step = if s >= 20.0 { 2.0 } else { 1.0 };
            self.settings.set_size(k, s + delta * step);
            let v = self.settings.size(k);
            self.notify(format!("Size {v:.0} px"));
        } else if self.tool == Tool::Shape {
            self.settings.shape_size = (self.settings.shape_size + delta).clamp(1.0, 50.0);
            if let Some(s) = self.shape.as_mut() {
                s.params.size = self.settings.shape_size;
            }
        }
    }

    // ------------------------------------------------------------------ rendering

    /// Collect everything floating above the active layer.
    fn overlay_rect(&self) -> Rect {
        // Strokes report their own dirty rectangles from `Stroke::finish`.
        let mut r = Rect::new(0, 0, 0, 0);
        if let Some(s) = &self.shape {
            r = r.union(&s.bounds());
        }
        if let Some(s) = &self.selection {
            if s.is_lifted() && !self.selection_drawn_separately() {
                r = r.union(&s.dest.union(&s.src).inflate(1));
            }
        }
        if let Some(t) = &self.text {
            r = r.union(&Rect::new(t.x as i32 - 1, t.y as i32 - 1, t.w as i32 + 3, t.h as i32 + 3));
        }
        r
    }

    /// A floating selection on the top-most visible layer is drawn as its own texture so that
    /// moving it doesn't recomposite the canvas every frame.
    pub fn selection_drawn_separately(&self) -> bool {
        self.selection.as_ref().is_some_and(|s| {
            s.content.as_ref().is_some_and(|c| c.width() <= self.max_side && c.height() <= self.max_side)
        }) && self.doc.layers.iter().skip(self.doc.active + 1).all(|l| !l.visible)
    }

    pub fn selection_texture(&mut self, ctx: &egui::Context) -> Option<TextureHandle> {
        let key = self.transparent_key();
        let s = self.selection.as_mut()?;
        s.prepare(key);
        let v = s.version;
        if self.sel_tex.as_ref().map(|(ver, _)| *ver) != Some(v) {
            let p = s.display_pixels(key)?;
            let img = ColorImage::from_rgba_premultiplied([p.width() as usize, p.height() as usize], p.data());
            let opts = TextureOptions { magnification: egui::TextureFilter::Nearest, ..TextureOptions::LINEAR };
            self.sel_tex = Some((v, ctx.load_texture("selection", img, opts)));
        }
        self.sel_tex.as_ref().map(|(_, t)| t.clone())
    }

    /// Bring the canvas textures up to date with everything that changed since last frame.
    pub fn update_canvas(&mut self, ctx: &egui::Context, overlays_changed: bool) {
        let size = (self.doc.width, self.doc.height);
        let side = (ctx.input(|i| i.max_texture_side).min(crate::doc::MAX_DIM as usize) as u32).max(256);
        if self.tiles.is_empty() || self.tex_size != size || self.max_side != side {
            self.tiles.clear();
            self.tex_size = size;
            self.max_side = side;
            self.full_dirty = true;
        }
        if overlays_changed {
            let cur = self.overlay_rect();
            let prev = self.prev_overlay;
            self.invalidate(prev.union(&cur));
            self.prev_overlay = cur;
        }
        let region = if self.full_dirty { self.doc.bounds() } else { self.dirty.clamp_to(size.0, size.1) };
        if !region.is_empty() {
            // Gather overlays.
            let key = self.transparent_key();
            if let Some(s) = self.selection.as_mut() {
                s.prepare(key);
            }
            let (dw, dh) = (self.doc.width, self.doc.height);
            let separate = self.selection_drawn_separately();
            let mut ovs: Vec<Overlay> = Vec::new();
            if let Some(s) = &self.stroke {
                ovs.push(Overlay { pixmap: &s.stroke.out, x: 0, y: 0, opacity: s.stroke.opacity, erase: s.stroke.is_eraser(), transform: None });
            }
            if let Some(sh) = self.shape.as_mut() {
                ovs.extend(sh.overlays(dw, dh));
            }
            if let Some(sel) = self.selection.as_ref().filter(|_| !separate) {
                if let Some(o) = sel.overlay(key) {
                    ovs.push(o);
                }
            }
            if let Some(t) = self.text.as_mut() {
                let bg = self.settings.text.fill_background.then_some(self.settings.secondary);
                let (x, y) = (t.x.round() as i32, t.y.round() as i32);
                let px = t.render(&mut self.text_engine, &self.settings.text, self.settings.primary, bg, self.doc.dpi);
                ovs.push(Overlay { pixmap: px, x, y, opacity: 1.0, erase: false, transform: None });
            }
            let opts = TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                wrap_mode: egui::TextureWrapMode::ClampToEdge,
                mipmap_mode: None,
            };
            let image = |doc: &Document, r: Rect| {
                let px = doc.composite(r, &ovs);
                ColorImage::from_rgba_premultiplied([r.w as usize, r.h as usize], px.data())
            };
            if self.tiles.is_empty() {
                let step = side as usize;
                for y in (0..dh).step_by(step) {
                    for x in (0..dw).step_by(step) {
                        let rect = Rect::new(x as i32, y as i32, side.min(dw - x) as i32, side.min(dh - y) as i32);
                        let tex = ctx.load_texture("canvas", image(&self.doc, rect), opts);
                        self.tiles.push(Tile { rect, tex });
                    }
                }
            } else {
                for t in &mut self.tiles {
                    let r = region.intersect(&t.rect);
                    if r == t.rect {
                        t.tex.set(image(&self.doc, r), opts);
                    } else if !r.is_empty() {
                        t.tex.set_partial([(r.x - t.rect.x) as usize, (r.y - t.rect.y) as usize], image(&self.doc, r), opts);
                    }
                }
            }
        }
        self.full_dirty = false;
        self.dirty = Rect::new(0, 0, 0, 0);
    }

    /// The canvas textures and the document rectangle each one covers.
    pub fn canvas_tiles(&self) -> Vec<(Rect, egui::TextureId)> {
        self.tiles.iter().map(|t| (t.rect, t.tex.id())).collect()
    }

    pub fn thumbnail_texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if self.thumb_tex.is_none() || self.thumb_gen != self.history.generation {
            let flat = self.doc.flatten();
            let s = (256.0 / self.doc.width.max(self.doc.height) as f32).min(1.0);
            let t = raster::resize(&flat, ((self.doc.width as f32 * s) as u32).max(1), ((self.doc.height as f32 * s) as u32).max(1));
            let img = ColorImage::from_rgba_premultiplied([t.width() as usize, t.height() as usize], t.data());
            self.thumb_tex = Some(ctx.load_texture("thumb", img, TextureOptions::LINEAR));
            self.thumb_gen = self.history.generation;
        }
        self.thumb_tex.clone().unwrap()
    }

    pub fn layer_thumbnails(&mut self, ctx: &egui::Context) {
        if self.layer_thumbs_gen == self.history.generation && self.layer_thumbs.len() == self.doc.layers.len() {
            return;
        }
        self.layer_thumbs_gen = self.history.generation;
        let s = (64.0 / self.doc.width.max(self.doc.height) as f32).min(1.0);
        let (tw, th) = (((self.doc.width as f32 * s) as u32).max(1), ((self.doc.height as f32 * s) as u32).max(1));
        self.layer_thumbs = self
            .doc
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let t = raster::resize(&l.pixels, tw, th);
                let img = ColorImage::from_rgba_premultiplied([tw as usize, th as usize], t.data());
                Some(ctx.load_texture(format!("layer{i}"), img, TextureOptions::LINEAR))
            })
            .collect();
    }

    pub fn checker_texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if self.checker.is_none() {
            let (a, b) = if self.palette.dark { (70u8, 90u8) } else { (255u8, 214u8) };
            let mut img = ColorImage::filled([16, 16], egui::Color32::WHITE);
            for y in 0..16 {
                for x in 0..16 {
                    let v = if (x / 8 + y / 8) % 2 == 0 { a } else { b };
                    img.pixels[y * 16 + x] = egui::Color32::from_gray(v);
                }
            }
            let opts = TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..TextureOptions::NEAREST };
            self.checker = Some(ctx.load_texture("checker", img, opts));
        }
        self.checker.clone().unwrap()
    }

    // ------------------------------------------------------------------ frame

    pub fn persist_settings(&self) {
        if !self.ephemeral {
            self.settings.save();
        }
    }

    fn window_title(&self) -> String {
        format!("{}{} - Paint", if self.history.is_dirty() { "*" } else { "" }, self.doc.title())
    }
}

impl eframe::App for PaintApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Window close: ask to save first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.dialog.is_none() {
                self.request(After::Exit);
            }
        }
        if let Some(t) = self.pending_theme.take() {
            self.set_theme(ctx, t);
        }
        // Follow the desktop's light/dark switch while "Use system setting" is selected.
        if self.settings.theme == ThemePref::System {
            if let Some(t) = ctx.system_theme() {
                if (t == egui::Theme::Dark) != self.palette.dark {
                    self.set_theme(ctx, ThemePref::System);
                }
            }
        }
        if self.script.as_ref().is_some_and(|s| s.finished) {
            self.allow_close = true;
        }
        if self.allow_close {
            self.persist_settings();
            io::clipboard_shutdown();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Only when it changes: every viewport command schedules a repaint, so sending the
        // title each frame would keep the app redrawing forever while idle.
        let title = self.window_title();
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }

        // Files dropped onto the window open (or import when holding Ctrl).
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if let Some(p) = dropped.into_iter().next() {
            if ctx.input(|i| i.modifiers.command) {
                match io::load_pixmap(&p) {
                    Ok(px) => self.paste_pixmap(px),
                    Err(e) => self.error(format!("{e:#}")),
                }
            } else {
                self.request(After::Open(Some(p)));
            }
        }

        if let Some((_, t)) = &self.toast {
            if t.elapsed().as_secs_f32() > 3.0 {
                self.toast = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::ui::shortcuts::handle(self, &ctx);
        if !self.fullscreen {
            crate::ui::chrome::menu_bar(self, ui);
            crate::ui::toolbar::toolbar(self, ui);
            if self.settings.show_status_bar {
                crate::ui::chrome::status_bar(self, ui);
            }
            if self.settings.show_layers {
                crate::ui::chrome::layers_panel(self, ui);
            }
        }
        crate::ui::canvas::canvas(self, ui);
        crate::ui::dialogs::show(self, &ctx);
        crate::ui::chrome::toast(self, &ctx);
        if self.show_thumbnail {
            crate::ui::chrome::thumbnail_window(self, &ctx);
        }

        if let Some((path, frames)) = self.screenshot.as_mut() {
            *frames = frames.saturating_sub(1);
            if *frames == 1 {
                let path = path.clone();
                let done = self.screenshot_done.clone();
                let cb = egui::ScreenshotCallback::new(move |img| {
                    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                    if let Some(buf) = image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, rgba) {
                        let _ = buf.save(&path);
                    }
                    done.store(true, std::sync::atomic::Ordering::SeqCst);
                });
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
            }
            if self.screenshot_done.load(std::sync::atomic::Ordering::SeqCst) {
                self.allow_close = true;
            }
            ctx.request_repaint();
        }
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.persist_settings();
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.drive_script(ctx, raw);
    }
}
