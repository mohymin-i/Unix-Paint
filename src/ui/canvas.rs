//! The drawing surface: zoom/scroll, rulers, grid, overlays (selection, shapes, text), and the
//! pointer state machine that drives every tool.

use crate::app::{ActiveStroke, MAX_ZOOM, MIN_ZOOM, PaintApp, Tool, ZoomRequest};
use crate::brush::{BrushKind, Stroke};
use crate::color::Rgba;
use crate::raster::{self, Rect};
use crate::selection::Selection;
use crate::shape_edit::PendingShape;
use crate::shapes::ShapeKind;
use crate::text::TextBox;
use egui::{Color32, CursorIcon, Pos2, Sense, Shape, Stroke as EStroke, Vec2, pos2, vec2};
use std::time::Instant;

const PAD: f32 = 28.0;
const HANDLE: f32 = 8.0;
const RULER: f32 = 22.0;

pub struct CanvasState {
    pub scroll: Vec2,
    pub origin: Pos2,
    pub viewport: egui::Rect,
    /// Top-left visible document pixel, for paste placement.
    pub visible_origin: (i32, i32),
    drag: Option<Drag>,
    pub resize_drag: Option<(u8, u32, u32)>,
    set_scroll: Option<Vec2>,
    button: Option<egui::PointerButton>,
    last_click: Option<(Instant, Pos2)>,
}

impl Default for CanvasState {
    fn default() -> Self {
        CanvasState {
            scroll: Vec2::ZERO,
            origin: Pos2::ZERO,
            viewport: egui::Rect::NOTHING,
            visible_origin: (0, 0),
            drag: None,
            resize_drag: None,
            set_scroll: None,
            button: None,
            last_click: None,
        }
    }
}

impl CanvasState {
    /// Forget any pointer interaction in progress (the document was replaced under it).
    pub fn cancel_interaction(&mut self) {
        self.drag = None;
        self.resize_drag = None;
        self.button = None;
    }
}

#[derive(Clone)]
enum Drag {
    Marquee { start: (f32, f32), cur: (f32, f32) },
    Lasso(Vec<(f32, f32)>),
    MoveSel { grab: (f32, f32), orig: Rect },
    ResizeSel { handle: usize, orig: Rect },
    ShapeDraw,
    ShapeMove { grab: (f32, f32), orig: Vec<(f32, f32)> },
    ShapeBox { handle: usize, orig: (f32, f32, f32, f32) },
    ShapePoint(usize),
    CurveBend,
    TextCreate { start: (f32, f32), cur: (f32, f32) },
    TextMove { grab: (f32, f32), orig: (f32, f32) },
    TextResize { grab: (f32, f32), orig: (f32, f32) },
    TextSelect,
    CanvasResize,
    Pan { grab: Pos2, orig: Vec2 },
    None,
}

impl PaintApp {
    pub fn to_doc(&self, p: Pos2) -> (f32, f32) {
        ((p.x - self.canvas.origin.x) / self.zoom, (p.y - self.canvas.origin.y) / self.zoom)
    }

    pub fn to_screen(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.canvas.origin.x + x * self.zoom, self.canvas.origin.y + y * self.zoom)
    }

    fn rect_to_screen(&self, r: Rect) -> egui::Rect {
        egui::Rect::from_min_max(self.to_screen(r.x as f32, r.y as f32), self.to_screen(r.right() as f32, r.bottom() as f32))
    }
}

/// The eight resize handles of a rectangle, clockwise from top-left.
fn handles(r: egui::Rect) -> [Pos2; 8] {
    let (l, t, rr, b, cx, cy) = (r.left(), r.top(), r.right(), r.bottom(), r.center().x, r.center().y);
    [pos2(l, t), pos2(cx, t), pos2(rr, t), pos2(rr, cy), pos2(rr, b), pos2(cx, b), pos2(l, b), pos2(l, cy)]
}

fn hit_handle(r: egui::Rect, p: Pos2) -> Option<usize> {
    handles(r).iter().position(|h| (h.x - p.x).abs() <= HANDLE && (h.y - p.y).abs() <= HANDLE)
}

fn handle_cursor(h: usize) -> CursorIcon {
    match h {
        0 | 4 => CursorIcon::ResizeNwSe,
        2 | 6 => CursorIcon::ResizeNeSw,
        1 | 5 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeHorizontal,
    }
}

/// Move the edges of (x0,y0,x1,y1) grabbed by handle `h` to doc point `p`.
fn apply_handle(h: usize, o: (f32, f32, f32, f32), p: (f32, f32)) -> (f32, f32, f32, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = o;
    if matches!(h, 0 | 6 | 7) {
        x0 = p.0;
    }
    if matches!(h, 2 | 3 | 4) {
        x1 = p.0;
    }
    if matches!(h, 0 | 1 | 2) {
        y0 = p.1;
    }
    if matches!(h, 4 | 5 | 6) {
        y1 = p.1;
    }
    (x0, y0, x1, y1)
}

/// Constrain to a square box (shapes) or 45° steps (lines) with Shift.
fn constrain(kind: ShapeKind, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    if matches!(kind, ShapeKind::Line | ShapeKind::Curve | ShapeKind::Polygon) {
        let ang = dy.atan2(dx);
        let step = std::f32::consts::FRAC_PI_4;
        let snapped = (ang / step).round() * step;
        let len = dx.hypot(dy);
        (a.0 + len * snapped.cos(), a.1 + len * snapped.sin())
    } else {
        let s = dx.abs().max(dy.abs());
        (a.0 + s * dx.signum(), a.1 + s * dy.signum())
    }
}

pub fn canvas(app: &mut PaintApp, ui: &mut egui::Ui) {
    let pal = app.palette;
    let frame = egui::Frame::new().fill(pal.workspace);
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        let full = ui.max_rect();
        let rulers = app.settings.show_rulers && !app.fullscreen;
        let view_rect = if rulers {
            egui::Rect::from_min_max(full.min + vec2(RULER, RULER), full.max)
        } else {
            full
        };
        let mut overlays_changed = false;

        // ---- zoom requests (from commands, wheel, magnifier)
        let ctrl_wheel = ui.input(|i| if i.modifiers.command { i.zoom_delta() } else { 1.0 });
        if ctrl_wheel != 1.0 {
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| view_rect.contains(*p)) {
                let z = (app.zoom * ctrl_wheel).clamp(MIN_ZOOM, MAX_ZOOM);
                app.zoom_request = Some(ZoomRequest::At(z, p));
            }
        }
        let ready = view_rect.width() > 200.0 && view_rect.height() > 150.0;
        if let Some(req) = app.zoom_request.take_if(|_| ready) {
            let (dw, dh) = (app.doc.width as f32, app.doc.height as f32);
            let fit = ((view_rect.width() - 2.0 * PAD) / dw).min((view_rect.height() - 2.0 * PAD) / dh).max(MIN_ZOOM);
            let (z, anchor) = match req {
                ZoomRequest::Fit => (fit.min(1.0), None),
                ZoomRequest::To(z) => (z, Some(view_rect.center())),
                ZoomRequest::At(z, p) => (z, Some(p)),
            };
            let z = z.clamp(MIN_ZOOM, MAX_ZOOM);
            match anchor {
                Some(p) => {
                    let doc_pt = app.to_doc(p);
                    let content = vec2(dw * z + 2.0 * PAD, dh * z + 2.0 * PAD);
                    let off = vec2(
                        ((view_rect.width() - content.x) / 2.0).max(0.0) + PAD,
                        ((view_rect.height() - content.y) / 2.0).max(0.0) + PAD,
                    );
                    let scroll = vec2(doc_pt.0 * z, doc_pt.1 * z) + off - (p - view_rect.min);
                    app.canvas.set_scroll = Some(scroll.max(Vec2::ZERO));
                }
                None => app.canvas.set_scroll = Some(Vec2::ZERO),
            }
            app.zoom = z;
        }

        let z = app.zoom;
        let doc_size = vec2(app.doc.width as f32 * z, app.doc.height as f32 * z);
        let content = vec2(
            (doc_size.x + 2.0 * PAD).max(view_rect.width()),
            (doc_size.y + 2.0 * PAD).max(view_rect.height()),
        );

        let mut sa = egui::ScrollArea::both()
            .id_salt("canvas-scroll")
            .auto_shrink([false, false])
            .scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() });
        if let Some(s) = app.canvas.set_scroll.take() {
            sa = sa.scroll_offset(s);
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(view_rect));
        let out = sa.show_viewport(&mut child, |ui, viewport| {
            let (content_rect, resp) = ui.allocate_exact_size(content, Sense::hover());
            let origin = content_rect.min
                + vec2(((content.x - doc_size.x) / 2.0).max(PAD), ((content.y - doc_size.y) / 2.0).max(PAD));
            app.canvas.origin = origin.round();
            app.canvas.viewport = egui::Rect::from_min_size(view_rect.min, view_rect.size());
            let _ = viewport;
            let tl = app.to_doc(view_rect.min);
            app.canvas.visible_origin = (tl.0.max(0.0) as i32, tl.1.max(0.0) as i32);

            overlays_changed |= handle_input(app, ui, &resp, view_rect);
            draw(app, ui, overlays_changed);
        });
        app.canvas.scroll = out.state.clamped_offset();

        if rulers {
            draw_rulers(app, ui, full, view_rect);
        }
    });
}

fn draw(app: &mut PaintApp, ui: &mut egui::Ui, overlays_changed: bool) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let pal = app.palette;
    let z = app.zoom;
    let canvas_rect = egui::Rect::from_min_size(app.canvas.origin, vec2(app.doc.width as f32 * z, app.doc.height as f32 * z));

    // Drop shadow + checkerboard for transparent areas.
    painter.add(Shape::Rect(
        egui::epaint::RectShape::filled(canvas_rect.translate(vec2(0.0, 2.0)).expand(1.0), 2.0, Color32::from_black_alpha(if pal.dark { 90 } else { 28 }))
            .with_blur_width(10.0),
    ));
    if !app.doc.background_visible || app.doc.background.a < 255 {
        let checker = app.checker_texture(&ctx);
        let uv = egui::Rect::from_min_max(Pos2::ZERO, pos2(canvas_rect.width() / 16.0, canvas_rect.height() / 16.0));
        painter.image(checker.id(), canvas_rect, uv, Color32::WHITE);
    }
    app.update_canvas(&ctx, overlays_changed);
    for (r, tex) in app.canvas_tiles() {
        painter.image(tex, app.rect_to_screen(r), egui::Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
    }
    if app.selection_drawn_separately() {
        if let Some(st) = app.selection_texture(&ctx) {
            let dest = app.selection.as_ref().unwrap().dest;
            let r = app.rect_to_screen(dest);
            painter.with_clip_rect(canvas_rect.intersect(painter.clip_rect())).image(
                st.id(),
                r,
                egui::Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    // Gridlines.
    if app.settings.show_grid {
        let step = if z >= 4.0 {
            1.0
        } else {
            *[5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0].iter().find(|s| *s * z >= 10.0).unwrap_or(&1000.0)
        };
        let vis = canvas_rect.intersect(app.canvas.viewport);
        // Grid sits on the image, so its colour must not depend on the UI theme.
        let col = Color32::from_rgba_unmultiplied(110, 110, 110, 70);
        let (x0, y0) = app.to_doc(vis.min);
        let (x1, y1) = app.to_doc(vis.max);
        let mut x = (x0 / step).ceil() * step;
        while x <= x1 {
            let sx = app.to_screen(x, 0.0).x;
            painter.line_segment([pos2(sx, vis.top()), pos2(sx, vis.bottom())], EStroke::new(1.0, col));
            x += step;
        }
        let mut y = (y0 / step).ceil() * step;
        while y <= y1 {
            let sy = app.to_screen(0.0, y).y;
            painter.line_segment([pos2(vis.left(), sy), pos2(vis.right(), sy)], EStroke::new(1.0, col));
            y += step;
        }
    }

    let t = ui.input(|i| i.time) as f32;
    let ants = |painter: &egui::Painter, pts: Vec<Pos2>, closed: bool| {
        if pts.len() < 2 {
            return;
        }
        let mut pts = pts;
        if closed {
            pts.push(pts[0]);
        }
        painter.add(Shape::line(pts.clone(), EStroke::new(1.0, Color32::WHITE)));
        let dash = Shape::dashed_line_with_offset(&pts, EStroke::new(1.0, pal.accent), &[4.0], &[4.0], (t * 12.0) % 8.0);
        painter.extend(dash);
    };
    let draw_handles = |painter: &egui::Painter, r: egui::Rect| {
        for h in handles(r) {
            let hr = egui::Rect::from_center_size(h, vec2(7.0, 7.0));
            painter.rect(hr, 2.0, Color32::WHITE, EStroke::new(1.0, pal.accent), egui::StrokeKind::Middle);
        }
    };

    // Selection outline.
    let mut animate = false;
    if let Some(sel) = &app.selection {
        for poly in sel.outline_doc() {
            let pts: Vec<Pos2> = poly.iter().map(|&(x, y)| app.to_screen(x, y)).collect();
            ants(&painter, pts, true);
        }
        let r = app.rect_to_screen(sel.dest);
        if sel.mask.is_some() {
            ants(&painter, vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true);
        }
        draw_handles(&painter, r);
        animate = true;
    }
    match &app.canvas.drag {
        Some(Drag::Marquee { start, cur }) => {
            let r = egui::Rect::from_two_pos(app.to_screen(start.0, start.1), app.to_screen(cur.0, cur.1));
            ants(&painter, vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true);
            animate = true;
        }
        Some(Drag::Lasso(pts)) => {
            ants(&painter, pts.iter().map(|&(x, y)| app.to_screen(x, y)).collect(), false);
            animate = true;
        }
        Some(Drag::TextCreate { start, cur }) => {
            let r = egui::Rect::from_two_pos(app.to_screen(start.0, start.1), app.to_screen(cur.0, cur.1));
            ants(&painter, vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true);
        }
        _ => {}
    }

    // Pending shape handles.
    if let Some(s) = &app.shape {
        if s.has_box() {
            let (x0, y0, x1, y1) = s.box_rect();
            let r = egui::Rect::from_min_max(app.to_screen(x0, y0), app.to_screen(x1, y1));
            ants(&painter, vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true);
            draw_handles(&painter, r);
            animate = true;
        } else {
            if s.kind == ShapeKind::Curve {
                let p: Vec<Pos2> = s.pts.iter().map(|&(x, y)| app.to_screen(x, y)).collect();
                if s.stage > 0 {
                    let guide = EStroke::new(1.0, pal.accent.gamma_multiply(0.6));
                    painter.line_segment([p[0], p[2]], guide);
                    painter.line_segment([p[1], p[3]], guide);
                }
            }
            let n = if s.kind == ShapeKind::Curve { if s.stage > 0 { 4 } else { 2 } } else { s.pts.len() };
            for &(x, y) in s.pts.iter().take(n) {
                let c = app.to_screen(x, y);
                painter.circle(c, 4.5, Color32::WHITE, EStroke::new(1.5, pal.accent));
            }
        }
    }

    // Text box frame and caret.
    if let Some(tb) = &app.text {
        let r = egui::Rect::from_min_size(app.to_screen(tb.x, tb.y), vec2(tb.w * z, tb.h * z));
        for (x, y, w, h) in tb.selection_rects() {
            let sr = egui::Rect::from_min_size(app.to_screen(tb.x + x, tb.y + y), vec2(w * z, h * z));
            painter.rect_filled(sr, 0.0, pal.accent.gamma_multiply(0.35));
        }
        ants(&painter, vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true);
        let rh = egui::Rect::from_center_size(r.right_bottom(), vec2(7.0, 7.0));
        painter.rect(rh, 2.0, Color32::WHITE, EStroke::new(1.0, pal.accent), egui::StrokeKind::Middle);
        if let Some((cx, cy, ch)) = tb.caret_pos() {
            if (t * 2.0) as i32 % 2 == 0 || ui.input(|i| i.time_since_last_scroll() < 0.5) {
                let a = app.to_screen(tb.x + cx, tb.y + cy);
                painter.line_segment([a, a + vec2(0.0, ch * z)], EStroke::new(1.5, app.settings.primary.to_egui().to_opaque()));
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    // Canvas resize handles.
    if app.selection.is_none() && app.shape.is_none() && app.text.is_none() {
        let (w, h) = match app.canvas.resize_drag {
            Some((_, w, h)) => (w as f32, h as f32),
            None => (app.doc.width as f32, app.doc.height as f32),
        };
        let r = egui::Rect::from_min_size(app.canvas.origin, vec2(w * z, h * z));
        if app.canvas.resize_drag.is_some() {
            ants(&painter, vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true);
        }
        for p in [pos2(r.right(), r.center().y), pos2(r.center().x, r.bottom()), r.right_bottom()] {
            let hr = egui::Rect::from_center_size(p + vec2(4.0, 4.0) * 0.0, vec2(7.0, 7.0));
            painter.rect(hr, 1.5, if pal.dark { pal.panel } else { Color32::WHITE }, EStroke::new(1.0, pal.text_dim), egui::StrokeKind::Middle);
        }
    }

    // Brush outline cursor.
    if let Some(hp) = ui.input(|i| i.pointer.hover_pos()) {
        if app.canvas.viewport.contains(hp) && app.dialog.is_none() {
            if let Some(k) = app.tool.size_kind(&app.settings) {
                let s = app.settings.size(k) * z;
                if s >= 6.0 {
                    let col = if pal.dark { Color32::WHITE } else { Color32::from_gray(40) };
                    if k == BrushKind::Eraser {
                        let r = egui::Rect::from_center_size(hp, vec2(s, s));
                        painter.rect_stroke(r, 0.0, EStroke::new(1.0, col), egui::StrokeKind::Middle);
                    } else {
                        painter.circle_stroke(hp, s / 2.0, EStroke::new(1.0, col));
                        painter.circle_stroke(hp, s / 2.0 + 1.0, EStroke::new(1.0, Color32::from_white_alpha(120)));
                    }
                }
            }
        }
    }

    if animate {
        ctx.request_repaint_after(std::time::Duration::from_millis(80));
    }
}

fn draw_rulers(app: &PaintApp, ui: &mut egui::Ui, full: egui::Rect, view: egui::Rect) {
    let pal = app.palette;
    let painter = ui.painter_at(full);
    let top = egui::Rect::from_min_max(pos2(view.left(), full.top()), pos2(full.right(), view.top()));
    let left = egui::Rect::from_min_max(pos2(full.left(), view.top()), pos2(view.left(), full.bottom()));
    let bg = pal.toolbar;
    painter.rect_filled(egui::Rect::from_min_max(full.min, view.min), 0.0, bg);
    painter.rect_filled(top, 0.0, bg);
    painter.rect_filled(left, 0.0, bg);
    painter.line_segment([top.left_bottom(), top.right_bottom()], EStroke::new(1.0, pal.border));
    painter.line_segment([left.right_top(), left.right_bottom()], EStroke::new(1.0, pal.border));
    let z = app.zoom;
    let major = *[1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0]
        .iter()
        .find(|s| *s * z >= 60.0)
        .unwrap_or(&5000.0);
    let minor = major / 10.0;
    let font = egui::FontId::proportional(9.5);
    let tick = EStroke::new(1.0, pal.text_dim);
    let (dx0, dy0) = app.to_doc(view.min);
    let (dx1, dy1) = app.to_doc(view.max);
    // Ticks are counted in whole minor steps: accumulating a fractional step drifts, and the
    // major ticks (every tenth) would then be missed.
    for i in (dx0 / minor).floor() as i64..=(dx1 / minor).ceil() as i64 {
        let x = i as f32 * minor;
        let sx = app.to_screen(x, 0.0).x;
        if sx >= view.left() {
            let is_major = i % 10 == 0;
            let len = if is_major { 10.0 } else if minor * z >= 4.0 { 4.0 } else { 0.0 };
            if len > 0.0 {
                painter.line_segment([pos2(sx, top.bottom() - len), pos2(sx, top.bottom())], tick);
            }
            if is_major {
                painter.text(pos2(sx + 2.0, top.top() + 1.0), egui::Align2::LEFT_TOP, format!("{x:.0}"), font.clone(), pal.text_dim);
            }
        }
    }
    for i in (dy0 / minor).floor() as i64..=(dy1 / minor).ceil() as i64 {
        let y = i as f32 * minor;
        let sy = app.to_screen(0.0, y).y;
        if sy >= view.top() {
            let is_major = i % 10 == 0;
            let len = if is_major { 10.0 } else if minor * z >= 4.0 { 4.0 } else { 0.0 };
            if len > 0.0 {
                painter.line_segment([pos2(left.right() - len, sy), pos2(left.right(), sy)], tick);
            }
            if is_major {
                painter.text(pos2(left.left() + 2.0, sy + 2.0), egui::Align2::LEFT_TOP, format!("{y:.0}"), font.clone(), pal.text_dim);
            }
        }
    }
    // Cursor markers.
    if let Some((cx, cy)) = app.cursor_doc {
        let s = app.to_screen(cx as f32, cy as f32);
        painter.line_segment([pos2(s.x, top.top()), pos2(s.x, top.bottom())], EStroke::new(1.0, pal.accent));
        painter.line_segment([pos2(left.left(), s.y), pos2(left.right(), s.y)], EStroke::new(1.0, pal.accent));
    }
}

// ------------------------------------------------------------------------------------------------
// Input

fn handle_input(app: &mut PaintApp, ui: &mut egui::Ui, _resp: &egui::Response, view: egui::Rect) -> bool {
    let (hover, pressed, released, down, mods, moves, double) = ui.input(|i| {
        let pressed = [egui::PointerButton::Primary, egui::PointerButton::Secondary, egui::PointerButton::Middle]
            .into_iter()
            .find(|b| i.pointer.button_pressed(*b));
        let released = [egui::PointerButton::Primary, egui::PointerButton::Secondary, egui::PointerButton::Middle]
            .into_iter()
            .find(|b| i.pointer.button_released(*b));
        let moves: Vec<Pos2> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::PointerMoved(p) => Some(*p),
                _ => None,
            })
            .collect();
        (
            i.pointer.hover_pos(),
            pressed,
            released,
            i.pointer.any_down(),
            i.modifiers,
            moves,
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
        )
    });
    let space = ui.input(|i| i.key_down(egui::Key::Space)) && app.text.is_none();
    let mut changed = false;

    if let Some(p) = hover {
        if view.contains(p) {
            let (x, y) = app.to_doc(p);
            app.cursor_doc = (x >= 0.0 && y >= 0.0 && x < app.doc.width as f32 && y < app.doc.height as f32)
                .then_some((x.floor() as i32, y.floor() as i32));
        } else if app.canvas.drag.is_none() && app.stroke.is_none() {
            app.cursor_doc = None;
        }
    }

    // ---- press
    if let (Some(b), Some(p)) = (pressed, hover) {
        if on_canvas_layer(ui, p) && view.contains(p) && app.dialog.is_none() {
            app.canvas.button = Some(b);
            if b == egui::PointerButton::Middle || space {
                app.canvas.drag = Some(Drag::Pan { grab: p, orig: app.canvas.scroll });
            } else {
                let is_double = double
                    || app.canvas.last_click.is_some_and(|(t, q)| t.elapsed().as_secs_f32() < 0.4 && q.distance(p) < 5.0);
                app.canvas.last_click = Some((Instant::now(), p));
                on_press(app, p, b, mods, is_double);
            }
            changed = true;
        }
    }

    // ---- drag
    if app.canvas.button.is_some() && down {
        let pts: Vec<Pos2> = if moves.is_empty() { hover.into_iter().collect() } else { moves };
        for p in pts {
            on_drag(app, p, mods);
        }
        if let Some(s) = app.stroke.as_mut() {
            let dt = s.last_tick.elapsed().as_secs_f32();
            s.last_tick = Instant::now();
            if s.stroke.tick(dt) {
                ui.ctx().request_repaint();
            }
        }
        changed = true;
    }
    if let Some(s) = app.stroke.as_mut() {
        if let Some(r) = s.stroke.finish() {
            app.invalidate(r);
        }
    }

    // ---- release
    if let Some(b) = released {
        if app.canvas.button == Some(b) {
            let p = hover.unwrap_or(app.canvas.origin);
            on_release(app, p, mods);
            app.canvas.button = None;
            changed = true;
        }
    } else if app.canvas.button.is_some() && !down {
        // Released outside the window.
        on_release(app, hover.unwrap_or(app.canvas.origin), mods);
        app.canvas.button = None;
        changed = true;
    }

    // ---- cursor icon
    if let Some(p) = hover {
        if view.contains(p) && on_canvas_layer(ui, p) {
            let icon = cursor_for(app, p, space);
            ui.ctx().set_cursor_icon(icon);
        }
    }
    changed || app.text.is_some() || ui.input(|i| !i.events.is_empty())
}

/// True when nothing (popup, menu, window) covers the canvas at `p`.
fn on_canvas_layer(ui: &egui::Ui, p: Pos2) -> bool {
    ui.ctx().layer_id_at(p) == Some(ui.layer_id())
}

fn cursor_for(app: &PaintApp, p: Pos2, space: bool) -> CursorIcon {
    if space || matches!(app.canvas.drag, Some(Drag::Pan { .. })) {
        return CursorIcon::Grabbing;
    }
    if let Some(Drag::MoveSel { .. } | Drag::ShapeMove { .. } | Drag::TextMove { .. }) = app.canvas.drag {
        return CursorIcon::Move;
    }
    if let Some(sel) = &app.selection {
        let r = app.rect_to_screen(sel.dest);
        if let Some(h) = hit_handle(r, p) {
            return handle_cursor(h);
        }
        if r.contains(p) {
            return CursorIcon::Move;
        }
    }
    if let Some(s) = &app.shape {
        if s.has_box() {
            let (x0, y0, x1, y1) = s.box_rect();
            let r = egui::Rect::from_min_max(app.to_screen(x0, y0), app.to_screen(x1, y1));
            if let Some(h) = hit_handle(r, p) {
                return handle_cursor(h);
            }
            if r.contains(p) {
                return CursorIcon::Move;
            }
        }
    }
    if let Some(t) = &app.text {
        let r = egui::Rect::from_min_size(app.to_screen(t.x, t.y), vec2(t.w * app.zoom, t.h * app.zoom));
        if r.right_bottom().distance(p) < HANDLE {
            return CursorIcon::ResizeNwSe;
        }
        if r.expand(4.0).contains(p) && !r.shrink(3.0).contains(p) {
            return CursorIcon::Move;
        }
        if r.contains(p) {
            return CursorIcon::Text;
        }
    }
    if let Some(h) = canvas_resize_hit(app, p) {
        return [CursorIcon::ResizeHorizontal, CursorIcon::ResizeVertical, CursorIcon::ResizeNwSe][h as usize];
    }
    match app.tool {
        Tool::Text => CursorIcon::Text,
        Tool::Magnifier => CursorIcon::ZoomIn,
        Tool::Brush | Tool::Eraser => {
            let k = app.tool.size_kind(&app.settings).unwrap();
            if app.settings.size(k) * app.zoom >= 6.0 { CursorIcon::None } else { CursorIcon::Crosshair }
        }
        Tool::Picker | Tool::Fill | Tool::Pencil => CursorIcon::Crosshair,
        _ => CursorIcon::Crosshair,
    }
}

fn canvas_resize_hit(app: &PaintApp, p: Pos2) -> Option<u8> {
    if app.selection.is_some() || app.shape.is_some() || app.text.is_some() {
        return None;
    }
    let r = egui::Rect::from_min_size(app.canvas.origin, vec2(app.doc.width as f32 * app.zoom, app.doc.height as f32 * app.zoom));
    let near = |q: Pos2| (q.x - p.x).abs() <= HANDLE && (q.y - p.y).abs() <= HANDLE;
    if near(r.right_bottom()) {
        Some(2)
    } else if near(pos2(r.right(), r.center().y)) {
        Some(0)
    } else if near(pos2(r.center().x, r.bottom())) {
        Some(1)
    } else {
        None
    }
}

fn color_for(app: &PaintApp, b: egui::PointerButton) -> Rgba {
    if b == egui::PointerButton::Secondary { app.settings.secondary } else { app.settings.primary }
}

fn on_press(app: &mut PaintApp, p: Pos2, b: egui::PointerButton, mods: egui::Modifiers, double: bool) {
    let d = app.to_doc(p);
    app.canvas.drag = Some(Drag::None);

    // Canvas resize grips.
    if b == egui::PointerButton::Primary {
        if let Some(h) = canvas_resize_hit(app, p) {
            app.canvas.resize_drag = Some((h, app.doc.width, app.doc.height));
            app.canvas.drag = Some(Drag::CanvasResize);
            return;
        }
    }

    // ---- pending shape interactions
    if let Some(s) = app.shape.as_mut() {
        if s.kind == ShapeKind::Polygon && !s.closed {
            let first = app_to_screen(app.canvas.origin, app.zoom, s.pts[0]);
            if double || first.distance(p) < HANDLE * 1.5 {
                let s = app.shape.as_mut().unwrap();
                if !double {
                    s.pts.pop();
                }
                s.closed = true;
                app.commit_shape();
                return;
            }
            let s = app.shape.as_mut().unwrap();
            s.pts.push(d);
            app.canvas.drag = Some(Drag::ShapePoint(s.pts.len() - 1));
            return;
        }
        if s.kind == ShapeKind::Curve && s.stage < 2 {
            s.stage += 1;
            s.pts[2 + (s.stage as usize - 1).min(1)] = d;
            if s.stage == 1 {
                s.pts[3] = d;
            }
            app.canvas.drag = Some(Drag::CurveBend);
            return;
        }
        if s.has_box() {
            let (x0, y0, x1, y1) = s.box_rect();
            let r = egui::Rect::from_min_max(app_to_screen(app.canvas.origin, app.zoom, (x0, y0)), app_to_screen(app.canvas.origin, app.zoom, (x1, y1)));
            if let Some(h) = hit_handle(r, p) {
                app.canvas.drag = Some(Drag::ShapeBox { handle: h, orig: (x0, y0, x1, y1) });
                return;
            }
            if r.contains(p) {
                app.canvas.drag = Some(Drag::ShapeMove { grab: d, orig: s.pts.clone() });
                return;
            }
        } else {
            let n = if s.kind == ShapeKind::Curve { 4 } else { s.pts.len() };
            for i in 0..n {
                if app_to_screen(app.canvas.origin, app.zoom, s.pts[i]).distance(p) <= HANDLE {
                    app.canvas.drag = Some(Drag::ShapePoint(i));
                    return;
                }
            }
        }
        app.commit_shape();
    }

    // ---- text box interactions
    if let Some(t) = app.text.as_mut() {
        let r = egui::Rect::from_min_size(app_to_screen(app.canvas.origin, app.zoom, (t.x, t.y)), vec2(t.w * app.zoom, t.h * app.zoom));
        if r.right_bottom().distance(p) < HANDLE {
            app.canvas.drag = Some(Drag::TextResize { grab: d, orig: (t.w, t.h) });
            return;
        }
        if r.expand(4.0).contains(p) && !r.shrink(3.0).contains(p) {
            app.canvas.drag = Some(Drag::TextMove { grab: d, orig: (t.x, t.y) });
            return;
        }
        if r.contains(p) {
            t.click(d.0 - t.x, d.1 - t.y, mods.shift);
            if double {
                t.select_all();
            }
            app.canvas.drag = Some(Drag::TextSelect);
            return;
        }
        app.commit_text();
        if app.tool == Tool::Text {
            // A click outside just commits, like Paint.
            app.canvas.drag = Some(Drag::None);
            return;
        }
    }

    // ---- selection interactions
    if let Some(sel) = &app.selection {
        let r = app.rect_to_screen(sel.dest);
        let dest = sel.dest;
        if b == egui::PointerButton::Primary {
            if let Some(h) = hit_handle(r, p) {
                app.lift_selection();
                app.canvas.drag = Some(Drag::ResizeSel { handle: h, orig: dest });
                return;
            }
            if r.contains(p) {
                // Ctrl-drag stamps a copy, as in Paint.
                if mods.command {
                    let lifted = app.selection.as_ref().unwrap().is_lifted();
                    if lifted {
                        let key = app.transparent_key();
                        let s = app.selection.as_mut().unwrap();
                        s.prepare(key);
                        if let Some(o) = s.overlay(key) {
                            let px = o.pixmap.clone();
                            let t = o.transform;
                            let layer = app.doc.active_pixels_mut();
                            match t {
                                Some(t) => layer.draw_pixmap(0, 0, px.as_ref(), &tiny_skia::PixmapPaint::default(), t, None),
                                None => raster::draw_over(layer, &px, dest.x, dest.y, 1.0),
                            }
                        }
                    } else {
                        // Copy the pixels without leaving a hole behind.
                        app.history.begin_selection(&app.doc);
                        let s = app.selection.as_mut().unwrap();
                        s.content = Some(s.extract(&app.doc.layers[app.doc.active].pixels));
                    }
                }
                app.lift_selection();
                app.canvas.drag = Some(Drag::MoveSel { grab: d, orig: dest });
                return;
            }
        }
        app.commit_selection();
    }

    // ---- tools
    let color = color_for(app, b);
    match app.tool {
        Tool::Pencil | Tool::Brush | Tool::Eraser => {
            let kind = match app.tool {
                Tool::Pencil => BrushKind::Pencil,
                Tool::Eraser => BrushKind::Eraser,
                _ => app.settings.brush,
            };
            let size = app.settings.size(kind);
            let opacity = if kind == BrushKind::Eraser { 1.0 } else { app.settings.opacity(kind) };
            let mut s = Stroke::new(kind, color, size, opacity, app.doc.width, app.doc.height);
            s.add_point(d.0, d.1);
            app.history.begin_patch(&app.doc);
            app.stroke = Some(ActiveStroke { stroke: s, last_tick: Instant::now() });
        }
        Tool::Fill => {
            let (x, y) = (d.0.floor() as i32, d.1.floor() as i32);
            app.history.begin_patch(&app.doc);
            match raster::flood_fill(app.doc.active_pixels_mut(), x, y, color, 0) {
                Some(r) => {
                    app.history.end_patch(&app.doc, "Fill", r);
                    app.invalidate(r);
                }
                None => app.history.cancel_patch(),
            }
            app.canvas.drag = Some(Drag::None);
        }
        Tool::Picker => {
            let (x, y) = (d.0.floor() as i32, d.1.floor() as i32);
            if x >= 0 && y >= 0 && x < app.doc.width as i32 && y < app.doc.height as i32 {
                let px = app.doc.composite(Rect::new(x, y, 1, 1), &[]);
                if let Some(mut c) = raster::get_pixel(&px, 0, 0) {
                    if c.a == 0 {
                        c = Rgba::WHITE;
                    }
                    c.a = 255;
                    if b == egui::PointerButton::Secondary {
                        app.settings.secondary = c;
                    } else {
                        app.settings.primary = c;
                    }
                }
                let back = app.prev_tool;
                app.set_tool(back);
            }
        }
        Tool::Magnifier => {
            if b == egui::PointerButton::Secondary {
                app.zoom_out(Some(p));
            } else {
                app.zoom_in(Some(p));
            }
        }
        Tool::Text => {
            app.canvas.drag = Some(Drag::TextCreate { start: d, cur: d });
        }
        Tool::Select => {
            app.canvas.drag = Some(if app.settings.free_form_select { Drag::Lasso(vec![d]) } else { Drag::Marquee { start: d, cur: d } });
        }
        Tool::Shape => {
            let mut params = app.shape_params();
            if b == egui::PointerButton::Secondary {
                std::mem::swap(&mut params.outline_color, &mut params.fill_color);
            }
            let kind = app.settings.shape;
            let mut s = PendingShape::new(kind, d, params);
            if kind == ShapeKind::Polygon {
                s.stage = 1;
            }
            app.shape = Some(s);
            app.canvas.drag = Some(Drag::ShapeDraw);
        }
    }
}

fn app_to_screen(origin: Pos2, zoom: f32, p: (f32, f32)) -> Pos2 {
    pos2(origin.x + p.0 * zoom, origin.y + p.1 * zoom)
}

fn on_drag(app: &mut PaintApp, p: Pos2, mods: egui::Modifiers) {
    let d = app.to_doc(p);
    if let Some(s) = app.stroke.as_mut() {
        s.stroke.add_point(d.0, d.1);
        return;
    }
    let Some(drag) = app.canvas.drag.clone() else { return };
    match drag {
        Drag::Pan { grab, orig } => {
            app.canvas.set_scroll = Some((orig - (p - grab)).max(Vec2::ZERO));
        }
        Drag::Marquee { start, .. } => {
            let cur = (d.0.clamp(0.0, app.doc.width as f32), d.1.clamp(0.0, app.doc.height as f32));
            let cur = if mods.shift { constrain(ShapeKind::Rectangle, start, cur) } else { cur };
            app.canvas.drag = Some(Drag::Marquee { start, cur });
        }
        Drag::Lasso(mut pts) => {
            let c = (d.0.clamp(0.0, app.doc.width as f32), d.1.clamp(0.0, app.doc.height as f32));
            if pts.last().is_none_or(|l| (l.0 - c.0).abs() + (l.1 - c.1).abs() >= 1.0) {
                pts.push(c);
            }
            app.canvas.drag = Some(Drag::Lasso(pts));
        }
        Drag::MoveSel { grab, orig } => {
            if let Some(s) = app.selection.as_mut() {
                s.dest.x = orig.x + (d.0 - grab.0).round() as i32;
                s.dest.y = orig.y + (d.1 - grab.1).round() as i32;
            }
        }
        Drag::ResizeSel { handle, orig } => {
            if let Some(s) = app.selection.as_mut() {
                let o = (orig.x as f32, orig.y as f32, orig.right() as f32, orig.bottom() as f32);
                let (mut x0, mut y0, mut x1, mut y1) = apply_handle(handle, o, (d.0.round(), d.1.round()));
                if mods.shift && matches!(handle, 0 | 2 | 4 | 6) {
                    let aspect = orig.w as f32 / orig.h.max(1) as f32;
                    let w = (x1 - x0).abs();
                    let h = w / aspect;
                    if matches!(handle, 0 | 2) { y0 = y1 - h } else { y1 = y0 + h }
                }
                if x1 - x0 < 1.0 {
                    if matches!(handle, 0 | 6 | 7) { x0 = x1 - 1.0 } else { x1 = x0 + 1.0 }
                }
                if y1 - y0 < 1.0 {
                    if matches!(handle, 0 | 1 | 2) { y0 = y1 - 1.0 } else { y1 = y0 + 1.0 }
                }
                s.dest = Rect::new(x0 as i32, y0 as i32, (x1 - x0) as i32, (y1 - y0) as i32);
            }
        }
        Drag::ShapeDraw => {
            if let Some(s) = app.shape.as_mut() {
                let a = s.pts[0];
                let idx = if s.kind == ShapeKind::Polygon { s.pts.len() - 1 } else { 1 };
                let c = if mods.shift { constrain(s.kind, if s.kind == ShapeKind::Polygon && idx > 0 { s.pts[idx - 1] } else { a }, d) } else { d };
                s.pts[idx] = c;
                if s.kind == ShapeKind::Curve {
                    s.pts[2] = a;
                    s.pts[3] = c;
                }
            }
        }
        Drag::ShapeMove { grab, orig } => {
            if let Some(s) = app.shape.as_mut() {
                s.pts = orig;
                s.translate(d.0 - grab.0, d.1 - grab.1);
            }
        }
        Drag::ShapeBox { handle, orig } => {
            if let Some(s) = app.shape.as_mut() {
                let (x0, y0, x1, y1) = apply_handle(handle, orig, d);
                s.set_box(x0, y0, x1, y1);
            }
        }
        Drag::ShapePoint(i) => {
            if let Some(s) = app.shape.as_mut() {
                let c = if mods.shift && i > 0 { constrain(s.kind, s.pts[i - 1], d) } else { d };
                if i < s.pts.len() {
                    s.pts[i] = c;
                }
            }
        }
        Drag::CurveBend => {
            if let Some(s) = app.shape.as_mut() {
                if s.stage == 1 {
                    s.pts[2] = d;
                    s.pts[3] = d;
                } else {
                    s.pts[3] = d;
                }
            }
        }
        Drag::TextCreate { start, .. } => app.canvas.drag = Some(Drag::TextCreate { start, cur: d }),
        Drag::TextMove { grab, orig } => {
            if let Some(t) = app.text.as_mut() {
                t.x = (orig.0 + d.0 - grab.0).round();
                t.y = (orig.1 + d.1 - grab.1).round();
            }
        }
        Drag::TextResize { grab, orig } => {
            if let Some(t) = app.text.as_mut() {
                t.w = (orig.0 + d.0 - grab.0).max(10.0);
                t.h = (orig.1 + d.1 - grab.1).max(10.0);
            }
        }
        Drag::TextSelect => {
            if let Some(t) = app.text.as_mut() {
                t.click(d.0 - t.x, d.1 - t.y, true);
            }
        }
        Drag::CanvasResize => {
            if let Some((h, _, _)) = app.canvas.resize_drag {
                let w = if h != 1 { d.0.round().clamp(1.0, crate::doc::MAX_DIM as f32) as u32 } else { app.doc.width };
                let hh = if h != 0 { d.1.round().clamp(1.0, crate::doc::MAX_DIM as f32) as u32 } else { app.doc.height };
                app.canvas.resize_drag = Some((h, w, hh));
            }
        }
        Drag::None => {}
    }
}

fn on_release(app: &mut PaintApp, p: Pos2, _mods: egui::Modifiers) {
    let d = app.to_doc(p);
    if let Some(mut s) = app.stroke.take() {
        s.stroke.finish();
        let r = s.stroke.bbox;
        let paint = tiny_skia::PixmapPaint {
            opacity: s.stroke.opacity,
            blend_mode: if s.stroke.is_eraser() { tiny_skia::BlendMode::DestinationOut } else { tiny_skia::BlendMode::SourceOver },
            ..Default::default()
        };
        if !r.is_empty() {
            // Only the touched rectangle is blended, not the whole canvas.
            let ink = raster::copy_rect(&s.stroke.out, r);
            app.doc.active_pixels_mut().draw_pixmap(r.x, r.y, ink.as_ref(), &paint, tiny_skia::Transform::identity(), None);
        }
        let label = app.tool.name(&app.settings);
        app.history.end_patch(&app.doc, &label, r);
        app.invalidate(r);
        app.canvas.drag = None;
        return;
    }
    let drag = app.canvas.drag.take();
    match drag {
        Some(Drag::Marquee { start, cur }) => {
            let r = Rect::from_points(start.0.round() as i32, start.1.round() as i32, cur.0.round() as i32, cur.1.round() as i32)
                .clamp_to(app.doc.width, app.doc.height);
            if r.w >= 1 && r.h >= 1 {
                app.selection = Some(Selection::rect(r));
            }
        }
        Some(Drag::Lasso(pts)) => {
            app.selection = Selection::free_form(&pts, app.doc.bounds());
        }
        Some(Drag::ShapeDraw) => {
            if let Some(s) = app.shape.as_ref() {
                if s.kind != ShapeKind::Polygon && s.is_degenerate() {
                    app.shape = None;
                } else if s.kind == ShapeKind::Polygon {
                    let s = app.shape.as_mut().unwrap();
                    let last = s.pts[s.pts.len() - 1];
                    let first = s.pts[0];
                    if (last.0 - first.0).abs() < 1.0 && (last.1 - first.1).abs() < 1.0 {
                        app.shape = None;
                    } else {
                        s.pts.push(last);
                    }
                }
            }
        }
        Some(Drag::ShapePoint(_)) => {
            if let Some(s) = app.shape.as_mut() {
                if s.kind == ShapeKind::Polygon && !s.closed {
                    let last = *s.pts.last().unwrap();
                    s.pts.push(last);
                }
            }
        }
        Some(Drag::CurveBend) => {
            if app.shape.as_ref().is_some_and(|s| s.stage >= 2) {
                app.commit_shape();
            }
        }
        Some(Drag::TextCreate { start, cur }) => {
            let (x0, y0) = (start.0.min(cur.0).round(), start.1.min(cur.1).round());
            let (w, h) = ((cur.0 - start.0).abs(), (cur.1 - start.1).abs());
            let px = crate::text::pt_to_px(app.settings.text.size_pt, app.doc.dpi);
            let w = if w < 8.0 { (px * 14.0).max(120.0) } else { w };
            let h = if h < 8.0 { px * 1.4 } else { h };
            let x0 = x0.clamp(0.0, (app.doc.width as f32 - 4.0).max(0.0));
            let y0 = y0.clamp(0.0, (app.doc.height as f32 - 4.0).max(0.0));
            let mut t = TextBox::new(x0, y0, w, h);
            t.layout(&mut app.text_engine, &app.settings.text, app.doc.dpi);
            app.text = Some(t);
        }
        Some(Drag::CanvasResize) => {
            if let Some((_, w, h)) = app.canvas.resize_drag.take() {
                if (w, h) != (app.doc.width, app.doc.height) {
                    app.history.doc_edit(&mut app.doc, "Resize", |doc| doc.resize_canvas(w, h));
                    app.invalidate_all();
                }
            }
        }
        _ => {}
    }
    let _ = d;
}
