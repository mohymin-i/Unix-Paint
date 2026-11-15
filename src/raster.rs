//! Pixel-level helpers on premultiplied tiny-skia pixmaps.

use crate::color::Rgba;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, PremultipliedColorU8, Transform};

pub fn new_pixmap(w: u32, h: u32) -> Pixmap {
    Pixmap::new(w.max(1), h.max(1)).expect("pixmap allocation")
}

#[cfg(test)]
pub fn filled(w: u32, h: u32, c: Rgba) -> Pixmap {
    let mut p = new_pixmap(w, h);
    p.fill(c.to_skia());
    p
}

/// Integer rectangle helper (x, y, w, h) clamped to a size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }
    pub fn from_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect::new(x, y, (r - x).max(0), (b - y).max(0))
    }
    pub fn inflate(&self, d: i32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2 * d, self.h + 2 * d)
    }
    pub fn clamp_to(&self, w: u32, h: u32) -> Rect {
        self.intersect(&Rect::new(0, 0, w as i32, h as i32))
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

pub fn full_rect(p: &Pixmap) -> Rect {
    Rect::new(0, 0, p.width() as i32, p.height() as i32)
}

/// Copy a region out of a pixmap. Pixels outside the source are transparent.
pub fn copy_rect(src: &Pixmap, r: Rect) -> Pixmap {
    let mut out = new_pixmap(r.w.max(1) as u32, r.h.max(1) as u32);
    out.draw_pixmap(
        -r.x,
        -r.y,
        src.as_ref(),
        &PixmapPaint { blend_mode: tiny_skia::BlendMode::Source, ..Default::default() },
        Transform::identity(),
        None,
    );
    out
}

/// Overwrite a region of `dst` with `src` (no blending) at (x, y).
pub fn put_rect(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32) {
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let sw = src.width() as i32;
    let r = Rect::new(x, y, sw, src.height() as i32).clamp_to(dw as u32, dh as u32);
    if r.is_empty() {
        return;
    }
    let s = src.pixels();
    let d = dst.pixels_mut();
    for yy in r.y..r.bottom() {
        let so = ((yy - y) * sw + (r.x - x)) as usize;
        let do_ = (yy * dw + r.x) as usize;
        d[do_..do_ + r.w as usize].copy_from_slice(&s[so..so + r.w as usize]);
    }
}

pub fn draw_over(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32, opacity: f32) {
    dst.draw_pixmap(
        x,
        y,
        src.as_ref(),
        &PixmapPaint { opacity, ..Default::default() },
        Transform::identity(),
        None,
    );
}

pub fn get_pixel(p: &Pixmap, x: i32, y: i32) -> Option<Rgba> {
    if x < 0 || y < 0 || x >= p.width() as i32 || y >= p.height() as i32 {
        return None;
    }
    p.pixel(x as u32, y as u32).map(Rgba::from_premul)
}

/// Scan-line flood fill of the contiguous region matching the seed pixel.
/// `tolerance` is the max per-channel difference (0 = exact). Returns the bounding box changed.
pub fn flood_fill(p: &mut Pixmap, sx: i32, sy: i32, color: Rgba, tolerance: u8) -> Option<Rect> {
    let w = p.width() as i32;
    let h = p.height() as i32;
    if sx < 0 || sy < 0 || sx >= w || sy >= h {
        return None;
    }
    let fill = color.premul();
    let px = p.pixels_mut();
    let target = px[(sy * w + sx) as usize];
    if target == fill {
        return None;
    }
    let t = tolerance as i32;
    let matches = |c: PremultipliedColorU8| -> bool {
        if t == 0 {
            return c == target;
        }
        (c.red() as i32 - target.red() as i32).abs() <= t
            && (c.green() as i32 - target.green() as i32).abs() <= t
            && (c.blue() as i32 - target.blue() as i32).abs() <= t
            && (c.alpha() as i32 - target.alpha() as i32).abs() <= t
    };
    let mut visited = vec![false; (w * h) as usize];
    let mut stack = vec![(sx, sy)];
    let (mut minx, mut miny, mut maxx, mut maxy) = (sx, sy, sx, sy);
    while let Some((x, y)) = stack.pop() {
        let row = (y * w) as usize;
        if visited[row + x as usize] || !matches(px[row + x as usize]) {
            continue;
        }
        let mut l = x;
        while l > 0 && !visited[row + (l - 1) as usize] && matches(px[row + (l - 1) as usize]) {
            l -= 1;
        }
        let mut r = x;
        while r < w - 1 && !visited[row + (r + 1) as usize] && matches(px[row + (r + 1) as usize]) {
            r += 1;
        }
        for i in l..=r {
            px[row + i as usize] = fill;
            visited[row + i as usize] = true;
        }
        minx = minx.min(l);
        maxx = maxx.max(r);
        miny = miny.min(y);
        maxy = maxy.max(y);
        for ny in [y - 1, y + 1] {
            if ny < 0 || ny >= h {
                continue;
            }
            let nrow = (ny * w) as usize;
            let mut i = l;
            while i <= r {
                if !visited[nrow + i as usize] && matches(px[nrow + i as usize]) {
                    stack.push((i, ny));
                    // skip the rest of this run
                    while i <= r && matches(px[nrow + i as usize]) {
                        i += 1;
                    }
                }
                i += 1;
            }
        }
    }
    Some(Rect::new(minx, miny, maxx - minx + 1, maxy - miny + 1))
}

pub fn flip_h(p: &Pixmap) -> Pixmap {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let mut out = new_pixmap(w as u32, h as u32);
    let s = p.pixels();
    let d = out.pixels_mut();
    for y in 0..h {
        for x in 0..w {
            d[y * w + x] = s[y * w + (w - 1 - x)];
        }
    }
    out
}

pub fn flip_v(p: &Pixmap) -> Pixmap {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let mut out = new_pixmap(w as u32, h as u32);
    let s = p.pixels();
    let d = out.pixels_mut();
    for y in 0..h {
        d[y * w..(y + 1) * w].copy_from_slice(&s[(h - 1 - y) * w..(h - y) * w]);
    }
    out
}

/// Rotate by quarter turns clockwise (1, 2 or 3).
pub fn rotate_quarter(p: &Pixmap, quarters: u32) -> Pixmap {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let q = quarters % 4;
    if q == 0 {
        return p.clone();
    }
    let (nw, nh) = if q == 2 { (w, h) } else { (h, w) };
    let mut out = new_pixmap(nw as u32, nh as u32);
    let s = p.pixels();
    let d = out.pixels_mut();
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match q {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            d[ny * nw + nx] = s[y * w + x];
        }
    }
    out
}

/// Rotate by an arbitrary angle (degrees, clockwise), expanding the canvas; new area transparent.
pub fn rotate_any(p: &Pixmap, degrees: f32) -> Pixmap {
    let d = degrees.rem_euclid(360.0);
    for (a, q) in [(0.0, 0), (90.0, 1), (180.0, 2), (270.0, 3)] {
        if (d - a).abs() < 1e-4 {
            return rotate_quarter(p, q);
        }
    }
    let (w, h) = (p.width() as f32, p.height() as f32);
    let r = d.to_radians();
    let (s, c) = (r.sin().abs(), r.cos().abs());
    let nw = (w * c + h * s).ceil();
    let nh = (w * s + h * c).ceil();
    let mut out = new_pixmap(nw as u32, nh as u32);
    let t = Transform::from_translate(nw / 2.0, nh / 2.0)
        .pre_rotate(d)
        .pre_translate(-w / 2.0, -h / 2.0);
    out.draw_pixmap(
        0,
        0,
        p.as_ref(),
        &PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() },
        t,
        None,
    );
    out
}

pub fn resize(p: &Pixmap, nw: u32, nh: u32) -> Pixmap {
    let nw = nw.max(1);
    let nh = nh.max(1);
    if nw == p.width() && nh == p.height() {
        return p.clone();
    }
    // Area-average when shrinking a lot gives far better results than bicubic sampling.
    // Filtering runs on the premultiplied pixels so transparent areas don't bleed their
    // (meaningless) colour into the edges.
    if nw * 2 <= p.width() || nh * 2 <= p.height() {
        let img = image::RgbaImage::from_raw(p.width(), p.height(), p.data().to_vec()).expect("size");
        let r = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
        let mut out = new_pixmap(nw, nh);
        for (d, s) in out.pixels_mut().iter_mut().zip(r.pixels()) {
            let a = s[3];
            *d = PremultipliedColorU8::from_rgba(s[0].min(a), s[1].min(a), s[2].min(a), a).unwrap();
        }
        return out;
    }
    let mut out = new_pixmap(nw, nh);
    let sx = nw as f32 / p.width() as f32;
    let sy = nh as f32 / p.height() as f32;
    out.draw_pixmap(
        0,
        0,
        p.as_ref(),
        &PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() },
        Transform::from_scale(sx, sy),
        None,
    );
    out
}

/// Nearest-neighbour resize (used for scaling selections while dragging, like Paint does).
pub fn resize_nearest(p: &Pixmap, nw: u32, nh: u32) -> Pixmap {
    let nw = nw.max(1);
    let nh = nh.max(1);
    let (w, h) = (p.width(), p.height());
    let mut out = new_pixmap(nw, nh);
    let s = p.pixels();
    let d = out.pixels_mut();
    for y in 0..nh {
        let sy = ((y as u64 * h as u64) / nh as u64) as u32;
        for x in 0..nw {
            let sx = ((x as u64 * w as u64) / nw as u64) as u32;
            d[(y * nw + x) as usize] = s[(sy * w + sx) as usize];
        }
    }
    out
}

/// Skew in degrees; horizontal shifts rows, vertical shifts columns. Canvas grows to fit.
/// Size of the image `skew` produces for a `w` × `h` input.
pub fn skewed_size(w: u32, h: u32, h_deg: f32, v_deg: f32) -> (f32, f32) {
    let (w, h) = (w as f32, h as f32);
    let kx = h_deg.to_radians().tan();
    let ky = v_deg.to_radians().tan();
    ((w + (h * kx).abs()).ceil().max(1.0), (h + (w * ky).abs()).ceil().max(1.0))
}

pub fn skew(p: &Pixmap, h_deg: f32, v_deg: f32) -> Pixmap {
    let (w, h) = (p.width() as f32, p.height() as f32);
    let kx = h_deg.to_radians().tan();
    let ky = v_deg.to_radians().tan();
    let (nw, nh) = skewed_size(p.width(), p.height(), h_deg, v_deg);
    let ox = if kx < 0.0 { -h * kx } else { 0.0 };
    let oy = if ky < 0.0 { -w * ky } else { 0.0 };
    let mut out = new_pixmap(nw as u32, nh as u32);
    let t = Transform::from_row(1.0, ky, kx, 1.0, ox, oy);
    out.draw_pixmap(
        0,
        0,
        p.as_ref(),
        &PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() },
        t,
        None,
    );
    out
}

pub fn invert(p: &mut Pixmap) {
    for px in p.pixels_mut() {
        let a = px.alpha();
        *px = PremultipliedColorU8::from_rgba(a - px.red(), a - px.green(), a - px.blue(), a).unwrap();
    }
}

/// Convert to pure black & white (Paint's monochrome mode).
pub fn to_monochrome(p: &mut Pixmap) {
    for px in p.pixels_mut() {
        let c = px.demultiply();
        if c.alpha() == 0 {
            continue;
        }
        let l = 0.299 * c.red() as f32 + 0.587 * c.green() as f32 + 0.114 * c.blue() as f32;
        let v = if l >= 128.0 { 255 } else { 0 };
        *px = Rgba::rgb(v, v, v).premul();
    }
}

pub fn to_image(p: &Pixmap) -> image::RgbaImage {
    let data = p.clone().take_demultiplied();
    image::RgbaImage::from_raw(p.width(), p.height(), data).expect("size")
}

pub fn from_image(img: &image::RgbaImage) -> Pixmap {
    let mut p = new_pixmap(img.width(), img.height());
    for (d, s) in p.pixels_mut().iter_mut().zip(img.pixels()) {
        *d = Rgba { r: s[0], g: s[1], b: s[2], a: s[3] }.premul();
    }
    p
}

pub fn has_transparency(p: &Pixmap) -> bool {
    p.pixels().iter().any(|c| c.alpha() != 255)
}

/// Replace every pixel equal (ignoring tiny AA differences) to `key` with transparency.
pub fn key_out(p: &mut Pixmap, key: Rgba) {
    let k = key.premul();
    for px in p.pixels_mut() {
        if px.alpha() == 255 && *px == k {
            *px = PremultipliedColorU8::TRANSPARENT;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flood_fill_region() {
        let mut p = filled(10, 10, Rgba::WHITE);
        // vertical black wall at x=5
        for y in 0..10 {
            p.pixels_mut()[y * 10 + 5] = Rgba::BLACK.premul();
        }
        let r = flood_fill(&mut p, 1, 1, Rgba::rgb(255, 0, 0), 0).unwrap();
        assert_eq!(r, Rect::new(0, 0, 5, 10));
        assert_eq!(get_pixel(&p, 4, 9), Some(Rgba::rgb(255, 0, 0)));
        assert_eq!(get_pixel(&p, 6, 0), Some(Rgba::WHITE));
    }

    #[test]
    fn rotate_and_flip() {
        let mut p = filled(3, 2, Rgba::WHITE);
        p.pixels_mut()[0] = Rgba::BLACK.premul(); // top-left
        let r = rotate_quarter(&p, 1);
        assert_eq!((r.width(), r.height()), (2, 3));
        assert_eq!(get_pixel(&r, 1, 0), Some(Rgba::BLACK)); // top-right after cw
        let f = flip_h(&p);
        assert_eq!(get_pixel(&f, 2, 0), Some(Rgba::BLACK));
        let v = flip_v(&p);
        assert_eq!(get_pixel(&v, 0, 1), Some(Rgba::BLACK));
        assert_eq!(rotate_quarter(&rotate_quarter(&p, 3), 1).data(), p.data());
    }

    #[test]
    fn copy_put_roundtrip() {
        let mut p = filled(8, 8, Rgba::WHITE);
        p.pixels_mut()[3 * 8 + 3] = Rgba::BLACK.premul();
        let c = copy_rect(&p, Rect::new(2, 2, 3, 3));
        assert_eq!(get_pixel(&c, 1, 1), Some(Rgba::BLACK));
        let mut q = filled(8, 8, Rgba::WHITE);
        put_rect(&mut q, &c, 2, 2);
        assert_eq!(q.data(), p.data());
    }

    #[test]
    fn invert_twice_is_identity() {
        let mut p = filled(4, 4, Rgba::rgb(10, 200, 30));
        let orig = p.clone();
        invert(&mut p);
        assert_eq!(get_pixel(&p, 0, 0), Some(Rgba::rgb(245, 55, 225)));
        invert(&mut p);
        assert_eq!(p.data(), orig.data());
    }
}
