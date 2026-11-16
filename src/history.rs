//! Undo/redo stack storing only the changed region of each edit.

use crate::raster;
use tiny_skia::Pixmap;

const BUDGET_BYTES: usize = 768 * 1024 * 1024;

enum Kind {
    Region { x: i32, y: i32, before: Pixmap, after: Pixmap },
    Whole { before: Pixmap, after: Pixmap },
}

struct Entry {
    id: u64,
    kind: Kind,
}

impl Entry {
    fn bytes(&self) -> usize {
        match &self.kind {
            Kind::Region { before, after, .. } | Kind::Whole { before, after } => {
                before.data().len() + after.data().len()
            }
        }
    }
}

#[derive(Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    next_id: u64,
    bytes: usize,
}

impl History {
    /// Record the change from `before` to `after`. Returns false if nothing changed.
    pub fn record(&mut self, before: &Pixmap, after: &Pixmap) -> bool {
        let kind = if before.width() != after.width() || before.height() != after.height() {
            Kind::Whole { before: before.clone(), after: after.clone() }
        } else {
            let Some(r) = raster::diff_bbox(before, after) else {
                return false;
            };
            Kind::Region {
                x: r.x(),
                y: r.y(),
                before: before.clone_rect(r).unwrap(),
                after: after.clone_rect(r).unwrap(),
            }
        };
        self.next_id += 1;
        let e = Entry { id: self.next_id, kind };
        self.bytes += e.bytes();
        self.undo.push(e);
        for r in self.redo.drain(..) {
            self.bytes -= r.bytes();
        }
        while self.bytes > BUDGET_BYTES && self.undo.len() > 1 {
            let old = self.undo.remove(0);
            self.bytes -= old.bytes();
        }
        true
    }

    pub fn undo(&mut self, doc: &mut Pixmap) -> bool {
        let Some(e) = self.undo.pop() else { return false };
        match &e.kind {
            Kind::Region { x, y, before, .. } => raster::copy_into(doc, before, *x, *y),
            Kind::Whole { before, .. } => *doc = before.clone(),
        }
        self.redo.push(e);
        true
    }

    pub fn redo(&mut self, doc: &mut Pixmap) -> bool {
        let Some(e) = self.redo.pop() else { return false };
        match &e.kind {
            Kind::Region { x, y, after, .. } => raster::copy_into(doc, after, *x, *y),
            Kind::Whole { after, .. } => *doc = after.clone(),
        }
        self.undo.push(e);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Identifies the current document state; compare with the saved revision for "dirty".
    pub fn revision(&self) -> u64 {
        self.undo.last().map_or(0, |e| e.id)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Color32;

    #[test]
    fn undo_redo_region() {
        let mut doc = raster::new_canvas(10, 10, Color32::WHITE);
        let before = doc.clone();
        raster::set_px(&mut doc, 3, 4, raster::premul(Color32::RED));
        let mut h = History::default();
        assert!(h.record(&before, &doc));
        let after = doc.clone();
        assert!(h.undo(&mut doc));
        assert_eq!(doc.data(), before.data());
        assert!(h.redo(&mut doc));
        assert_eq!(doc.data(), after.data());
        assert!(!h.record(&doc.clone(), &doc));
    }
}
