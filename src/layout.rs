//! Arbol binario de layout de panes.

use crate::session::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub cols: usize,
    pub rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Divide columnas (pane izquierdo / derecho).
    Vertical,
    /// Divide filas (pane superior / inferior).
    Horizontal,
}

#[derive(Debug, Clone)]
pub enum Layout {
    Leaf(SessionId),
    Split {
        orient: Orientation,
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

impl Layout {
    pub fn leaf(id: SessionId) -> Self {
        Layout::Leaf(id)
    }

    pub fn split(orient: Orientation, ratio: f32, a: Layout, b: Layout) -> Self {
        Layout::Split {
            orient,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    /// Rectangulo (en celdas) de cada hoja, dado el area total. Una celda de
    /// divider entre los dos hijos de cada split.
    pub fn rects(&self, area: Rect) -> Vec<(SessionId, Rect)> {
        match self {
            Layout::Leaf(id) => vec![(*id, area)],
            Layout::Split {
                orient,
                ratio,
                a,
                b,
            } => {
                let (ra, rb) = split_area(area, *orient, *ratio);
                let mut out = a.rects(ra);
                out.extend(b.rects(rb));
                out
            }
        }
    }

    /// Celdas ocupadas por dividers entre panes.
    pub fn divider_rects(&self, area: Rect) -> Vec<Rect> {
        let mut out = Vec::new();
        collect_dividers(self, area, &mut out);
        out
    }
}

fn collect_dividers(layout: &Layout, area: Rect, out: &mut Vec<Rect>) {
    match layout {
        Layout::Leaf(_) => {}
        Layout::Split {
            orient,
            ratio,
            a,
            b,
        } => {
            let (ra, rb) = split_area(area, *orient, *ratio);
            match orient {
                Orientation::Vertical => out.push(Rect {
                    x: ra.x + ra.cols,
                    y: area.y,
                    cols: 1,
                    rows: area.rows,
                }),
                Orientation::Horizontal => out.push(Rect {
                    x: area.x,
                    y: ra.y + ra.rows,
                    cols: area.cols,
                    rows: 1,
                }),
            }
            collect_dividers(a, ra, out);
            collect_dividers(b, rb, out);
        }
    }
}

fn split_area(area: Rect, orient: Orientation, ratio: f32) -> (Rect, Rect) {
    match orient {
        Orientation::Vertical => {
            let usable = area.cols.saturating_sub(1);
            let left = ((usable as f32 * ratio).round() as usize)
                .clamp(1, usable.saturating_sub(1).max(1));
            let a = Rect { cols: left, ..area };
            let b = Rect {
                x: area.x + left + 1,
                cols: usable - left,
                ..area
            };
            (a, b)
        }
        Orientation::Horizontal => {
            let usable = area.rows.saturating_sub(1);
            let top = ((usable as f32 * ratio).round() as usize)
                .clamp(1, usable.saturating_sub(1).max(1));
            let a = Rect { rows: top, ..area };
            let b = Rect {
                y: area.y + top + 1,
                rows: usable - top,
                ..area
            };
            (a, b)
        }
    }
}

/// Layout de panes dentro de una tab, con foco en una hoja.
#[derive(Debug, Clone)]
pub struct TabLayout {
    root: Layout,
    focused: SessionId,
}

impl TabLayout {
    pub fn new(id: SessionId) -> Self {
        Self {
            root: Layout::leaf(id),
            focused: id,
        }
    }

    pub fn focused(&self) -> SessionId {
        self.focused
    }

    pub fn layout(&self) -> &Layout {
        &self.root
    }

    pub fn leaves(&self) -> Vec<SessionId> {
        let mut out = Vec::new();
        collect_leaves(&self.root, &mut out);
        out
    }

    /// Divide la hoja enfocada; la nueva hoja recibe el foco.
    pub fn split_focused(&mut self, orient: Orientation, new_id: SessionId) {
        let old = self.focused;
        self.root = replace_leaf(
            &self.root,
            old,
            Layout::split(orient, 0.5, Layout::leaf(old), Layout::leaf(new_id)),
        );
        self.focused = new_id;
    }

    pub fn focus_next(&mut self) {
        let leaves = self.leaves();
        if leaves.len() <= 1 {
            return;
        }
        if let Some(i) = leaves.iter().position(|&id| id == self.focused) {
            self.focused = leaves[(i + 1) % leaves.len()];
        }
    }

    pub fn focus_prev(&mut self) {
        let leaves = self.leaves();
        if leaves.len() <= 1 {
            return;
        }
        if let Some(i) = leaves.iter().position(|&id| id == self.focused) {
            self.focused = leaves[(i + leaves.len() - 1) % leaves.len()];
        }
    }

    /// Colapsa la hoja enfocada. Devuelve el `SessionId` cerrado si habia mas de una hoja.
    pub fn close_focused(&mut self) -> Option<SessionId> {
        if self.leaves().len() <= 1 {
            return None;
        }
        let closing = self.focused;
        let (new_root, new_focus) = remove_leaf(&self.root, closing)?;
        self.root = new_root;
        self.focused = new_focus;
        Some(closing)
    }
}

fn collect_leaves(layout: &Layout, out: &mut Vec<SessionId>) {
    match layout {
        Layout::Leaf(id) => out.push(*id),
        Layout::Split { a, b, .. } => {
            collect_leaves(a, out);
            collect_leaves(b, out);
        }
    }
}

fn first_leaf(layout: &Layout) -> SessionId {
    match layout {
        Layout::Leaf(id) => *id,
        Layout::Split { a, .. } => first_leaf(a),
    }
}

fn replace_leaf(layout: &Layout, target: SessionId, replacement: Layout) -> Layout {
    match layout {
        Layout::Leaf(id) if *id == target => replacement,
        Layout::Leaf(_) => layout.clone(),
        Layout::Split {
            orient,
            ratio,
            a,
            b,
        } => Layout::Split {
            orient: *orient,
            ratio: *ratio,
            a: Box::new(replace_leaf(a, target, replacement.clone())),
            b: Box::new(replace_leaf(b, target, replacement)),
        },
    }
}

fn remove_leaf(layout: &Layout, target: SessionId) -> Option<(Layout, SessionId)> {
    match layout {
        Layout::Leaf(id) => {
            if *id == target {
                None
            } else {
                Some((layout.clone(), *id))
            }
        }
        Layout::Split {
            orient,
            ratio,
            a,
            b,
        } => {
            if contains_leaf(a, target) {
                match remove_leaf(a, target) {
                    None => Some((b.as_ref().clone(), first_leaf(b))),
                    Some((new_a, focus)) => Some((
                        Layout::Split {
                            orient: *orient,
                            ratio: *ratio,
                            a: Box::new(new_a),
                            b: b.clone(),
                        },
                        focus,
                    )),
                }
            } else if contains_leaf(b, target) {
                match remove_leaf(b, target) {
                    None => Some((a.as_ref().clone(), first_leaf(a))),
                    Some((new_b, focus)) => Some((
                        Layout::Split {
                            orient: *orient,
                            ratio: *ratio,
                            a: a.clone(),
                            b: Box::new(new_b),
                        },
                        focus,
                    )),
                }
            } else {
                None
            }
        }
    }
}

fn contains_leaf(layout: &Layout, id: SessionId) -> bool {
    match layout {
        Layout::Leaf(leaf) => *leaf == id,
        Layout::Split { a, b, .. } => contains_leaf(a, id) || contains_leaf(b, id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_hoja_ocupa_todo() {
        let id = SessionId(1);
        let layout = Layout::leaf(id);
        let rects = layout.rects(Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        });
        assert_eq!(
            rects,
            vec![(
                id,
                Rect {
                    x: 0,
                    y: 0,
                    cols: 80,
                    rows: 24
                }
            )]
        );
    }

    #[test]
    fn split_vertical_divide_columnas() {
        let a = SessionId(1);
        let b = SessionId(2);
        let layout = Layout::split(Orientation::Vertical, 0.5, Layout::leaf(a), Layout::leaf(b));
        let rects = layout.rects(Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        });
        assert_eq!(rects[0].1.cols + rects[1].1.cols, 79);
        assert_eq!(rects[0].1.rows, 24);
        assert_eq!(rects[1].1.x, rects[0].1.cols + 1);
    }

    #[test]
    fn split_y_focus_next_recorren_hojas() {
        let a = SessionId(1);
        let mut tab = TabLayout::new(a);
        let b = SessionId(2);
        tab.split_focused(Orientation::Vertical, b);
        assert_eq!(tab.leaves().len(), 2);
        let first = tab.focused();
        tab.focus_next();
        assert_ne!(tab.focused(), first);
    }
}
