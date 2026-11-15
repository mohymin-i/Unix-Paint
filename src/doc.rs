//! Document model: a stack of layers over an optional solid background, as in Windows 11 Paint.

use crate::color::Rgba;
use crate::raster::{self, Rect};
use std::path::PathBuf;
use std::sync::Arc;
use tiny_skia::{BlendMode, Pixmap, PixmapPaint, Transform};

pub const DEFAULT_WIDTH: u32 = 1152;
pub const DEFAULT_HEIGHT: u32 = 648;
pub const MAX_DIM: u32 = 16384;

#[derive(Clone)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    /// Shared copy-on-write, so undo snapshots only cost memory for layers that changed.
    pub pixels: Arc<Pixmap>,
}

impl Layer {
    pub fn new(name: impl Into<String>, w: u32, h: u32) -> Self {
        Layer { name: name.into(), visible: true, pixels: Arc::new(raster::new_pixmap(w, h)) }
    }

    pub fn pixels_mut(&mut self) -> &mut Pixmap {
        Arc::make_mut(&mut self.pixels)
    }
}

/// Everything the undo system needs to restore a structural change.
#[derive(Clone)]
pub struct DocState {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub active: usize,
    pub background: Rgba,
    pub background_visible: bool,
}

impl DocState {
    /// Bytes held by this snapshot that `other` doesn't already share.
    pub fn byte_size_excluding(&self, other: Option<&DocState>) -> usize {
        self.layers
            .iter()
            .filter(|l| !other.is_some_and(|o| o.layers.iter().any(|m| Arc::ptr_eq(&l.pixels, &m.pixels))))
            .map(|l| l.pixels.data().len())
            .sum()
    }
}

pub struct Document {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub active: usize,
    pub background: Rgba,
    pub background_visible: bool,
    pub dpi: f32,
    pub monochrome: bool,
    pub path: Option<PathBuf>,
    /// File size on disk of the last load/save, for the status bar.
    pub file_size: Option<u64>,
    layer_counter: u32,
}

/// Something drawn on top of (or erased from) the active layer before it is committed.
pub struct Overlay<'a> {
    pub pixmap: &'a Pixmap,
    pub x: i32,
    pub y: i32,
    pub opacity: f32,
    pub erase: bool,
    /// Optional affine transform from overlay pixel space to document space (for scaled /
    /// rotated floating selections). When set, x/y are ignored.
    pub transform: Option<Transform>,
}

impl Document {
    pub fn new(w: u32, h: u32) -> Self {
        Document {
            width: w,
            height: h,
            layers: vec![Layer::new("Layer 1", w, h)],
            active: 0,
            background: Rgba::WHITE,
            background_visible: true,
            dpi: 96.0,
            monochrome: false,
            path: None,
            file_size: None,
            layer_counter: 1,
        }
    }

    /// Build a single-layer document from an image. Opaque images go on a layer over a white
    /// background; images with transparency hide the background so it is preserved.
    pub fn from_pixmap(p: Pixmap) -> Self {
        let mut d = Document::new(p.width(), p.height());
        d.background_visible = !raster::has_transparency(&p);
        d.layers[0].pixels = Arc::new(p);
        d
    }

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into())
    }

    pub fn active_layer(&self) -> &Layer {
        &self.layers[self.active]
    }

    pub fn active_pixels_mut(&mut self) -> &mut Pixmap {
        self.layers[self.active].pixels_mut()
    }

    pub fn next_layer_name(&mut self) -> String {
        self.layer_counter += 1;
        format!("Layer {}", self.layer_counter)
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    pub fn snapshot(&self) -> DocState {
        DocState {
            width: self.width,
            height: self.height,
            layers: self.layers.clone(),
            active: self.active,
            background: self.background,
            background_visible: self.background_visible,
        }
    }

    pub fn restore(&mut self, s: DocState) {
        self.width = s.width;
        self.height = s.height;
        self.layers = s.layers;
        self.active = s.active.min(self.layers.len().saturating_sub(1));
        self.background = s.background;
        self.background_visible = s.background_visible;
    }

    /// Composite `rect` of the document into a new pixmap of the rect's size.
    pub fn composite(&self, rect: Rect, overlays: &[Overlay]) -> Pixmap {
        let mut out = raster::new_pixmap(rect.w.max(1) as u32, rect.h.max(1) as u32);
        if self.background_visible {
            out.fill(self.background.to_skia());
        }
        for (i, layer) in self.layers.iter().enumerate() {
            if !layer.visible {
                continue;
            }
            let erasers: Vec<&Overlay> =
                if i == self.active { overlays.iter().filter(|o| o.erase).collect() } else { vec![] };
            if erasers.is_empty() {
                raster::draw_over(&mut out, &layer.pixels, -rect.x, -rect.y, 1.0);
            } else {
                let mut tmp = raster::copy_rect(&layer.pixels, rect);
                for o in erasers {
                    draw_overlay(&mut tmp, o, rect, BlendMode::DestinationOut);
                }
                raster::draw_over(&mut out, &tmp, 0, 0, 1.0);
            }
            if i == self.active {
                for o in overlays.iter().filter(|o| !o.erase) {
                    draw_overlay(&mut out, o, rect, BlendMode::SourceOver);
                }
            }
        }
        out
    }

    /// Flattened image of the whole document (what gets saved to flat formats).
    pub fn flatten(&self) -> Pixmap {
        self.composite(self.bounds(), &[])
    }

    /// Resize the canvas (not the image): anchored at top-left, new area transparent.
    pub fn resize_canvas(&mut self, w: u32, h: u32) {
        let w = w.clamp(1, MAX_DIM);
        let h = h.clamp(1, MAX_DIM);
        if (w, h) == (self.width, self.height) {
            return;
        }
        for l in &mut self.layers {
            let mut p = raster::new_pixmap(w, h);
            raster::put_rect(&mut p, &l.pixels, 0, 0);
            l.pixels = Arc::new(p);
        }
        self.width = w;
        self.height = h;
    }

    /// Apply a whole-image transform to every layer; the document takes the result's size.
    pub fn transform_all(&mut self, f: impl Fn(&Pixmap) -> Pixmap) {
        for l in &mut self.layers {
            l.pixels = Arc::new(f(&l.pixels));
        }
        self.width = self.layers[0].pixels.width();
        self.height = self.layers[0].pixels.height();
    }

    pub fn merge_down(&mut self, idx: usize) {
        if idx == 0 || idx >= self.layers.len() {
            return;
        }
        let top = self.layers.remove(idx);
        if top.visible {
            raster::draw_over(self.layers[idx - 1].pixels_mut(), &top.pixels, 0, 0, 1.0);
        }
        self.active = idx - 1;
    }
}

fn draw_overlay(dst: &mut Pixmap, o: &Overlay, rect: Rect, mode: BlendMode) {
    let paint = PixmapPaint {
        opacity: o.opacity,
        blend_mode: mode,
        // Nearest, like `Selection::commit`, so a resized selection previews exactly as it lands.
        quality: tiny_skia::FilterQuality::Nearest,
    };
    match o.transform {
        Some(t) => {
            let t = t.post_translate(-rect.x as f32, -rect.y as f32);
            dst.draw_pixmap(0, 0, o.pixmap.as_ref(), &paint, t, None);
        }
        None => dst.draw_pixmap(o.x - rect.x, o.y - rect.y, o.pixmap.as_ref(), &paint, Transform::identity(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_background_and_layers() {
        let mut d = Document::new(4, 4);
        d.layers[0].pixels_mut().fill(Rgba::rgb(255, 0, 0).to_skia());
        d.layers[0].pixels_mut().pixels_mut()[0] = tiny_skia::PremultipliedColorU8::TRANSPARENT;
        let flat = d.flatten();
        assert_eq!(raster::get_pixel(&flat, 0, 0), Some(Rgba::WHITE));
        assert_eq!(raster::get_pixel(&flat, 1, 0), Some(Rgba::rgb(255, 0, 0)));
        d.background_visible = false;
        assert_eq!(raster::get_pixel(&d.flatten(), 0, 0).unwrap().a, 0);
    }

    #[test]
    fn eraser_overlay_cuts_active_layer() {
        let mut d = Document::new(4, 4);
        d.layers[0].pixels_mut().fill(Rgba::BLACK.to_skia());
        let mut er = raster::new_pixmap(1, 1);
        er.fill(Rgba::BLACK.to_skia());
        let o = Overlay { pixmap: &er, x: 2, y: 2, opacity: 1.0, erase: true, transform: None };
        let c = d.composite(d.bounds(), &[o]);
        assert_eq!(raster::get_pixel(&c, 2, 2), Some(Rgba::WHITE));
        assert_eq!(raster::get_pixel(&c, 1, 2), Some(Rgba::BLACK));
    }

    #[test]
    fn merge_down_combines() {
        let mut d = Document::new(2, 1);
        d.layers[0].pixels_mut().pixels_mut()[0] = Rgba::BLACK.premul();
        let mut l2 = Layer::new("2", 2, 1);
        l2.pixels_mut().pixels_mut()[1] = Rgba::rgb(0, 255, 0).premul();
        d.layers.push(l2);
        d.merge_down(1);
        assert_eq!(d.layers.len(), 1);
        assert_eq!(raster::get_pixel(&d.layers[0].pixels, 1, 0), Some(Rgba::rgb(0, 255, 0)));
    }
}
