//! Auto-shape geometry (Paint's `ShapeFactory`). Every shape is defined in a unit box and
//! mapped onto the dragged rectangle.

use serde::{Deserialize, Serialize};
use tiny_skia::{Path, PathBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShapeKind {
    Line,
    Curve,
    Oval,
    Rectangle,
    RoundedRectangle,
    Polygon,
    Triangle,
    RightTriangle,
    Diamond,
    Pentagon,
    Hexagon,
    RightArrow,
    LeftArrow,
    UpArrow,
    DownArrow,
    FourPointStar,
    FivePointStar,
    SixPointStar,
    RoundedRectCallout,
    OvalCallout,
    CloudCallout,
    Heart,
    Lightning,
}

impl ShapeKind {
    /// Gallery order, as in Paint's Shapes group.
    pub const ALL: [ShapeKind; 23] = [
        ShapeKind::Line,
        ShapeKind::Curve,
        ShapeKind::Oval,
        ShapeKind::Rectangle,
        ShapeKind::RoundedRectangle,
        ShapeKind::Polygon,
        ShapeKind::Triangle,
        ShapeKind::RightTriangle,
        ShapeKind::Diamond,
        ShapeKind::Pentagon,
        ShapeKind::Hexagon,
        ShapeKind::RightArrow,
        ShapeKind::LeftArrow,
        ShapeKind::UpArrow,
        ShapeKind::DownArrow,
        ShapeKind::FourPointStar,
        ShapeKind::FivePointStar,
        ShapeKind::SixPointStar,
        ShapeKind::RoundedRectCallout,
        ShapeKind::OvalCallout,
        ShapeKind::CloudCallout,
        ShapeKind::Heart,
        ShapeKind::Lightning,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ShapeKind::Line => "Line",
            ShapeKind::Curve => "Curve",
            ShapeKind::Oval => "Oval",
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::RoundedRectangle => "Rounded rectangle",
            ShapeKind::Polygon => "Polygon",
            ShapeKind::Triangle => "Triangle",
            ShapeKind::RightTriangle => "Right triangle",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Pentagon => "Pentagon",
            ShapeKind::Hexagon => "Hexagon",
            ShapeKind::RightArrow => "Right arrow",
            ShapeKind::LeftArrow => "Left arrow",
            ShapeKind::UpArrow => "Up arrow",
            ShapeKind::DownArrow => "Down arrow",
            ShapeKind::FourPointStar => "Four-point star",
            ShapeKind::FivePointStar => "Five-point star",
            ShapeKind::SixPointStar => "Six-point star",
            ShapeKind::RoundedRectCallout => "Rounded rectangular callout",
            ShapeKind::OvalCallout => "Oval callout",
            ShapeKind::CloudCallout => "Cloud callout",
            ShapeKind::Heart => "Heart",
            ShapeKind::Lightning => "Lightning",
        }
    }

    /// Shapes drawn by dragging a bounding box (everything except line/curve/polygon).
    pub fn is_box_shape(self) -> bool {
        !matches!(self, ShapeKind::Line | ShapeKind::Curve | ShapeKind::Polygon)
    }

    /// Whether the shape is closed (has an interior that can be filled).
    pub fn is_closed(self) -> bool {
        !matches!(self, ShapeKind::Line | ShapeKind::Curve)
    }

    /// Path for this box shape inside (x0,y0)-(x1,y1).
    pub fn path(self, x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Path> {
        let (l, t) = (x0.min(x1), y0.min(y1));
        let (w, h) = ((x1 - x0).abs().max(1.0), (y1 - y0).abs().max(1.0));
        let m = |u: f32, v: f32| (l + u * w, t + v * h);
        let mut pb = PathBuilder::new();
        let poly = |pb: &mut PathBuilder, pts: &[(f32, f32)]| {
            let p = m(pts[0].0, pts[0].1);
            pb.move_to(p.0, p.1);
            for q in &pts[1..] {
                let p = m(q.0, q.1);
                pb.line_to(p.0, p.1);
            }
            pb.close();
        };
        match self {
            ShapeKind::Line | ShapeKind::Curve | ShapeKind::Polygon => return None,
            ShapeKind::Rectangle => poly(&mut pb, &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            ShapeKind::Oval => {
                return PathBuilder::from_oval(tiny_skia::Rect::from_xywh(l, t, w, h)?);
            }
            ShapeKind::RoundedRectangle => {
                let r = (w.min(h) * 0.2).min(24.0f32.max(w.min(h) * 0.12));
                rounded_rect(&mut pb, l, t, w, h, r);
            }
            ShapeKind::Triangle => poly(&mut pb, &[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            ShapeKind::RightTriangle => poly(&mut pb, &[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
            ShapeKind::Diamond => poly(&mut pb, &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)]),
            ShapeKind::Pentagon => poly(&mut pb, &regular(5, 0.0)),
            ShapeKind::Hexagon => {
                poly(&mut pb, &[(0.25, 0.0), (0.75, 0.0), (1.0, 0.5), (0.75, 1.0), (0.25, 1.0), (0.0, 0.5)])
            }
            ShapeKind::RightArrow => poly(&mut pb, &ARROW),
            ShapeKind::LeftArrow => poly(&mut pb, &ARROW.map(|(x, y)| (1.0 - x, y))),
            ShapeKind::UpArrow => poly(&mut pb, &ARROW.map(|(x, y)| (y, 1.0 - x))),
            ShapeKind::DownArrow => poly(&mut pb, &ARROW.map(|(x, y)| (y, x))),
            ShapeKind::FourPointStar => poly(&mut pb, &star(4, 0.38)),
            ShapeKind::FivePointStar => poly(&mut pb, &star(5, 0.382)),
            ShapeKind::SixPointStar => poly(&mut pb, &star(6, 0.577)),
            ShapeKind::RoundedRectCallout => {
                let bh = h * 0.78;
                let r = (w.min(bh) * 0.15).min(20.0);
                // body with a tail on the bottom edge, drawn as one outline
                let (bx0, by0, bx1, by1) = (l, t, l + w, t + bh);
                pb.move_to(bx0 + r, by0);
                pb.line_to(bx1 - r, by0);
                pb.quad_to(bx1, by0, bx1, by0 + r);
                pb.line_to(bx1, by1 - r);
                pb.quad_to(bx1, by1, bx1 - r, by1);
                pb.line_to(l + w * 0.42, by1);
                pb.line_to(l + w * 0.12, t + h);
                pb.line_to(l + w * 0.22, by1);
                pb.line_to(bx0 + r, by1);
                pb.quad_to(bx0, by1, bx0, by1 - r);
                pb.line_to(bx0, by0 + r);
                pb.quad_to(bx0, by0, bx0 + r, by0);
                pb.close();
            }
            ShapeKind::OvalCallout => {
                // Ellipse in the top 78% with a wedge tail joining at the bottom-left.
                let (cx, cy, rx, ry) = (l + w / 2.0, t + h * 0.39, w / 2.0, h * 0.39);
                let a0: f32 = 100f32.to_radians();
                let a1: f32 = 125f32.to_radians();
                let pt = |a: f32| (cx + rx * a.cos(), cy + ry * a.sin());
                let start = pt(a1);
                pb.move_to(start.0, start.1);
                let steps = 48;
                for i in 1..=steps {
                    let a = a1 + (std::f32::consts::TAU - (a1 - a0)) * i as f32 / steps as f32;
                    let p = pt(a);
                    pb.line_to(p.0, p.1);
                }
                pb.line_to(l + w * 0.12, t + h);
                pb.close();
            }
            ShapeKind::CloudCallout => {
                let (cx, cy, rx, ry) = (l + w / 2.0, t + h * 0.4, w / 2.0 * 0.82, h * 0.4 * 0.82);
                let n = 10;
                let pt = |a: f32, k: f32| (cx + rx * k * a.cos(), cy + ry * k * a.sin());
                let p0 = pt(0.0, 1.0);
                pb.move_to(p0.0, p0.1);
                for i in 0..n {
                    let a0 = std::f32::consts::TAU * i as f32 / n as f32;
                    let a1 = std::f32::consts::TAU * (i + 1) as f32 / n as f32;
                    let c = pt((a0 + a1) / 2.0, 1.22);
                    let p = pt(a1, 1.0);
                    pb.quad_to(c.0, c.1, p.0, p.1);
                }
                pb.close();
                for (u, v, s) in [(0.24, 0.84, 0.07), (0.16, 0.93, 0.045), (0.1, 0.985, 0.025)] {
                    let r = s * w.min(h * 1.5);
                    if let Some(rect) = tiny_skia::Rect::from_xywh(l + u * w - r, t + v * h - r * 0.8, 2.0 * r, 1.6 * r) {
                        pb.push_oval(rect);
                    }
                }
            }
            ShapeKind::Heart => {
                let p = |u: f32, v: f32| m(u, v);
                let s = p(0.5, 0.26);
                pb.move_to(s.0, s.1);
                let c = |pb: &mut PathBuilder, a: (f32, f32), b: (f32, f32), e: (f32, f32)| {
                    let (a, b, e) = (p(a.0, a.1), p(b.0, b.1), p(e.0, e.1));
                    pb.cubic_to(a.0, a.1, b.0, b.1, e.0, e.1);
                };
                c(&mut pb, (0.5, 0.05), (0.22, -0.04), (0.06, 0.12));
                c(&mut pb, (-0.06, 0.26), (0.0, 0.5), (0.14, 0.64));
                pb.line_to(m(0.5, 1.0).0, m(0.5, 1.0).1);
                pb.line_to(m(0.86, 0.64).0, m(0.86, 0.64).1);
                c(&mut pb, (1.0, 0.5), (1.06, 0.26), (0.94, 0.12));
                c(&mut pb, (0.78, -0.04), (0.5, 0.05), (0.5, 0.26));
                pb.close();
            }
            ShapeKind::Lightning => poly(&mut pb, &LIGHTNING.map(|(x, y)| (x / 21600.0, y / 21600.0))),
        }
        pb.finish()
    }
}

const ARROW: [(f32, f32); 7] = [(0.0, 0.25), (0.6, 0.25), (0.6, 0.0), (1.0, 0.5), (0.6, 1.0), (0.6, 0.75), (0.0, 0.75)];

const LIGHTNING: [(f32, f32); 11] = [
    (8458.0, 0.0),
    (0.0, 3923.0),
    (7564.0, 8416.0),
    (4993.0, 9720.0),
    (12197.0, 13904.0),
    (9987.0, 14934.0),
    (21600.0, 21600.0),
    (14768.0, 12911.0),
    (16558.0, 12016.0),
    (11030.0, 6840.0),
    (12831.0, 6120.0),
];

/// Regular polygon with a vertex at the top, fitted to the unit box.
fn regular(n: usize, rot: f32) -> Vec<(f32, f32)> {
    let pts: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + rot + std::f32::consts::TAU * i as f32 / n as f32;
            (a.cos(), a.sin())
        })
        .collect();
    fit_unit(pts)
}

fn star(n: usize, inner: f32) -> Vec<(f32, f32)> {
    let pts: Vec<(f32, f32)> = (0..2 * n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / n as f32;
            let r = if i % 2 == 0 { 1.0 } else { inner };
            (r * a.cos(), r * a.sin())
        })
        .collect();
    fit_unit(pts)
}

fn fit_unit(pts: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in &pts {
        minx = minx.min(x);
        miny = miny.min(y);
        maxx = maxx.max(x);
        maxy = maxy.max(y);
    }
    pts.into_iter().map(|(x, y)| ((x - minx) / (maxx - minx), (y - miny) / (maxy - miny))).collect()
}

pub fn rounded_rect(pb: &mut PathBuilder, l: f32, t: f32, w: f32, h: f32, r: f32) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.5523 * r;
    let (rr, bb) = (l + w, t + h);
    pb.move_to(l + r, t);
    pb.line_to(rr - r, t);
    pb.cubic_to(rr - r + k, t, rr, t + r - k, rr, t + r);
    pb.line_to(rr, bb - r);
    pb.cubic_to(rr, bb - r + k, rr - r + k, bb, rr - r, bb);
    pb.line_to(l + r, bb);
    pb.cubic_to(l + r - k, bb, l, bb - r + k, l, bb - r);
    pb.line_to(l, t + r);
    pb.cubic_to(l, t + r - k, l + r - k, t, l + r, t);
    pb.close();
}

/// Flatten a path into polylines (one per subpath) for textured outline media.
pub fn flatten(path: &Path, tol: f32) -> Vec<(Vec<(f32, f32)>, bool)> {
    use tiny_skia::PathSegment;
    let mut out: Vec<(Vec<(f32, f32)>, bool)> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();
    let mut last = (0.0, 0.0);
    let steps_for = |len: f32| ((len / tol.max(0.5)).ceil() as usize).clamp(4, 64);
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), false));
                }
                cur.clear();
                last = (p.x, p.y);
                cur.push(last);
            }
            PathSegment::LineTo(p) => {
                last = (p.x, p.y);
                cur.push(last);
            }
            PathSegment::QuadTo(c, p) => {
                let n = steps_for((c.x - last.0).hypot(c.y - last.1) + (p.x - c.x).hypot(p.y - c.y));
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    cur.push((u * u * last.0 + 2.0 * u * t * c.x + t * t * p.x, u * u * last.1 + 2.0 * u * t * c.y + t * t * p.y));
                }
                last = (p.x, p.y);
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let n = steps_for(
                    (c1.x - last.0).hypot(c1.y - last.1) + (c2.x - c1.x).hypot(c2.y - c1.y) + (p.x - c2.x).hypot(p.y - c2.y),
                );
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    let x = u * u * u * last.0 + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p.x;
                    let y = u * u * u * last.1 + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p.y;
                    cur.push((x, y));
                }
                last = (p.x, p.y);
            }
            PathSegment::Close => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), true));
                }
            }
        }
    }
    if cur.len() > 1 {
        out.push((cur, false));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_box_shape_has_a_path_within_bounds() {
        for k in ShapeKind::ALL {
            if !k.is_box_shape() {
                assert!(k.path(0.0, 0.0, 10.0, 10.0).is_none());
                continue;
            }
            let p = k.path(10.0, 20.0, 110.0, 220.0).unwrap_or_else(|| panic!("{k:?}"));
            let b = p.bounds();
            // bounds include Bezier control points, so allow a little slack
            assert!(b.left() >= 0.0 && b.right() <= 120.0, "{k:?} {b:?}");
            assert!(b.top() >= 10.0 && b.bottom() <= 230.0, "{k:?} {b:?}");
            assert!(!flatten(&p, 1.0).is_empty());
        }
    }
}
