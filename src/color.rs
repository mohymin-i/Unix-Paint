//! Straight-alpha RGBA colour used throughout the document model.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 255 }
    }

    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }

    pub fn to_egui(self) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(self.r, self.g, self.b, self.a)
    }

    pub fn from_egui(c: egui::Color32) -> Self {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        Rgba { r, g, b, a }
    }

    /// Premultiplied pixel as stored in a tiny-skia pixmap.
    pub fn premul(self) -> tiny_skia::PremultipliedColorU8 {
        let a = self.a as u16;
        let m = |c: u8| ((c as u16 * a + 127) / 255) as u8;
        tiny_skia::PremultipliedColorU8::from_rgba(m(self.r), m(self.g), m(self.b), self.a).unwrap()
    }

    pub fn from_premul(p: tiny_skia::PremultipliedColorU8) -> Self {
        let c = p.demultiply();
        Rgba { r: c.red(), g: c.green(), b: c.blue(), a: c.alpha() }
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgba::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    /// HSV in [0,360) x [0,1] x [0,1].
    pub fn to_hsv(self) -> (f32, f32, f32) {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / d).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        let s = if max == 0.0 { 0.0 } else { d / max };
        (h, s, max)
    }

    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let h = h.rem_euclid(360.0) / 60.0;
        let c = v * s;
        let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        let f = |u: f32| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgba::rgb(f(r), f(g), f(b))
    }
}

/// Default palette of the Windows Paint colour grid (two rows of ten).
pub const DEFAULT_PALETTE: [Rgba; 20] = [
    Rgba::rgb(0, 0, 0),
    Rgba::rgb(127, 127, 127),
    Rgba::rgb(136, 0, 21),
    Rgba::rgb(237, 28, 36),
    Rgba::rgb(255, 127, 39),
    Rgba::rgb(255, 242, 0),
    Rgba::rgb(34, 177, 76),
    Rgba::rgb(0, 162, 232),
    Rgba::rgb(63, 72, 204),
    Rgba::rgb(163, 73, 164),
    Rgba::rgb(255, 255, 255),
    Rgba::rgb(195, 195, 195),
    Rgba::rgb(185, 122, 87),
    Rgba::rgb(255, 174, 201),
    Rgba::rgb(255, 201, 14),
    Rgba::rgb(239, 228, 176),
    Rgba::rgb(181, 230, 29),
    Rgba::rgb(153, 217, 234),
    Rgba::rgb(112, 146, 190),
    Rgba::rgb(200, 191, 231),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_roundtrip() {
        for c in DEFAULT_PALETTE {
            let (h, s, v) = c.to_hsv();
            let d = Rgba::from_hsv(h, s, v);
            assert!((c.r as i32 - d.r as i32).abs() <= 1, "{c:?} {d:?}");
            assert!((c.g as i32 - d.g as i32).abs() <= 1);
            assert!((c.b as i32 - d.b as i32).abs() <= 1);
        }
    }

    #[test]
    fn hex_roundtrip() {
        let c = Rgba::rgb(0x12, 0xAB, 0xEF);
        assert_eq!(Rgba::parse_hex(&c.hex()), Some(c));
    }
}
