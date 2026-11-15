//! A shape that has been drawn but not yet committed: it stays editable (move, resize,
//! change colours / outline / fill / size) until the user clicks elsewhere, as in Paint.

use crate::brush::{BrushKind, Media, Stroke};
use crate::color::Rgba;
use crate::doc::Overlay;
use crate::raster::{self, Rect};
use crate::shapes::{self, ShapeKind};
use tiny_skia::{LineCap, LineJoin, Path, PathBuilder};

#[derive(Clone, PartialEq)]
pub struct ShapeParams {
    pub outline: Media,
    pub fill: Media,
    pub size: f32,
    pub outline_color: Rgba,
    pub fill_color: Rgba,
}

pub struct PendingShape {
    pub kind: ShapeKind,
    /// Box shapes: [corner, opposite corner]. Line: [a, b]. Curve: [a, b, c1, c2].
    /// Polygon: vertices.
    pub pts: Vec<(f32, f32)>,
    /// Curve: 0 = line drawn, 1 = first bend set, 2 = done. Polygon: 1 while adding points.
    pub stage: u8,
    /// Polygon has been closed / finished.
    pub closed: bool,
    pub params: ShapeParams,
    cache: Option<Rendered>,
}

struct Rendered {
    key: (Vec<(f32, f32)>, ShapeParams, bool, u8),
    origin: (i32, i32),
    fill: Option<Stroke>,
    outline: Option<Stroke>,
}

impl PendingShape {
    pub fn new(kind: ShapeKind, start: (f32, f32), params: ShapeParams) -> Self {
        let pts = match kind {
            ShapeKind::Curve => vec![start, start, start, start],
            _ => vec![start, start],
        };
        PendingShape { kind, pts, stage: 0, closed: false, params, cache: None }
    }

    pub fn is_degenerate(&self) -> bool {
        let (a, b) = (self.pts[0], self.pts[1]);
        match self.kind {
            ShapeKind::Polygon => self.pts.len() < 2,
            _ => (a.0 - b.0).abs() < 1.0 && (a.1 - b.1).abs() < 1.0,
        }
    }

    /// Whether the shape has an editable bounding box (vs. point handles).
    pub fn has_box(&self) -> bool {
        self.kind.is_box_shape()
    }

    pub fn path(&self) -> Option<Path> {
        match self.kind {
            ShapeKind::Line => {
                let mut pb = PathBuilder::new();
                pb.move_to(self.pts[0].0, self.pts[0].1);
                pb.line_to(self.pts[1].0 + 0.001, self.pts[1].1);
                pb.finish()
            }
            ShapeKind::Curve => {
                let (a, b, c1, c2) = (self.pts[0], self.pts[1], self.pts[2], self.pts[3]);
                let mut pb = PathBuilder::new();
                pb.move_to(a.0, a.1);
                pb.cubic_to(c1.0, c1.1, c2.0, c2.1, b.0 + 0.001, b.1);
                pb.finish()
            }
            ShapeKind::Polygon => {
                let mut pb = PathBuilder::new();
                pb.move_to(self.pts[0].0, self.pts[0].1);
                for p in &self.pts[1..] {
                    pb.line_to(p.0, p.1);
                }
                if self.closed {
                    pb.close();
                }
                pb.finish()
            }
            k => k.path(self.pts[0].0, self.pts[0].1, self.pts[1].0, self.pts[1].1),
        }
    }

    pub fn bounds(&self) -> Rect {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in &self.pts {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if let Some(p) = self.path() {
            let b = p.bounds();
            x0 = x0.min(b.left());
            y0 = y0.min(b.top());
            x1 = x1.max(b.right());
            y1 = y1.max(b.bottom());
        }
        let m = (self.params.size / 2.0).ceil() as i32 + 3;
        Rect::from_points(x0.floor() as i32 - m, y0.floor() as i32 - m, x1.ceil() as i32 + m, y1.ceil() as i32 + m)
    }

    /// Box handles rect (x0, y0, x1, y1) normalised.
    pub fn box_rect(&self) -> (f32, f32, f32, f32) {
        let (a, b) = (self.pts[0], self.pts[1]);
        (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
    }

    pub fn set_box(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.pts[0] = (x0, y0);
        self.pts[1] = (x1, y1);
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        for p in &mut self.pts {
            p.0 += dx;
            p.1 += dy;
        }
    }

    fn render(&mut self, doc_w: u32, doc_h: u32) {
        let key = (self.pts.clone(), self.params.clone(), self.closed, self.stage);
        if self.cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let b = self.bounds().clamp_to(doc_w, doc_h);
        let (ox, oy) = (b.x, b.y);
        let (w, h) = (b.w.max(1) as u32, b.h.max(1) as u32);
        let path = self.path();
        let shift = tiny_skia::Transform::from_translate(-ox as f32, -oy as f32);
        let local = path.and_then(|p| p.transform(shift));
        let mut fill = None;
        let mut outline = None;
        if let Some(path) = &local {
            let p = &self.params;
            if self.kind.is_closed() && (self.kind != ShapeKind::Polygon || self.closed) && p.fill != Media::None {
                let mut s = Stroke::new(fill_brush(p.fill), p.fill_color, p.size, 1.0, w, h);
                s.fill_path(path);
                s.finish();
                fill = Some(s);
            }
            if p.outline != Media::None {
                let mut s = Stroke::new(p.outline.brush(), p.outline_color, p.size, 1.0, w, h);
                if p.outline == Media::Solid {
                    let join = if matches!(self.kind, ShapeKind::Rectangle | ShapeKind::Polygon | ShapeKind::Triangle | ShapeKind::RightTriangle | ShapeKind::Diamond | ShapeKind::Pentagon | ShapeKind::Hexagon | ShapeKind::Lightning) {
                        LineJoin::Miter
                    } else {
                        LineJoin::Round
                    };
                    let cap = if self.kind == ShapeKind::Line || self.kind == ShapeKind::Curve { LineCap::Round } else { LineCap::Butt };
                    s.stroke_path(path, cap, join);
                } else {
                    for (poly, closed) in shapes::flatten(path, 2.0) {
                        s.add_polyline(&poly, closed);
                    }
                }
                s.finish();
                outline = Some(s);
            }
        }
        self.cache = Some(Rendered { key, origin: (ox, oy), fill, outline });
    }

    pub fn overlays(&mut self, doc_w: u32, doc_h: u32) -> Vec<Overlay<'_>> {
        self.render(doc_w, doc_h);
        let c = self.cache.as_ref().unwrap();
        let mut v = Vec::new();
        for s in [&c.fill, &c.outline].into_iter().flatten() {
            v.push(Overlay { pixmap: &s.out, x: c.origin.0, y: c.origin.1, opacity: 1.0, erase: false, transform: None });
        }
        v
    }

    /// Draw onto a layer. Returns the affected rectangle.
    pub fn commit(&mut self, layer: &mut tiny_skia::Pixmap) -> Rect {
        let (w, h) = (layer.width(), layer.height());
        self.render(w, h);
        let c = self.cache.as_ref().unwrap();
        for s in [&c.fill, &c.outline].into_iter().flatten() {
            raster::draw_over(layer, &s.out, c.origin.0, c.origin.1, 1.0);
        }
        self.bounds()
    }
}

fn fill_brush(m: Media) -> BrushKind {
    match m {
        Media::Solid | Media::None => BrushKind::Brush,
        other => other.brush(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ShapeParams {
        ShapeParams { outline: Media::Solid, fill: Media::Solid, size: 3.0, outline_color: Rgba::BLACK, fill_color: Rgba::rgb(255, 0, 0) }
    }

    #[test]
    fn rectangle_commit_fills_and_outlines() {
        let mut layer = raster::new_pixmap(50, 50);
        let mut s = PendingShape::new(ShapeKind::Rectangle, (10.0, 10.0), params());
        s.pts[1] = (40.0, 40.0);
        s.commit(&mut layer);
        assert_eq!(raster::get_pixel(&layer, 25, 25), Some(Rgba::rgb(255, 0, 0)));
        assert_eq!(raster::get_pixel(&layer, 10, 25), Some(Rgba::BLACK));
        assert_eq!(raster::get_pixel(&layer, 2, 2).unwrap().a, 0);
    }

    #[test]
    fn every_shape_and_media_renders() {
        for k in ShapeKind::ALL {
            for m in Media::ALL {
                let mut p = params();
                p.outline = if m == Media::None { Media::Solid } else { m };
                p.fill = m;
                let mut s = PendingShape::new(k, (5.0, 5.0), p);
                match k {
                    ShapeKind::Curve => s.pts = vec![(5.0, 5.0), (60.0, 60.0), (5.0, 60.0), (60.0, 5.0)],
                    ShapeKind::Polygon => {
                        s.pts = vec![(5.0, 5.0), (60.0, 5.0), (30.0, 60.0)];
                        s.closed = true;
                    }
                    _ => s.pts[1] = (60.0, 60.0),
                }
                let mut layer = raster::new_pixmap(70, 70);
                s.commit(&mut layer);
                assert!(layer.pixels().iter().filter(|p| p.alpha() > 0).count() > 30, "{k:?} {m:?}");
            }
        }
    }
}
