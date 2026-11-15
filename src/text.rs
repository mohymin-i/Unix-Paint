//! Text tool: an editable text box rendered with cosmic-text, rasterised onto the layer on commit.

use crate::color::Rgba;
use crate::raster;
use crate::settings::TextStyle;
use cosmic_text::{
    Align, Attrs, Buffer, Color, Cursor, Family, FontSystem, Metrics, Shaping, Style, SwashCache, UnderlineStyle,
    Weight, Wrap,
};
use tiny_skia::{Pixmap, PremultipliedColorU8};

pub struct TextEngine {
    pub fonts: FontSystem,
    cache: SwashCache,
    pub families: Vec<String>,
    pub default_family: String,
}

impl TextEngine {
    pub fn new() -> Self {
        let fonts = FontSystem::new();
        let mut families: Vec<String> = fonts
            .db()
            .faces()
            .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
            .filter(|n| !n.starts_with('.'))
            .collect();
        families.sort_by_key(|a| a.to_lowercase());
        families.dedup();
        let prefer = ["Segoe UI", "Inter", "Noto Sans", "Cantarell", "Ubuntu", "DejaVu Sans", "Helvetica Neue", "Arial"];
        let default_family = prefer
            .iter()
            .find(|p| families.iter().any(|f| f == *p))
            .map(|s| s.to_string())
            .or_else(|| families.first().cloned())
            .unwrap_or_else(|| "sans-serif".into());
        TextEngine { fonts, cache: SwashCache::new(), families, default_family }
    }

    /// Raw bytes of the regular face of a family, for using it as the UI font.
    pub fn family_bytes(&self, family: &str) -> Option<Vec<u8>> {
        let db = self.fonts.db();
        let face = db.faces().find(|f| {
            f.families.iter().any(|(n, _)| n == family) && f.style == fontdb_style_normal() && f.weight.0 == 400
        })?;
        db.with_face_data(face.id, |data, _| data.to_vec())
    }
}

fn fontdb_style_normal() -> cosmic_text::fontdb::Style {
    cosmic_text::fontdb::Style::Normal
}

pub struct TextBox {
    /// Box position and size in document pixels.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub text: String,
    /// Byte offset of the caret in `text`.
    pub caret: usize,
    /// Selection anchor (byte offset), if a range is selected.
    pub anchor: Option<usize>,
    buffer: Option<Buffer>,
    /// Cache key for the rendered pixmap.
    rendered: Option<(String, TextStyle, Rgba, Option<Rgba>, f32, Pixmap)>,
}

pub fn pt_to_px(pt: f32, dpi: f32) -> f32 {
    pt * dpi / 72.0
}

impl TextBox {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        TextBox { x, y, w: w.max(8.0), h: h.max(8.0), text: String::new(), caret: 0, anchor: None, buffer: None, rendered: None }
    }

    fn attrs<'a>(style: &'a TextStyle, family: &'a str, color: Rgba) -> Attrs<'a> {
        let mut a = Attrs::new().family(Family::Name(family)).color(Color::rgba(color.r, color.g, color.b, color.a));
        if style.bold {
            a = a.weight(Weight::BOLD);
        }
        if style.italic {
            a = a.style(Style::Italic);
        }
        if style.underline {
            a = a.underline(UnderlineStyle::Single);
        }
        if style.strikeout {
            a = a.strikethrough();
        }
        a
    }

    /// Lay out the text; grows the box height to fit like Paint's text box.
    pub fn layout(&mut self, eng: &mut TextEngine, style: &TextStyle, dpi: f32) {
        let px = pt_to_px(style.size_pt, dpi).max(1.0);
        let metrics = Metrics::new(px, (px * 1.25).ceil());
        let family = if style.family.is_empty() { eng.default_family.clone() } else { style.family.clone() };
        let buffer = self.buffer.get_or_insert_with(|| Buffer::new(&mut eng.fonts, metrics));
        buffer.set_metrics(metrics);
        buffer.set_wrap(Wrap::WordOrGlyph);
        buffer.set_size(Some(self.w), None);
        let align = match style.align {
            1 => Align::Center,
            2 => Align::Right,
            _ => Align::Left,
        };
        buffer.set_text(&self.text, &Self::attrs(style, &family, Rgba::BLACK), Shaping::Advanced, Some(align));
        buffer.shape_until_scroll(&mut eng.fonts, false);
        let mut bottom: f32 = metrics.line_height;
        for run in buffer.layout_runs() {
            bottom = bottom.max(run.line_top + run.line_height);
        }
        self.h = self.h.max(bottom + 2.0);
    }

    fn cursor_at(&self, byte: usize) -> Cursor {
        let before = &self.text[..byte.min(self.text.len())];
        let line = before.matches('\n').count();
        let idx = before.rfind('\n').map(|i| before.len() - i - 1).unwrap_or(before.len());
        Cursor::new(line, idx)
    }

    fn byte_of(&self, c: Cursor) -> usize {
        let mut off = 0;
        for (i, l) in self.text.split('\n').enumerate() {
            if i == c.line {
                return off + c.index.min(l.len());
            }
            off += l.len() + 1;
        }
        self.text.len()
    }

    /// Caret rectangle (x, top, height) relative to the box.
    pub fn caret_pos(&self) -> Option<(f32, f32, f32)> {
        let b = self.buffer.as_ref()?;
        let c = self.cursor_at(self.caret);
        let lh = b.metrics().line_height;
        Some(b.cursor_position(&c).map(|(x, y)| (x, y, lh)).unwrap_or((0.0, c.line as f32 * lh, lh)))
    }

    /// Highlight rectangles (x, y, w, h) relative to the box for the selected range.
    pub fn selection_rects(&self) -> Vec<(f32, f32, f32, f32)> {
        let (Some(a), Some(b)) = (self.anchor, self.buffer.as_ref()) else { return vec![] };
        if a == self.caret {
            return vec![];
        }
        let (s, e) = (a.min(self.caret), a.max(self.caret));
        let (cs, ce) = (self.cursor_at(s), self.cursor_at(e));
        let mut out = vec![];
        for run in b.layout_runs() {
            for (x, w) in run.highlight(cs, ce) {
                out.push((x, run.line_top, w.max(4.0), run.line_height));
            }
        }
        out
    }

    pub fn hit(&self, x: f32, y: f32) -> usize {
        match self.buffer.as_ref().and_then(|b| b.hit(x, y)) {
            Some(c) => self.byte_of(c),
            None => self.text.len(),
        }
    }

    fn delete_selection(&mut self) -> bool {
        if let Some(a) = self.anchor.take() {
            if a != self.caret {
                let (s, e) = (a.min(self.caret), a.max(self.caret));
                self.text.replace_range(s..e, "");
                self.caret = s;
                return true;
            }
        }
        false
    }

    pub fn selected_text(&self) -> Option<String> {
        let a = self.anchor?;
        if a == self.caret {
            return None;
        }
        Some(self.text[a.min(self.caret)..a.max(self.caret)].to_string())
    }

    pub fn insert(&mut self, s: &str) {
        self.delete_selection();
        let s: String = s.chars().filter(|c| *c == '\n' || !c.is_control()).collect();
        self.text.insert_str(self.caret, &s);
        self.caret += s.len();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() || self.caret == 0 {
            return;
        }
        let prev = self.text[..self.caret].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
        self.text.replace_range(prev..self.caret, "");
        self.caret = prev;
    }

    pub fn delete(&mut self) {
        if self.delete_selection() || self.caret >= self.text.len() {
            return;
        }
        let next = self.text[self.caret..].char_indices().nth(1).map(|(i, _)| self.caret + i).unwrap_or(self.text.len());
        self.text.replace_range(self.caret..next, "");
    }

    fn set_caret(&mut self, pos: usize, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = pos.min(self.text.len());
    }

    pub fn left(&mut self, extend: bool) {
        let p = self.text[..self.caret].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
        self.set_caret(p, extend);
    }

    pub fn right(&mut self, extend: bool) {
        let p = self.text[self.caret..].char_indices().nth(1).map(|(i, _)| self.caret + i).unwrap_or(self.text.len());
        self.set_caret(p, extend);
    }

    pub fn home(&mut self, extend: bool) {
        let p = self.text[..self.caret].rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.set_caret(p, extend);
    }

    pub fn end(&mut self, extend: bool) {
        let p = self.text[self.caret..].find('\n').map(|i| self.caret + i).unwrap_or(self.text.len());
        self.set_caret(p, extend);
    }

    pub fn vertical(&mut self, dir: i32, extend: bool) {
        if let Some((x, y, lh)) = self.caret_pos() {
            let p = self.hit(x + 0.5, y + lh * 0.5 + dir as f32 * lh);
            self.set_caret(p, extend);
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    pub fn click(&mut self, x: f32, y: f32, extend: bool) {
        let p = self.hit(x, y);
        self.set_caret(p, extend);
    }

    /// Render text (and optional background fill) into a box-sized pixmap.
    pub fn render(
        &mut self,
        eng: &mut TextEngine,
        style: &TextStyle,
        color: Rgba,
        background: Option<Rgba>,
        dpi: f32,
    ) -> &Pixmap {
        let key_ok = matches!(&self.rendered, Some((t, s, c, b, w, p))
            if *t == self.text && s == style && *c == color && *b == background && *w == self.w
               && p.height() == self.h.ceil() as u32);
        if !key_ok {
            self.layout(eng, style, dpi);
            let (w, h) = (self.w.ceil().max(1.0) as u32, self.h.ceil().max(1.0) as u32);
            let mut p = raster::new_pixmap(w, h);
            if let Some(bg) = background {
                p.fill(bg.to_skia());
            }
            let family = if style.family.is_empty() { eng.default_family.clone() } else { style.family.clone() };
            let buffer = self.buffer.as_mut().unwrap();
            buffer.set_text(&self.text, &Self::attrs(style, &family, color), Shaping::Advanced, None);
            let align = match style.align {
                1 => Align::Center,
                2 => Align::Right,
                _ => Align::Left,
            };
            for line in buffer.lines.iter_mut() {
                line.set_align(Some(align));
            }
            let (pw, ph) = (w as i32, h as i32);
            let pixels = p.pixels_mut();
            buffer.draw(&mut eng.fonts, &mut eng.cache, Color::rgba(color.r, color.g, color.b, color.a), |x, y, gw, gh, c| {
                for yy in y..y + gh as i32 {
                    for xx in x..x + gw as i32 {
                        if xx < 0 || yy < 0 || xx >= pw || yy >= ph {
                            continue;
                        }
                        let i = (yy * pw + xx) as usize;
                        blend(&mut pixels[i], Rgba { r: c.r(), g: c.g(), b: c.b(), a: c.a() });
                    }
                }
            });
            self.rendered = Some((self.text.clone(), style.clone(), color, background, self.w, p));
        }
        &self.rendered.as_ref().unwrap().5
    }
}

fn blend(dst: &mut PremultipliedColorU8, src: Rgba) {
    if src.a == 0 {
        return;
    }
    let s = src.premul();
    let ia = 255 - s.alpha() as u16;
    let m = |a: u8, b: u8| (a as u16 + (b as u16 * ia + 127) / 255).min(255) as u8;
    let a = m(s.alpha(), dst.alpha());
    let r = m(s.red(), dst.red()).min(a);
    let g = m(s.green(), dst.green()).min(a);
    let b = m(s.blue(), dst.blue()).min(a);
    *dst = PremultipliedColorU8::from_rgba(r, g, b, a).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_ops() {
        let mut t = TextBox::new(0.0, 0.0, 100.0, 20.0);
        t.insert("héllo");
        t.left(false);
        t.backspace();
        assert_eq!(t.text, "hélo");
        t.home(false);
        t.right(true);
        t.right(true);
        assert_eq!(t.selected_text().as_deref(), Some("hé"));
        t.insert("X");
        assert_eq!(t.text, "Xlo");
        t.insert("\nnext");
        t.select_all();
        t.delete();
        assert_eq!(t.text, "");
    }

    #[test]
    fn renders_ink() {
        let mut eng = TextEngine::new();
        if eng.families.is_empty() {
            return; // no system fonts available in this environment
        }
        let mut t = TextBox::new(0.0, 0.0, 200.0, 20.0);
        t.insert("Paint");
        let p = t.render(&mut eng, &TextStyle { size_pt: 24.0, ..Default::default() }, Rgba::BLACK, None, 96.0);
        assert!(p.pixels().iter().any(|c| c.alpha() > 200));
    }
}
