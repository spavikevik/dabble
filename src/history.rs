//! Undo/redo. Pixel edits store only the changed region of one layer;
//! layer add/remove/move and whole-image operations store what they need.

use crate::layers::Layer;
use crate::raster;
use tiny_skia::Pixmap;

const BUDGET_BYTES: usize = 768 * 1024 * 1024;

pub enum Change {
    /// Pixels of one layer changed inside a region.
    Pixels { layer: usize, x: i32, y: i32, before: Pixmap, after: Pixmap },
    /// A layer was inserted at `index`.
    Insert { index: usize, layer: Layer },
    /// The layer at `index` was removed.
    Remove { index: usize, layer: Layer },
    /// A layer moved from one position to another.
    Move { from: usize, to: usize },
    /// Every layer's pixels were replaced (rotate, crop, canvas resize, ...).
    Replace { before: Vec<Pixmap>, after: Vec<Pixmap> },
}

impl Change {
    fn bytes(&self) -> usize {
        match self {
            Change::Pixels { before, after, .. } => before.data().len() + after.data().len(),
            Change::Insert { layer, .. } | Change::Remove { layer, .. } => layer.pm.data().len(),
            Change::Move { .. } => 0,
            Change::Replace { before, after } => before.iter().chain(after).map(|p| p.data().len()).sum(),
        }
    }

    /// Apply forward (redo) or backward (undo) to a full set of layers.
    fn apply(&self, layers: &mut Vec<Layer>, forward: bool) {
        match self {
            Change::Pixels { layer, x, y, before, after } => {
                let src = if forward { after } else { before };
                raster::copy_into(&mut layers[*layer].pm, src, *x, *y);
            }
            Change::Insert { index, layer } | Change::Remove { index, layer } => {
                let inserting = matches!(self, Change::Insert { .. }) == forward;
                if inserting {
                    layers.insert(*index, layer.clone());
                } else {
                    layers.remove(*index);
                }
            }
            Change::Move { from, to } => {
                let (a, b) = if forward { (*from, *to) } else { (*to, *from) };
                let l = layers.remove(a);
                layers.insert(b, l);
            }
            Change::Replace { before, after } => {
                let src = if forward { after } else { before };
                for (l, p) in layers.iter_mut().zip(src) {
                    l.pm = p.clone();
                }
            }
        }
    }
}

/// Pixel change of one layer from `before` to `after`, or None if identical.
pub fn pixels(layer: usize, before: &Pixmap, after: &Pixmap) -> Option<Change> {
    let r = raster::diff_bbox(before, after)?;
    Some(Change::Pixels {
        layer,
        x: r.x(),
        y: r.y(),
        before: before.clone_rect(r).unwrap(),
        after: after.clone_rect(r).unwrap(),
    })
}

struct Entry {
    id: u64,
    changes: Vec<Change>,
    active_before: usize,
    active_after: usize,
}

impl Entry {
    fn bytes(&self) -> usize {
        self.changes.iter().map(Change::bytes).sum()
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
    /// Record a pixel edit of one layer. Returns false if nothing changed.
    pub fn record(&mut self, layer: usize, before: &Pixmap, after: &Pixmap) -> bool {
        match pixels(layer, before, after) {
            Some(c) => self.push(vec![c], layer, layer),
            None => false,
        }
    }

    /// Record a group of changes as one undo step.
    pub fn push(&mut self, changes: Vec<Change>, active_before: usize, active_after: usize) -> bool {
        if changes.is_empty() {
            return false;
        }
        self.next_id += 1;
        let e = Entry { id: self.next_id, changes, active_before, active_after };
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

    /// Undo one step on the full set of layers. Returns the active layer to select.
    pub fn undo(&mut self, layers: &mut Vec<Layer>) -> Option<usize> {
        let e = self.undo.pop()?;
        for c in e.changes.iter().rev() {
            c.apply(layers, false);
        }
        let a = e.active_before;
        self.redo.push(e);
        Some(a)
    }

    pub fn redo(&mut self, layers: &mut Vec<Layer>) -> Option<usize> {
        let e = self.redo.pop()?;
        for c in &e.changes {
            c.apply(layers, true);
        }
        let a = e.active_after;
        self.undo.push(e);
        Some(a)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Identifies the current document state (for the unsaved-changes marker).
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

    fn layer(c: Color32) -> Layer {
        Layer::new("L".into(), raster::new_canvas(4, 4, c))
    }

    #[test]
    fn undo_redo_pixels_and_structure() {
        let mut layers = vec![layer(Color32::WHITE)];
        let mut h = History::default();
        let before = layers[0].pm.clone();
        raster::set_px(&mut layers[0].pm, 1, 1, raster::premul(Color32::BLACK));
        assert!(h.record(0, &before, &layers[0].pm));
        assert!(!h.record(0, &layers[0].pm.clone(), &layers[0].pm));
        let red = layer(Color32::RED);
        layers.push(red.clone());
        h.push(vec![Change::Insert { index: 1, layer: red }], 0, 1);
        assert_eq!(h.undo(&mut layers), Some(0));
        assert_eq!(layers.len(), 1);
        assert_eq!(h.undo(&mut layers), Some(0));
        assert_eq!(raster::pixel(&layers[0].pm, 1, 1), Some(Color32::WHITE));
        h.redo(&mut layers);
        assert_eq!(h.redo(&mut layers), Some(1));
        assert_eq!(layers.len(), 2);
        assert_eq!(raster::pixel(&layers[0].pm, 1, 1), Some(Color32::BLACK));
    }
}
