//! Linear undo/redo. Pixel edits store before/after patches of the touched rectangle;
//! structural edits (resize, rotate, layer ops) store whole-document snapshots.

use crate::doc::{DocState, Document};
use crate::raster::{self, Rect};
use std::sync::Arc;
use tiny_skia::Pixmap;

/// Windows Paint keeps 50 undo levels.
pub const MAX_STEPS: usize = 50;
const MAX_BYTES: usize = 1536 * 1024 * 1024;

enum Change {
    Patch { layer: usize, x: i32, y: i32, before: Pixmap, after: Pixmap },
    Doc { before: Box<DocState>, after: Box<DocState> },
}

struct Step {
    label: String,
    change: Change,
}

impl Step {
    fn bytes(&self) -> usize {
        match &self.change {
            Change::Patch { before, after, .. } => before.data().len() + after.data().len(),
            Change::Doc { before, after } => before.byte_size_excluding(None) + after.byte_size_excluding(Some(before)),
        }
    }
}

#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Incremented on every change; compared with `saved_at` to know if the doc is dirty.
    pub generation: u64,
    saved_at: u64,
    /// Pending "before" captured by `begin_patch`.
    pending: Option<(usize, Arc<Pixmap>)>,
    pending_doc: Option<Box<DocState>>,
    /// Separate slot for a lifted (floating) selection, which can outlive other edits.
    pending_sel: Option<(usize, Arc<Pixmap>)>,
}

impl History {
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.pending_doc = None;
        self.pending_sel = None;
        self.generation += 1;
        self.saved_at = self.generation;
    }

    pub fn is_dirty(&self) -> bool {
        self.generation != self.saved_at
    }

    pub fn mark_saved(&mut self) {
        self.saved_at = self.generation;
    }

    pub fn mark_dirty(&mut self) {
        self.generation += 1;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Snapshot the active layer before a pixel edit whose extent isn't known yet. The layer
    /// is shared, not copied: the copy happens when (and only if) the edit writes to it.
    pub fn begin_patch(&mut self, doc: &Document) {
        self.pending = Some((doc.active, doc.active_layer().pixels.clone()));
    }

    /// Finish an edit started with `begin_patch`, recording only `rect`.
    pub fn end_patch(&mut self, doc: &Document, label: &str, rect: Rect) {
        let Some((layer, full_before)) = self.pending.take() else { return };
        let rect = rect.clamp_to(doc.width, doc.height);
        if rect.is_empty() || layer >= doc.layers.len() || Arc::ptr_eq(&full_before, &doc.layers[layer].pixels) {
            return;
        }
        let before = raster::copy_rect(&full_before, rect);
        let after = raster::copy_rect(&doc.layers[layer].pixels, rect);
        if before.data() == after.data() {
            return;
        }
        self.push(Step { label: label.into(), change: Change::Patch { layer, x: rect.x, y: rect.y, before, after } });
    }

    pub fn begin_selection(&mut self, doc: &Document) {
        self.pending_sel = Some((doc.active, doc.active_layer().pixels.clone()));
    }

    pub fn end_selection(&mut self, doc: &Document, label: &str) {
        let saved = self.pending.take();
        self.pending = self.pending_sel.take();
        self.end_patch(doc, label, doc.bounds());
        self.pending = saved;
    }

    pub fn cancel_patch(&mut self) {
        self.pending = None;
    }

    pub fn begin_doc(&mut self, doc: &Document) {
        self.pending_doc = Some(Box::new(doc.snapshot()));
    }

    pub fn end_doc(&mut self, doc: &Document, label: &str) {
        if let Some(before) = self.pending_doc.take() {
            self.push(Step { label: label.into(), change: Change::Doc { before, after: Box::new(doc.snapshot()) } });
        }
    }

    /// Convenience: run a structural edit and record it.
    pub fn doc_edit(&mut self, doc: &mut Document, label: &str, f: impl FnOnce(&mut Document)) {
        self.begin_doc(doc);
        f(doc);
        self.end_doc(doc, label);
    }

    fn push(&mut self, s: Step) {
        self.redo.clear();
        self.undo.push(s);
        self.generation += 1;
        while self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
        let mut total: usize = self.undo.iter().map(Step::bytes).sum();
        while self.undo.len() > 1 && total > MAX_BYTES {
            total -= self.undo.remove(0).bytes();
        }
    }

    pub fn undo(&mut self, doc: &mut Document) -> Option<String> {
        let s = self.undo.pop()?;
        apply(doc, &s.change, true);
        let label = s.label.clone();
        self.redo.push(s);
        self.generation += 1;
        Some(label)
    }

    pub fn redo(&mut self, doc: &mut Document) -> Option<String> {
        let s = self.redo.pop()?;
        apply(doc, &s.change, false);
        let label = s.label.clone();
        self.undo.push(s);
        self.generation += 1;
        Some(label)
    }
}

fn apply(doc: &mut Document, c: &Change, undo: bool) {
    match c {
        Change::Patch { layer, x, y, before, after } => {
            if let Some(l) = doc.layers.get_mut(*layer) {
                raster::put_rect(l.pixels_mut(), if undo { before } else { after }, *x, *y);
            }
        }
        Change::Doc { before, after } => doc.restore(if undo { (**before).clone() } else { (**after).clone() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    #[test]
    fn patch_undo_redo() {
        let mut d = Document::new(10, 10);
        let mut h = History::default();
        h.begin_patch(&d);
        d.active_pixels_mut().pixels_mut()[55] = Rgba::BLACK.premul();
        h.end_patch(&d, "Pencil", Rect::new(5, 5, 1, 1));
        assert!(h.is_dirty());
        assert_eq!(h.undo(&mut d).as_deref(), Some("Pencil"));
        assert_eq!(d.layers[0].pixels.pixels()[55].alpha(), 0);
        h.redo(&mut d);
        assert_eq!(d.layers[0].pixels.pixels()[55].alpha(), 255);
    }

    #[test]
    fn doc_undo_restores_size() {
        let mut d = Document::new(10, 20);
        let mut h = History::default();
        h.doc_edit(&mut d, "Rotate", |d| d.transform_all(|p| raster::rotate_quarter(p, 1)));
        assert_eq!((d.width, d.height), (20, 10));
        h.undo(&mut d);
        assert_eq!((d.width, d.height), (10, 20));
    }

    #[test]
    fn snapshots_share_untouched_layers() {
        let mut d = Document::new(64, 64);
        d.layers.push(crate::doc::Layer::new("2", 64, 64));
        let mut h = History::default();
        h.doc_edit(&mut d, "Hide", |d| d.layers[1].visible = false);
        assert_eq!(h.undo.last().unwrap().bytes(), 2 * 64 * 64 * 4);
        // An edit that never writes records nothing and copies nothing.
        h.begin_patch(&d);
        h.end_patch(&d, "Nothing", d.bounds());
        assert_eq!(h.undo.len(), 1);
    }

    #[test]
    fn caps_at_fifty() {
        let mut d = Document::new(2, 2);
        let mut h = History::default();
        for i in 0..60 {
            h.begin_patch(&d);
            d.active_pixels_mut().pixels_mut()[0] = Rgba::rgb(i as u8, 0, 0).premul();
            h.end_patch(&d, "x", Rect::new(0, 0, 1, 1));
        }
        let mut n = 0;
        while h.undo(&mut d).is_some() {
            n += 1;
        }
        assert_eq!(n, MAX_STEPS);
    }
}
