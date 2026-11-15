//! Rectangular / free-form selections and the floating content they lift off the canvas.

use crate::color::Rgba;
use crate::doc::{Document, Overlay};
use crate::raster::{self, Rect};
use tiny_skia::{FillRule, Mask, PathBuilder, Pixmap, PixmapPaint, Transform};

pub struct Selection {
    /// Region of the canvas the selection was taken from (pixel aligned).
    pub src: Rect,
    /// Current placement of the content; differs from `src` once moved or resized.
    pub dest: Rect,
    /// Per-pixel mask in `src`-local coordinates (free-form or inverted selections).
    pub mask: Option<Mask>,
    /// Outline polygons in `src`-local coordinates, for drawing marching ants.
    pub outline: Vec<Vec<(f32, f32)>>,
    /// Lifted pixels (src-sized). `None` while the selection is just a marquee.
    pub content: Option<Pixmap>,
    /// Cached content with the transparent-selection key applied.
    keyed: Option<(Rgba, Pixmap)>,
    /// Bumped whenever the displayed pixels change (for texture caching).
    pub version: u64,
}

impl Selection {
    pub fn rect(r: Rect) -> Self {
        let outline = vec![vec![(0.0, 0.0), (r.w as f32, 0.0), (r.w as f32, r.h as f32), (0.0, r.h as f32)]];
        Selection { src: r, dest: r, mask: None, outline, content: None, keyed: None, version: 0 }
    }

    /// Free-form selection from a lasso polygon in document coordinates.
    pub fn free_form(points: &[(f32, f32)], bounds: Rect) -> Option<Self> {
        if points.len() < 3 {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in points {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let r = Rect::from_points(x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32 + 1, y1.ceil() as i32 + 1)
            .intersect(&bounds);
        if r.w < 1 || r.h < 1 {
            return None;
        }
        let local: Vec<(f32, f32)> = points.iter().map(|&(x, y)| (x - r.x as f32, y - r.y as f32)).collect();
        let mut pb = PathBuilder::new();
        pb.move_to(local[0].0, local[0].1);
        for p in &local[1..] {
            pb.line_to(p.0, p.1);
        }
        pb.close();
        let path = pb.finish()?;
        let mut mask = Mask::new(r.w as u32, r.h as u32)?;
        mask.fill_path(&path, FillRule::Winding, false, Transform::identity());
        Some(Selection { src: r, dest: r, mask: Some(mask), outline: vec![local], content: None, keyed: None, version: 0 })
    }

    pub fn is_lifted(&self) -> bool {
        self.content.is_some()
    }

    pub fn is_transformed(&self) -> bool {
        self.dest != self.src
    }

    /// Selection that covers everything outside this one (Paint's "Invert selection").
    pub fn inverted(&self, doc_w: u32, doc_h: u32) -> Option<Selection> {
        let full = Rect::new(0, 0, doc_w as i32, doc_h as i32);
        let mut mask = Mask::new(doc_w, doc_h)?;
        mask.data_mut().fill(255);
        let d = mask.data_mut();
        for y in 0..self.dest.h {
            for x in 0..self.dest.w {
                let (gx, gy) = (self.dest.x + x, self.dest.y + y);
                if !full.contains(gx, gy) {
                    continue;
                }
                let inside = match &self.mask {
                    None => true,
                    Some(m) => {
                        let sx = x * self.src.w / self.dest.w.max(1);
                        let sy = y * self.src.h / self.dest.h.max(1);
                        m.data()[(sy * self.src.w + sx) as usize] > 127
                    }
                };
                if inside {
                    d[(gy * doc_w as i32 + gx) as usize] = 0;
                }
            }
        }
        let sx = self.dest.w as f32 / self.src.w.max(1) as f32;
        let sy = self.dest.h as f32 / self.src.h.max(1) as f32;
        let mut outline = vec![vec![(0.0, 0.0), (doc_w as f32, 0.0), (doc_w as f32, doc_h as f32), (0.0, doc_h as f32)]];
        for poly in &self.outline {
            outline.push(poly.iter().map(|&(x, y)| (self.dest.x as f32 + x * sx, self.dest.y as f32 + y * sy)).collect());
        }
        Some(Selection { src: full, dest: full, mask: Some(mask), outline, content: None, keyed: None, version: 0 })
    }

    /// Copy the selected pixels of `p` (masked), without modifying it.
    pub fn extract(&self, p: &Pixmap) -> Pixmap {
        let mut c = raster::copy_rect(p, self.src);
        if let Some(m) = &self.mask {
            apply_mask(&mut c, m);
        }
        c
    }

    /// Lift pixels off the active layer, leaving transparency behind.
    pub fn lift(&mut self, doc: &mut Document) {
        if self.content.is_some() {
            return;
        }
        let layer = doc.active_pixels_mut();
        let content = self.extract(layer);
        clear_region(layer, self.src, self.mask.as_ref());
        self.content = Some(content);
        self.version += 1;
    }

    /// Floating content provided from outside (paste, import).
    pub fn floating(content: Pixmap, x: i32, y: i32) -> Self {
        let r = Rect::new(x, y, content.width() as i32, content.height() as i32);
        let mut s = Selection::rect(r);
        s.content = Some(content);
        s
    }

    fn transform(&self) -> Transform {
        let sx = self.dest.w as f32 / self.src.w.max(1) as f32;
        let sy = self.dest.h as f32 / self.src.h.max(1) as f32;
        Transform::from_row(sx, 0.0, 0.0, sy, self.dest.x as f32, self.dest.y as f32)
    }

    /// Refresh the transparent-selection cache; call before `overlay`/`commit`.
    pub fn prepare(&mut self, transparent: Option<Rgba>) {
        if let (Some(key), Some(content)) = (transparent, self.content.as_ref()) {
            if self.keyed.as_ref().map(|(k, _)| *k) != Some(key) {
                let mut p = content.clone();
                raster::key_out(&mut p, key);
                self.keyed = Some((key, p));
                self.version += 1;
            }
        }
    }

    fn display_content(&self, transparent: Option<Rgba>) -> Option<&Pixmap> {
        let content = self.content.as_ref()?;
        match (transparent, &self.keyed) {
            (Some(key), Some((k, p))) if *k == key => Some(p),
            _ => Some(content),
        }
    }

    pub fn invalidate_key(&mut self) {
        self.keyed = None;
        self.version += 1;
    }

    /// Pixels as currently displayed (transparent key applied when enabled and prepared).
    pub fn display_pixels(&self, transparent: Option<Rgba>) -> Option<&Pixmap> {
        self.display_content(transparent)
    }

    pub fn overlay(&self, transparent: Option<Rgba>) -> Option<Overlay<'_>> {
        let t = self.transform();
        let identity = !self.is_transformed();
        let p = self.display_content(transparent)?;
        Some(Overlay { pixmap: p, x: self.dest.x, y: self.dest.y, opacity: 1.0, erase: false, transform: if identity { None } else { Some(t) } })
    }

    /// Stamp floating content onto the active layer. Returns the affected rectangle.
    pub fn commit(&mut self, doc: &mut Document, transparent: Option<Rgba>) -> Option<Rect> {
        let t = self.transform();
        let identity = !self.is_transformed();
        let dest = self.dest;
        let src = self.src;
        self.prepare(transparent);
        let keyed = self.keyed.take().filter(|(k, _)| Some(*k) == transparent).map(|(_, p)| p);
        let content = self.content.take()?;
        let p = keyed.unwrap_or(content);
        let layer = doc.active_pixels_mut();
        if identity {
            raster::draw_over(layer, &p, dest.x, dest.y, 1.0);
        } else {
            layer.draw_pixmap(
                0,
                0,
                p.as_ref(),
                &PixmapPaint { quality: tiny_skia::FilterQuality::Nearest, ..Default::default() },
                t,
                None,
            );
        }
        self.version += 1;
        Some(dest.union(&src))
    }

    /// Outline polygons in document coordinates for the current placement.
    pub fn outline_doc(&self) -> Vec<Vec<(f32, f32)>> {
        if self.src == Rect::new(0, 0, 0, 0) {
            return vec![];
        }
        let sx = self.dest.w as f32 / self.src.w.max(1) as f32;
        let sy = self.dest.h as f32 / self.src.h.max(1) as f32;
        let (ox, oy) = if self.mask.is_some() && self.outline.len() > 1 { (0.0, 0.0) } else { (self.dest.x as f32, self.dest.y as f32) };
        self.outline
            .iter()
            .map(|poly| {
                poly.iter()
                    .map(|&(x, y)| if self.outline.len() > 1 { (x, y) } else { (ox + x * sx, oy + y * sy) })
                    .collect()
            })
            .collect()
    }

    /// Apply an operation to the floating content (rotate/flip/invert/resize within a
    /// selection). Any pending stretch is baked in first, so the result is placed 1:1 around
    /// the centre of where the selection was.
    pub fn map_content(&mut self, f: impl Fn(&Pixmap) -> Pixmap) {
        let Some(c) = self.content.take() else { return };
        let c = if (self.dest.w, self.dest.h) != (self.src.w, self.src.h) {
            raster::resize_nearest(&c, self.dest.w.max(1) as u32, self.dest.h.max(1) as u32)
        } else {
            c
        };
        let n = f(&c);
        let (w, h) = (n.width() as i32, n.height() as i32);
        // Keep the top-left corner fixed when the size is unchanged (no half-pixel drift).
        let r = Rect::new(self.dest.x + (self.dest.w - w) / 2, self.dest.y + (self.dest.h - h) / 2, w, h);
        self.src = r;
        self.dest = r;
        self.mask = None;
        self.outline = vec![vec![(0.0, 0.0), (w as f32, 0.0), (w as f32, h as f32), (0.0, h as f32)]];
        self.content = Some(n);
        self.keyed = None;
        self.version += 1;
    }
}

pub fn apply_mask(p: &mut Pixmap, m: &Mask) {
    for (px, &a) in p.pixels_mut().iter_mut().zip(m.data()) {
        if a == 0 {
            *px = tiny_skia::PremultipliedColorU8::TRANSPARENT;
        } else if a < 255 {
            let k = a as u16;
            let s = |c: u8| ((c as u16 * k) / 255) as u8;
            *px = tiny_skia::PremultipliedColorU8::from_rgba(s(px.red()), s(px.green()), s(px.blue()), s(px.alpha())).unwrap();
        }
    }
}

/// Make the (masked) region transparent.
pub fn clear_region(p: &mut Pixmap, r: Rect, mask: Option<&Mask>) {
    let w = p.width() as i32;
    let bounds = raster::full_rect(p);
    let px = p.pixels_mut();
    for y in 0..r.h {
        for x in 0..r.w {
            let (gx, gy) = (r.x + x, r.y + y);
            if !bounds.contains(gx, gy) {
                continue;
            }
            if let Some(m) = mask {
                if m.data()[(y * r.w + x) as usize] < 128 {
                    continue;
                }
            }
            px[(gy * w + gx) as usize] = tiny_skia::PremultipliedColorU8::TRANSPARENT;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lift_move_commit() {
        let mut d = Document::new(10, 10);
        d.layers[0].pixels_mut().fill(Rgba::BLACK.to_skia());
        let mut s = Selection::rect(Rect::new(0, 0, 2, 2));
        s.lift(&mut d);
        assert_eq!(raster::get_pixel(&d.layers[0].pixels, 0, 0).unwrap().a, 0);
        s.dest = Rect::new(5, 5, 2, 2);
        s.commit(&mut d, None);
        assert_eq!(raster::get_pixel(&d.layers[0].pixels, 6, 6), Some(Rgba::BLACK));
        assert_eq!(raster::get_pixel(&d.layers[0].pixels, 1, 1).unwrap().a, 0);
    }

    #[test]
    fn map_content_takes_the_result_size() {
        let mut d = Document::new(40, 40);
        let mut s = Selection::rect(Rect::new(10, 10, 10, 6));
        s.lift(&mut d);
        // Resize to 20 × 12: the selection must become 20 × 12, centred on the old one.
        s.map_content(|p| raster::resize(p, 20, 12));
        assert_eq!(s.dest, Rect::new(5, 7, 20, 12));
        assert_eq!(s.src, s.dest);
        // A quarter turn swaps the sides.
        s.map_content(|p| raster::rotate_quarter(p, 1));
        assert_eq!((s.dest.w, s.dest.h), (12, 20));
        // A stretched selection is baked before the operation.
        s.dest = Rect::new(0, 0, 24, 20);
        s.map_content(raster::flip_h);
        assert_eq!(s.dest, Rect::new(0, 0, 24, 20));
        assert_eq!(s.content.as_ref().unwrap().width(), 24);
    }

    #[test]
    fn free_form_masks_outside() {
        let mut d = Document::new(20, 20);
        d.layers[0].pixels_mut().fill(Rgba::BLACK.to_skia());
        let s = Selection::free_form(&[(0.0, 0.0), (19.0, 0.0), (0.0, 19.0)], d.bounds()).unwrap();
        let c = s.extract(&d.layers[0].pixels);
        assert_eq!(raster::get_pixel(&c, 1, 1).unwrap().a, 255);
        assert_eq!(raster::get_pixel(&c, 18, 18).unwrap().a, 0);
    }

    #[test]
    fn invert_selection_excludes_original() {
        let s = Selection::rect(Rect::new(2, 2, 3, 3));
        let inv = s.inverted(10, 10).unwrap();
        let m = inv.mask.as_ref().unwrap();
        assert_eq!(m.data()[3 * 10 + 3], 0);
        assert_eq!(m.data()[0], 255);
    }
}
