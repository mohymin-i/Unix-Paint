//! Stroke engine for pencil, brushes, eraser and textured shape media.
//!
//! A stroke rasterises a *coverage* mask (white with alpha) as the pointer moves, then a
//! per-brush "finish" turns coverage into coloured pixels inside the dirty rectangle. The
//! finished pixmap is shown as an overlay over the active layer and committed on mouse-up,
//! so overlapping within one stroke never darkens (as in Paint).

use crate::color::Rgba;
use crate::raster::{self, Rect};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tiny_skia::{
    BlendMode, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PremultipliedColorU8,
    Stroke as SkStroke, Transform,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BrushKind {
    Pencil,
    Brush,
    Calligraphy45,
    Calligraphy135,
    Airbrush,
    Oil,
    Crayon,
    Marker,
    NaturalPencil,
    Watercolor,
    Eraser,
}

impl BrushKind {
    pub const BRUSHES: [BrushKind; 9] = [
        BrushKind::Brush,
        BrushKind::Calligraphy45,
        BrushKind::Calligraphy135,
        BrushKind::Airbrush,
        BrushKind::Oil,
        BrushKind::Crayon,
        BrushKind::Marker,
        BrushKind::NaturalPencil,
        BrushKind::Watercolor,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BrushKind::Pencil => "Pencil",
            BrushKind::Brush => "Brush",
            BrushKind::Calligraphy45 => "Calligraphy brush",
            BrushKind::Calligraphy135 => "Calligraphy pen",
            BrushKind::Airbrush => "Airbrush",
            BrushKind::Oil => "Oil brush",
            BrushKind::Crayon => "Crayon",
            BrushKind::Marker => "Marker",
            BrushKind::NaturalPencil => "Natural pencil",
            BrushKind::Watercolor => "Watercolor brush",
            BrushKind::Eraser => "Eraser",
        }
    }

    /// Size range, matching the ranges recovered from Paint's attribute store.
    pub fn size_range(self) -> (f32, f32) {
        match self {
            BrushKind::Pencil => (1.0, 50.0),
            BrushKind::Oil | BrushKind::Crayon | BrushKind::Watercolor | BrushKind::Marker | BrushKind::NaturalPencil => {
                (4.0, 248.0)
            }
            BrushKind::Airbrush | BrushKind::Eraser => (4.0, 248.0),
            _ => (1.0, 248.0),
        }
    }

    pub fn default_size(self) -> f32 {
        match self {
            BrushKind::Pencil => 1.0,
            BrushKind::Brush => 5.0,
            BrushKind::Calligraphy45 | BrushKind::Calligraphy135 => 8.0,
            BrushKind::Airbrush => 30.0,
            BrushKind::Eraser => 10.0,
            BrushKind::NaturalPencil => 8.0,
            _ => 30.0,
        }
    }

    pub fn default_opacity(self) -> f32 {
        1.0
    }

}

/// Outline / fill "media" for shapes (Paint's ToolMedia enum).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Media {
    None,
    Solid,
    Crayon,
    Marker,
    Oil,
    NaturalPencil,
    Watercolor,
}

impl Media {
    pub const ALL: [Media; 7] =
        [Media::None, Media::Solid, Media::Crayon, Media::Marker, Media::Oil, Media::NaturalPencil, Media::Watercolor];

    pub fn name(self) -> &'static str {
        match self {
            Media::None => "No outline",
            Media::Solid => "Solid color",
            Media::Crayon => "Crayon",
            Media::Marker => "Marker",
            Media::Oil => "Oil",
            Media::NaturalPencil => "Natural pencil",
            Media::Watercolor => "Watercolor",
        }
    }

    pub fn brush(self) -> BrushKind {
        match self {
            Media::None | Media::Solid => BrushKind::Brush,
            Media::Crayon => BrushKind::Crayon,
            Media::Marker => BrushKind::Marker,
            Media::Oil => BrushKind::Oil,
            Media::NaturalPencil => BrushKind::NaturalPencil,
            Media::Watercolor => BrushKind::Watercolor,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Procedural textures (tileable value noise), generated once.

const TEX: usize = 256;

struct Textures {
    coarse: Vec<f32>,
    fine: Vec<f32>,
    streak: Vec<f32>,
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

fn value_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let g = |xx: i32, yy: i32| hash(xx.rem_euclid(period), yy.rem_euclid(period), seed);
    let a = g(x0, y0) + (g(x0 + 1, y0) - g(x0, y0)) * s(fx);
    let b = g(x0, y0 + 1) + (g(x0 + 1, y0 + 1) - g(x0, y0 + 1)) * s(fx);
    a + (b - a) * s(fy)
}

fn fbm(x: f32, y: f32, base: i32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let mut freq = 1;
    for o in 0..octaves {
        let p = base * freq;
        let scale = p as f32 / TEX as f32;
        sum += amp * value_noise(x * scale, y * scale, p, seed + o);
        norm += amp;
        amp *= 0.5;
        freq *= 2;
    }
    sum / norm
}

fn textures() -> &'static Textures {
    static T: OnceLock<Textures> = OnceLock::new();
    T.get_or_init(|| {
        let mut coarse = vec![0.0; TEX * TEX];
        let mut fine = vec![0.0; TEX * TEX];
        let mut streak = vec![0.0; TEX * TEX];
        for y in 0..TEX {
            for x in 0..TEX {
                let (fx, fy) = (x as f32, y as f32);
                coarse[y * TEX + x] = fbm(fx, fy, 32, 4, 7);
                fine[y * TEX + x] = fbm(fx, fy, 128, 2, 11);
                // anisotropic: long in x, short in y
                streak[y * TEX + x] = fbm(fx * 0.125, fy * 2.0, 64, 3, 23);
            }
        }
        Textures { coarse, fine, streak }
    })
}

#[inline]
fn tex(t: &[f32], x: i32, y: i32) -> f32 {
    t[(y.rem_euclid(TEX as i32) as usize) * TEX + x.rem_euclid(TEX as i32) as usize]
}

/// Tiny deterministic PRNG for airbrush and bristles.
#[derive(Clone)]
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

// ---------------------------------------------------------------------------------------------

pub struct Stroke {
    pub kind: BrushKind,
    pub color: Rgba,
    pub size: f32,
    pub opacity: f32,
    cov: Pixmap,
    pub out: Pixmap,
    /// Union of every rect touched, for the undo patch.
    pub bbox: Rect,
    last: Option<(f32, f32)>,
    rng: Rng,
    /// Oil bristle intensities across the nib.
    bristles: Vec<f32>,
    /// Pending dirty rectangle to refinish.
    dirty: Rect,
}

impl Stroke {
    pub fn new(kind: BrushKind, color: Rgba, size: f32, opacity: f32, w: u32, h: u32) -> Self {
        let mut rng = Rng::new(0x9E3779B97F4A7C15 ^ (size.to_bits() as u64) << 7);
        let n = ((size / 1.5).ceil() as usize).clamp(4, 96);
        let bristles = (0..n).map(|_| 0.35 + 0.65 * rng.next_f32()).collect();
        Stroke {
            kind,
            color,
            size,
            opacity,
            cov: raster::new_pixmap(w, h),
            out: raster::new_pixmap(w, h),
            bbox: Rect::new(0, 0, 0, 0),
            last: None,
            rng,
            bristles,
            dirty: Rect::new(0, 0, 0, 0),
        }
    }

    pub fn is_eraser(&self) -> bool {
        self.kind == BrushKind::Eraser
    }

    fn aliased(&self) -> bool {
        matches!(self.kind, BrushKind::Pencil | BrushKind::Eraser | BrushKind::Airbrush)
    }

    fn mark(&mut self, r: Rect) {
        let margin = match self.kind {
            BrushKind::Watercolor => (self.size * 0.25).ceil() as i32 + 2,
            _ => 2,
        };
        let r = r.inflate(margin).clamp_to(self.cov.width(), self.cov.height());
        self.dirty = self.dirty.union(&r);
        self.bbox = self.bbox.union(&r);
    }

    fn cov_paint(&self, alpha: f32) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color_rgba8(255, 255, 255, (alpha * 255.0).round().clamp(0.0, 255.0) as u8);
        p.anti_alias = !self.aliased();
        if self.kind == BrushKind::Oil {
            p.blend_mode = BlendMode::Lighten;
        }
        p
    }

    /// Add a pointer position; returns true if anything was drawn.
    pub fn add_point(&mut self, x: f32, y: f32) -> bool {
        let p0 = self.last.unwrap_or((x, y));
        self.last = Some((x, y));
        self.segment(p0, (x, y));
        true
    }

    /// Stroke a whole polyline (used for shape outlines).
    pub fn add_polyline(&mut self, pts: &[(f32, f32)], closed: bool) {
        if pts.is_empty() {
            return;
        }
        if self.kind == BrushKind::Brush {
            // Solid outlines: one path so joins are clean.
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].0, pts[0].1);
            for p in &pts[1..] {
                pb.line_to(p.0, p.1);
            }
            if closed {
                pb.close();
            }
            if let Some(path) = pb.finish() {
                self.stroke_path(&path, LineCap::Round, LineJoin::Round);
            }
            return;
        }
        self.last = None;
        for &p in pts {
            self.add_point(p.0, p.1);
        }
        if closed {
            self.add_point(pts[0].0, pts[0].1);
        }
    }

    /// Fill a path with this brush's texture (shape fills).
    pub fn fill_path(&mut self, path: &Path) {
        let mut paint = self.cov_paint(1.0);
        paint.blend_mode = BlendMode::SourceOver;
        self.cov.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
        let b = path.bounds();
        self.mark(Rect::new(b.left().floor() as i32, b.top().floor() as i32, b.width().ceil() as i32 + 2, b.height().ceil() as i32 + 2));
    }

    pub fn stroke_path(&mut self, path: &Path, cap: LineCap, join: LineJoin) {
        let paint = self.cov_paint(1.0);
        let st = SkStroke { width: self.size.max(1.0), line_cap: cap, line_join: join, ..Default::default() };
        self.cov.stroke_path(path, &paint, &st, Transform::identity(), None);
        let b = path.bounds();
        let m = (self.size / 2.0).ceil() as i32 + 2;
        self.mark(Rect::new(b.left() as i32 - m, b.top() as i32 - m, b.width() as i32 + 2 * m + 1, b.height() as i32 + 2 * m + 1));
    }

    fn segment(&mut self, a: (f32, f32), b: (f32, f32)) {
        let s = self.size.max(1.0);
        let r = s / 2.0;
        let seg_rect = Rect::from_points(
            (a.0.min(b.0) - r - 2.0) as i32,
            (a.1.min(b.1) - r - 2.0) as i32,
            (a.0.max(b.0) + r + 3.0) as i32,
            (a.1.max(b.1) + r + 3.0) as i32,
        );
        match self.kind {
            BrushKind::Pencil => {
                if s <= 1.0 {
                    bresenham(a, b, |x, y| set_cov(&mut self.cov, x, y));
                } else {
                    self.line(a, b, s, LineCap::Round);
                }
            }
            BrushKind::Eraser => {
                let nib = square_nib(s);
                self.sweep(a, b, &nib);
            }
            BrushKind::Brush | BrushKind::Crayon | BrushKind::Watercolor | BrushKind::NaturalPencil => {
                self.line(a, b, s, LineCap::Round)
            }
            BrushKind::Marker => {
                let nib = rect_nib(s * 0.9, s, 0.0);
                self.sweep(a, b, &nib);
            }
            BrushKind::Calligraphy45 | BrushKind::Calligraphy135 => {
                let angle: f32 = if self.kind == BrushKind::Calligraphy45 { 45.0 } else { 135.0 };
                let nib = rect_nib(s, (s / 6.0).max(1.0), angle);
                self.sweep(a, b, &nib);
            }
            BrushKind::Airbrush => {
                // Spray along the segment; the timer adds more while held still.
                let len = ((b.0 - a.0).hypot(b.1 - a.1) / (r * 0.5).max(1.0)).ceil().max(1.0) as usize;
                for i in 0..len {
                    let t = i as f32 / len as f32;
                    self.spray(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, 0.5);
                }
            }
            BrushKind::Oil => self.oil(a, b),
        }
        self.mark(seg_rect);
    }

    /// Called every frame while the button is held (airbrush keeps spraying).
    pub fn tick(&mut self, dt: f32) -> bool {
        if self.kind != BrushKind::Airbrush {
            return false;
        }
        let Some((x, y)) = self.last else { return false };
        self.spray(x, y, dt * 30.0);
        let r = self.size / 2.0 + 2.0;
        self.mark(Rect::new((x - r) as i32, (y - r) as i32, (2.0 * r) as i32 + 2, (2.0 * r) as i32 + 2));
        true
    }

    fn spray(&mut self, cx: f32, cy: f32, amount: f32) {
        let r = self.size / 2.0;
        let n = ((r * r * 0.12 * amount).ceil() as usize).max(1);
        for _ in 0..n {
            let ang = self.rng.next_f32() * std::f32::consts::TAU;
            let d = r * self.rng.next_f32().sqrt();
            set_cov(&mut self.cov, (cx + d * ang.cos()) as i32, (cy + d * ang.sin()) as i32);
        }
    }

    fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, cap: LineCap) {
        let mut pb = PathBuilder::new();
        pb.move_to(a.0, a.1);
        // tiny-skia drops zero-length segments; nudge so a click leaves a dot.
        if a == b {
            pb.line_to(b.0 + 0.01, b.1);
        } else {
            pb.line_to(b.0, b.1);
        }
        let path = pb.finish().unwrap();
        let paint = self.cov_paint(1.0);
        let st = SkStroke { width, line_cap: cap, line_join: LineJoin::Round, ..Default::default() };
        self.cov.stroke_path(&path, &paint, &st, Transform::identity(), None);
    }

    /// Fill the convex hull of the nib at `a` and at `b`.
    fn sweep(&mut self, a: (f32, f32), b: (f32, f32), nib: &[(f32, f32)]) {
        let mut pts: Vec<(f32, f32)> = nib.iter().map(|p| (a.0 + p.0, a.1 + p.1)).collect();
        pts.extend(nib.iter().map(|p| (b.0 + p.0, b.1 + p.1)));
        let hull = convex_hull(pts);
        let mut pb = PathBuilder::new();
        pb.move_to(hull[0].0, hull[0].1);
        for p in &hull[1..] {
            pb.line_to(p.0, p.1);
        }
        pb.close();
        if let Some(path) = pb.finish() {
            let paint = self.cov_paint(1.0);
            self.cov.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    fn oil(&mut self, a: (f32, f32), b: (f32, f32)) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        let (nx, ny) = if len > 0.01 { (-dy / len, dx / len) } else { (0.0, 1.0) };
        let n = self.bristles.len();
        let w = self.size;
        let bw = (w / n as f32 * 1.6).max(1.0);
        for i in 0..n {
            let off = (i as f32 + 0.5) / n as f32 - 0.5;
            // Rounded nib: outer bristles are shorter / weaker.
            let edge = 1.0 - (off * 2.0).powi(4);
            let intensity = self.bristles[i] * (0.4 + 0.6 * edge);
            let o = off * w;
            let p0 = (a.0 + nx * o, a.1 + ny * o);
            let p1 = (b.0 + nx * o, b.1 + ny * o);
            let mut pb = PathBuilder::new();
            pb.move_to(p0.0, p0.1);
            pb.line_to(p1.0 + if p0 == p1 { 0.01 } else { 0.0 }, p1.1);
            let path = pb.finish().unwrap();
            let paint = self.cov_paint(intensity);
            let st = SkStroke { width: bw, line_cap: LineCap::Round, ..Default::default() };
            self.cov.stroke_path(&path, &paint, &st, Transform::identity(), None);
        }
    }

    /// Recompute coloured output for pending dirty pixels. Returns the rect refreshed.
    pub fn finish(&mut self) -> Option<Rect> {
        let r = self.dirty;
        if r.is_empty() {
            return None;
        }
        self.dirty = Rect::new(0, 0, 0, 0);
        let t = textures();
        let w = self.cov.width() as i32;
        let blurred = if self.kind == BrushKind::Watercolor {
            Some(box_blur_alpha(&self.cov, r, (self.size * 0.18).max(1.0) as i32))
        } else {
            None
        };
        let c = self.color;
        let cov = self.cov.pixels();
        let out = self.out.pixels_mut();
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let i = (y * w + x) as usize;
                let k = cov[i].alpha() as f32 / 255.0;
                if k <= 0.0 {
                    out[i] = PremultipliedColorU8::TRANSPARENT;
                    continue;
                }
                let (alpha, shade) = match self.kind {
                    BrushKind::Pencil | BrushKind::Eraser | BrushKind::Airbrush => (if k >= 0.5 { 1.0 } else { 0.0 }, 1.0),
                    BrushKind::Brush | BrushKind::Calligraphy45 | BrushKind::Calligraphy135 => (k, 1.0),
                    BrushKind::Marker => (k * 0.62, 1.0),
                    BrushKind::Crayon => {
                        let g = tex(&t.coarse, x, y) * 0.6 + tex(&t.fine, x, y) * 0.4;
                        let a = smoothstep(0.30, 0.55, g);
                        (k * a * 0.95, 1.0)
                    }
                    BrushKind::NaturalPencil => {
                        let g = tex(&t.fine, x * 2, y * 2);
                        (k * (0.25 + 0.6 * smoothstep(0.35, 0.7, g)), 1.0)
                    }
                    BrushKind::Oil => {
                        let g = tex(&t.streak, x, y);
                        (smoothstep(0.05, 0.5, k) * (0.85 + 0.15 * g), 0.72 + 0.28 * k)
                    }
                    BrushKind::Watercolor => {
                        let bl = blurred.as_ref().unwrap();
                        let bk = bl[((y - r.y) * r.w + (x - r.x)) as usize];
                        let edge = ((k - bk).max(0.0) * 3.0).min(1.0);
                        let g = tex(&t.coarse, x, y);
                        let a = (0.32 * k + 0.45 * edge) * (0.8 + 0.4 * g);
                        (a.min(1.0), 1.0 - 0.15 * edge)
                    }
                };
                let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
                let col = Rgba {
                    r: (c.r as f32 * shade) as u8,
                    g: (c.g as f32 * shade) as u8,
                    b: (c.b as f32 * shade) as u8,
                    a,
                };
                out[i] = col.premul();
            }
        }
        Some(r)
    }
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn set_cov(p: &mut Pixmap, x: i32, y: i32) {
    if x >= 0 && y >= 0 && (x as u32) < p.width() && (y as u32) < p.height() {
        let w = p.width() as usize;
        p.pixels_mut()[y as usize * w + x as usize] = PremultipliedColorU8::from_rgba(255, 255, 255, 255).unwrap();
    }
}

pub fn bresenham(a: (f32, f32), b: (f32, f32), mut plot: impl FnMut(i32, i32)) {
    let (mut x0, mut y0) = (a.0.floor() as i32, a.1.floor() as i32);
    let (x1, y1) = (b.0.floor() as i32, b.1.floor() as i32);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        plot(x0, y0);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn square_nib(s: f32) -> Vec<(f32, f32)> {
    let h = s / 2.0;
    vec![(-h, -h), (h, -h), (h, h), (-h, h)]
}

fn rect_nib(len: f32, thick: f32, angle_deg: f32) -> Vec<(f32, f32)> {
    let a = angle_deg.to_radians();
    let (c, s) = (a.cos(), -a.sin());
    let (hl, ht) = (len / 2.0, thick / 2.0);
    [(-hl, -ht), (hl, -ht), (hl, ht), (-hl, ht)]
        .iter()
        .map(|&(x, y)| (x * c - y * s, x * s + y * c))
        .collect()
}

fn convex_hull(mut pts: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: (f32, f32), a: (f32, f32), b: (f32, f32)| (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut lower: Vec<(f32, f32)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f32, f32)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Box blur of the alpha channel inside `r` (reads up to `rad` outside it).
fn box_blur_alpha(p: &Pixmap, r: Rect, rad: i32) -> Vec<f32> {
    let (w, h) = (p.width() as i32, p.height() as i32);
    let src = p.pixels();
    let ext = r.inflate(rad).clamp_to(w as u32, h as u32);
    // horizontal pass over ext rows
    let mut tmp = vec![0.0f32; (ext.w * ext.h) as usize];
    for y in 0..ext.h {
        let row = ((ext.y + y) * w) as usize;
        let mut acc = 0.0;
        let get = |x: i32| -> f32 {
            if x < 0 || x >= w { 0.0 } else { src[row + x as usize].alpha() as f32 / 255.0 }
        };
        for x in (ext.x - rad)..=(ext.x + rad) {
            acc += get(x);
        }
        for x in 0..ext.w {
            tmp[(y * ext.w + x) as usize] = acc / (2 * rad + 1) as f32;
            acc += get(ext.x + x + rad + 1) - get(ext.x + x - rad);
        }
    }
    let mut out = vec![0.0f32; (r.w * r.h).max(0) as usize];
    for x in 0..r.w {
        let cx = r.x + x - ext.x;
        for y in 0..r.h {
            let cy = r.y + y - ext.y;
            let mut acc = 0.0;
            let mut n = 0;
            for yy in (cy - rad).max(0)..=(cy + rad).min(ext.h - 1) {
                acc += tmp[(yy * ext.w + cx) as usize];
                n += 1;
            }
            out[(y * r.w + x) as usize] = acc / (2 * rad + 1).max(n) as f32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pencil_draws_single_pixels() {
        let mut s = Stroke::new(BrushKind::Pencil, Rgba::BLACK, 1.0, 1.0, 20, 20);
        s.add_point(2.0, 2.0);
        s.add_point(10.0, 2.0);
        s.finish();
        let n = s.out.pixels().iter().filter(|p| p.alpha() == 255).count();
        assert_eq!(n, 9);
    }

    #[test]
    fn every_brush_produces_ink() {
        for kind in BrushKind::BRUSHES.iter().copied().chain([BrushKind::Eraser, BrushKind::Pencil]) {
            let mut s = Stroke::new(kind, Rgba::rgb(200, 30, 30), 20.0, 1.0, 100, 100);
            s.add_point(20.0, 50.0);
            s.add_point(80.0, 50.0);
            s.tick(0.1);
            s.finish();
            let ink = s.out.pixels().iter().filter(|p| p.alpha() > 0).count();
            assert!(ink > 50, "{kind:?} produced {ink} pixels");
            assert!(!s.bbox.is_empty());
        }
    }

    #[test]
    fn hull_of_square() {
        let h = convex_hull(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.5, 0.5)]);
        assert_eq!(h.len(), 4);
    }
}
