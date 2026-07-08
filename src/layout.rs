//! Arbol binario de layout de panes (Hyprland dwindle).

mod smart_split;
mod spatial;

pub use smart_split::{smart_split_decision, SplitPlacement};
pub use spatial::{spatial_neighbor, Direction};

use crate::session::SessionId;

/// Minimo de celdas por lado hijo al dividir (4 celdas utiles + divider).
pub const MIN_PANE_COLS: usize = 4;
pub const MIN_PANE_ROWS: usize = 4;

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
        preserve_orient: bool,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

impl Layout {
    pub fn leaf(id: SessionId) -> Self {
        Layout::Leaf(id)
    }

    pub fn split(orient: Orientation, ratio: f32, a: Layout, b: Layout) -> Self {
        Self::split_with_preserve(orient, ratio, false, a, b)
    }

    pub fn split_with_preserve(
        orient: Orientation,
        ratio: f32,
        preserve_orient: bool,
        a: Layout,
        b: Layout,
    ) -> Self {
        Layout::Split {
            orient,
            ratio,
            preserve_orient,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    pub fn rects(&self, area: Rect) -> Vec<(SessionId, Rect)> {
        match self {
            Layout::Leaf(id) => vec![(*id, area)],
            Layout::Split {
                orient,
                ratio,
                a,
                b,
                ..
            } => {
                let (ra, rb) = split_area(area, *orient, *ratio);
                let mut out = a.rects(ra);
                out.extend(b.rects(rb));
                out
            }
        }
    }

    pub fn divider_rects(&self, area: Rect) -> Vec<Rect> {
        let mut out = Vec::new();
        collect_dividers(self, area, &mut out);
        out
    }

    pub fn recalc_dwindle_orients(&mut self, area: Rect, width_multiplier: f32) {
        match self {
            Layout::Leaf(_) => {}
            Layout::Split {
                orient,
                ratio,
                preserve_orient,
                a,
                b,
            } => {
                if !*preserve_orient {
                    *orient = dwindle_orient(area, width_multiplier);
                }
                let (ra, rb) = split_area(area, *orient, *ratio);
                a.recalc_dwindle_orients(ra, width_multiplier);
                b.recalc_dwindle_orients(rb, width_multiplier);
            }
        }
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
            ..
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

pub fn split_rect(area: Rect, orient: Orientation, ratio: f32) -> (Rect, Rect) {
    split_area(area, orient, ratio)
}

fn split_area(area: Rect, orient: Orientation, ratio: f32) -> (Rect, Rect) {
    match orient {
        Orientation::Vertical => {
            let usable = area.cols.saturating_sub(1);
            let left = ((usable as f32 * ratio).round() as usize)
                .clamp(1, usable.saturating_sub(1).max(1));
            let a = Rect { cols: left, ..area };
            let right_cols = usable.saturating_sub(left).max(1);
            let b = Rect {
                x: area.x + left + 1,
                cols: right_cols,
                ..area
            };
            (a, b)
        }
        Orientation::Horizontal => {
            let usable = area.rows.saturating_sub(1);
            let top = ((usable as f32 * ratio).round() as usize)
                .clamp(1, usable.saturating_sub(1).max(1));
            let a = Rect { rows: top, ..area };
            let bottom_rows = usable.saturating_sub(top).max(1);
            let b = Rect {
                y: area.y + top + 1,
                rows: bottom_rows,
                ..area
            };
            (a, b)
        }
    }
}

pub fn dwindle_orient(rect: Rect, width_multiplier: f32) -> Orientation {
    if rect.cols as f32 > rect.rows as f32 * width_multiplier {
        Orientation::Vertical
    } else {
        Orientation::Horizontal
    }
}

pub fn dwindle_split_orient(rect: Rect, width_multiplier: f32) -> Option<Orientation> {
    let primary = dwindle_orient(rect, width_multiplier);
    if can_split(rect, primary, MIN_PANE_COLS, MIN_PANE_ROWS) {
        return Some(primary);
    }
    let alt = match primary {
        Orientation::Vertical => Orientation::Horizontal,
        Orientation::Horizontal => Orientation::Vertical,
    };
    if can_split(rect, alt, MIN_PANE_COLS, MIN_PANE_ROWS) {
        return Some(alt);
    }
    None
}

pub fn can_split(rect: Rect, orient: Orientation, min_cols: usize, min_rows: usize) -> bool {
    match orient {
        Orientation::Vertical => {
            let usable = rect.cols.saturating_sub(1);
            usable >= min_cols * 2 && rect.rows >= min_rows
        }
        Orientation::Horizontal => {
            let usable = rect.rows.saturating_sub(1);
            usable >= min_rows * 2 && rect.cols >= min_cols
        }
    }
}

#[derive(Debug, Clone)]
pub struct TabLayout {
    root: Layout,
    focused: SessionId,
    pane_mru: Vec<SessionId>,
}

impl TabLayout {
    pub fn new(id: SessionId) -> Self {
        Self {
            root: Layout::leaf(id),
            focused: id,
            pane_mru: vec![id],
        }
    }

    pub fn focused(&self) -> SessionId {
        self.focused
    }

    pub fn layout(&self) -> &Layout {
        &self.root
    }

    pub fn recalc_dwindle_orients(&mut self, area: Rect, width_multiplier: f32) {
        self.root.recalc_dwindle_orients(area, width_multiplier);
    }

    pub fn leaves(&self) -> Vec<SessionId> {
        let mut out = Vec::new();
        collect_leaves(&self.root, &mut out);
        out
    }

    pub fn split_dwindle(&mut self, new_id: SessionId, orient: Orientation) {
        self.split_dwindle_ordered(new_id, orient, false, true);
    }

    pub fn split_dwindle_ordered(
        &mut self,
        new_id: SessionId,
        orient: Orientation,
        preserve: bool,
        old_first: bool,
    ) {
        let old = self.focused;
        let (a, b) = if old_first {
            (Layout::leaf(old), Layout::leaf(new_id))
        } else {
            (Layout::leaf(new_id), Layout::leaf(old))
        };
        let replacement = Layout::split_with_preserve(orient, 0.5, preserve, a, b);
        self.root = replace_leaf(&self.root, old, replacement);
        self.focused = new_id;
        self.touch_mru(new_id);
    }

    pub fn focus_next(&mut self) {
        let leaves = self.leaves_spatial();
        if leaves.len() <= 1 {
            return;
        }
        if let Some(i) = leaves.iter().position(|&id| id == self.focused) {
            let next = leaves[(i + 1) % leaves.len()];
            self.focus_pane(next);
        }
    }

    pub fn focus_prev(&mut self) {
        let leaves = self.leaves_spatial();
        if leaves.len() <= 1 {
            return;
        }
        if let Some(i) = leaves.iter().position(|&id| id == self.focused) {
            let prev = leaves[(i + leaves.len() - 1) % leaves.len()];
            self.focus_pane(prev);
        }
    }

    pub fn focus_direction(&mut self, area: Rect, dir: Direction) -> bool {
        let rects = self.root.rects(area);
        if let Some(next) = spatial_neighbor(&rects, self.focused, dir, &self.pane_mru) {
            self.focus_pane(next);
            return true;
        }
        false
    }

    pub fn toggle_split_focused(&mut self) -> bool {
        apply_parent_split(&mut self.root, self.focused, ParentSplitOp::Toggle)
    }

    pub fn swap_split_focused(&mut self) -> bool {
        apply_parent_split(&mut self.root, self.focused, ParentSplitOp::Swap)
    }

    pub fn leaves_spatial(&self) -> Vec<SessionId> {
        let area = Rect {
            x: 0,
            y: 0,
            cols: 10_000,
            rows: 10_000,
        };
        let mut leaves = self.root.rects(area);
        leaves.sort_by(|(_, a), (_, b)| a.y.cmp(&b.y).then_with(|| a.x.cmp(&b.x)));
        leaves.into_iter().map(|(id, _)| id).collect()
    }

    pub fn focus_pane(&mut self, id: SessionId) -> bool {
        if self.leaves().contains(&id) {
            self.focused = id;
            self.touch_mru(id);
            true
        } else {
            false
        }
    }

    pub fn close_pane(&mut self, target: SessionId) -> Option<SessionId> {
        if self.leaves().len() <= 1 {
            return None;
        }
        if !self.leaves().contains(&target) {
            return None;
        }
        let (new_root, new_focus) = remove_leaf(&self.root, target)?;
        self.root = new_root;
        self.focused = new_focus;
        self.pane_mru.retain(|&id| id != target);
        self.touch_mru(new_focus);
        Some(target)
    }

    pub fn close_focused(&mut self) -> Option<SessionId> {
        if self.leaves().len() <= 1 {
            return None;
        }
        let closing = self.focused;
        let (new_root, new_focus) = remove_leaf(&self.root, closing)?;
        self.root = new_root;
        self.focused = new_focus;
        self.pane_mru.retain(|&id| id != closing);
        self.touch_mru(new_focus);
        Some(closing)
    }

    fn touch_mru(&mut self, id: SessionId) {
        self.pane_mru.retain(|&x| x != id);
        self.pane_mru.insert(0, id);
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

fn is_direct_leaf(layout: &Layout, id: SessionId) -> bool {
    matches!(layout, Layout::Leaf(l) if *l == id)
}

fn replace_leaf(layout: &Layout, target: SessionId, replacement: Layout) -> Layout {
    match layout {
        Layout::Leaf(leaf_id) if *leaf_id == target => replacement,
        Layout::Leaf(_) => layout.clone(),
        Layout::Split {
            orient,
            ratio,
            preserve_orient,
            a,
            b,
        } => Layout::Split {
            orient: *orient,
            ratio: *ratio,
            preserve_orient: *preserve_orient,
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
            preserve_orient,
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
                            preserve_orient: *preserve_orient,
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
                            preserve_orient: *preserve_orient,
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

enum ParentSplitOp {
    Toggle,
    Swap,
}

fn apply_parent_split(layout: &mut Layout, target: SessionId, op: ParentSplitOp) -> bool {
    match layout {
        Layout::Leaf(_) => false,
        Layout::Split {
            orient,
            preserve_orient,
            a,
            b,
            ..
        } => {
            if is_direct_leaf(a, target) || is_direct_leaf(b, target) {
                match op {
                    ParentSplitOp::Toggle => {
                        *orient = match *orient {
                            Orientation::Vertical => Orientation::Horizontal,
                            Orientation::Horizontal => Orientation::Vertical,
                        };
                    }
                    ParentSplitOp::Swap => std::mem::swap(a, b),
                }
                *preserve_orient = true;
                true
            } else if contains_leaf(a, target) {
                apply_parent_split(a, target, op)
            } else if contains_leaf(b, target) {
                apply_parent_split(b, target, op)
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_mixed_orientations(layout: &Layout) -> bool {
        let mut seen = None;
        has_mixed_orientations_inner(layout, &mut seen)
    }

    fn has_mixed_orientations_inner(layout: &Layout, seen: &mut Option<Orientation>) -> bool {
        match layout {
            Layout::Leaf(_) => false,
            Layout::Split { orient, a, b, .. } => {
                if let Some(first) = seen {
                    if *first != *orient {
                        return true;
                    }
                } else {
                    *seen = Some(*orient);
                }
                has_mixed_orientations_inner(a, seen) || has_mixed_orientations_inner(b, seen)
            }
        }
    }

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
    fn dwindle_orient_respeta_multiplier() {
        let rect = Rect {
            x: 0,
            y: 0,
            cols: 10,
            rows: 10,
        };
        assert_eq!(dwindle_orient(rect, 1.0), Orientation::Horizontal);
        assert_eq!(dwindle_orient(rect, 0.5), Orientation::Vertical);
    }

    #[test]
    fn recalc_respeta_preserve_orient() {
        let mut layout = Layout::split_with_preserve(
            Orientation::Vertical,
            0.5,
            true,
            Layout::leaf(SessionId(1)),
            Layout::leaf(SessionId(2)),
        );
        layout.recalc_dwindle_orients(
            Rect {
                x: 0,
                y: 0,
                cols: 40,
                rows: 80,
            },
            1.0,
        );
        assert!(matches!(
            layout,
            Layout::Split {
                orient: Orientation::Vertical,
                ..
            }
        ));
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
        tab.split_dwindle(b, Orientation::Vertical);
        assert_eq!(tab.leaves().len(), 2);
        assert_eq!(tab.focused(), b);
        tab.focus_next();
        assert_eq!(tab.focused(), a);
    }

    #[test]
    fn dwindle_cuatro_splits_forman_espiral() {
        let area = Rect {
            x: 0,
            y: 0,
            cols: 120,
            rows: 40,
        };
        let mut tab = TabLayout::new(SessionId(1));
        for i in 2..=5 {
            let rect = tab
                .layout()
                .rects(area)
                .into_iter()
                .find(|(id, _)| *id == tab.focused())
                .unwrap()
                .1;
            let orient = dwindle_orient(rect, 1.0);
            tab.split_dwindle(SessionId(i), orient);
        }
        assert_eq!(tab.leaves().len(), 5);
        assert!(has_mixed_orientations(tab.layout()));
    }

    #[test]
    fn split_dwindle_foco_en_pane_nuevo() {
        let mut tab = TabLayout::new(SessionId(1));
        tab.split_dwindle(SessionId(2), Orientation::Vertical);
        assert_eq!(tab.focused(), SessionId(2));
    }

    #[test]
    fn recalc_dwindle_voltea_orient_al_estirar() {
        let mut layout = Layout::split(
            Orientation::Vertical,
            0.5,
            Layout::leaf(SessionId(1)),
            Layout::leaf(SessionId(2)),
        );
        let wide = Rect {
            x: 0,
            y: 0,
            cols: 120,
            rows: 24,
        };
        layout.recalc_dwindle_orients(wide, 1.0);
        assert!(matches!(
            layout,
            Layout::Split {
                orient: Orientation::Vertical,
                ..
            }
        ));

        let tall = Rect {
            x: 0,
            y: 0,
            cols: 40,
            rows: 80,
        };
        layout.recalc_dwindle_orients(tall, 1.0);
        assert!(matches!(
            layout,
            Layout::Split {
                orient: Orientation::Horizontal,
                ..
            }
        ));
    }

    #[test]
    fn toggle_split_cambia_orient_del_padre() {
        let mut tab = TabLayout::new(SessionId(1));
        tab.split_dwindle(SessionId(2), Orientation::Vertical);
        assert!(tab.toggle_split_focused());
        let area = Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        let rects = tab.layout().rects(area);
        assert_eq!(rects[0].1.rows + 1 + rects[1].1.rows, area.rows);
    }

    #[test]
    fn swap_split_intercambia_hijos() {
        let mut tab = TabLayout::new(SessionId(1));
        tab.split_dwindle(SessionId(2), Orientation::Vertical);
        let area = Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        let x_before = tab
            .layout()
            .rects(area)
            .into_iter()
            .find(|(id, _)| *id == SessionId(1))
            .unwrap()
            .1
            .x;
        assert!(tab.swap_split_focused());
        let x_after = tab
            .layout()
            .rects(area)
            .into_iter()
            .find(|(id, _)| *id == SessionId(1))
            .unwrap()
            .1
            .x;
        assert_ne!(x_before, x_after);
    }

    #[test]
    fn focus_direction_derecha() {
        let mut tab = TabLayout::new(SessionId(1));
        tab.split_dwindle(SessionId(2), Orientation::Vertical);
        tab.focus_pane(SessionId(1));
        let area = Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        assert!(tab.focus_direction(area, Direction::Right));
        assert_eq!(tab.focused(), SessionId(2));
    }

    #[test]
    fn split_rect_coincide_con_layout() {
        let area = Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        let (ra, rb) = split_rect(area, Orientation::Vertical, 0.5);
        let layout = Layout::split(
            Orientation::Vertical,
            0.5,
            Layout::leaf(SessionId(1)),
            Layout::leaf(SessionId(2)),
        );
        let rects = layout.rects(area);
        assert_eq!(rects[0].1, ra);
        assert_eq!(rects[1].1, rb);
    }

    #[test]
    fn split_horizontal_divide_filas() {
        let a = SessionId(1);
        let b = SessionId(2);
        let layout = Layout::split(
            Orientation::Horizontal,
            0.5,
            Layout::leaf(a),
            Layout::leaf(b),
        );
        let area = Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        let rects = layout.rects(area);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].1.rows + 1 + rects[1].1.rows, area.rows);
        assert_eq!(rects[1].1.y, rects[0].1.rows + 1);
    }

    #[test]
    fn divider_rects_entre_hijos() {
        let layout = Layout::split(
            Orientation::Vertical,
            0.5,
            Layout::leaf(SessionId(1)),
            Layout::leaf(SessionId(2)),
        );
        let area = Rect {
            x: 0,
            y: 0,
            cols: 80,
            rows: 24,
        };
        let rects = layout.rects(area);
        let dividers = layout.divider_rects(area);
        assert_eq!(dividers.len(), 1);
        assert_eq!(dividers[0].x, rects[0].1.cols);
        assert_eq!(dividers[0].cols, 1);
        assert_eq!(dividers[0].rows, area.rows);
    }

    #[test]
    fn can_split_rechaza_pane_demasiado_pequeno() {
        let tiny = Rect {
            x: 0,
            y: 0,
            cols: 6,
            rows: 24,
        };
        assert!(!can_split(
            tiny,
            Orientation::Vertical,
            MIN_PANE_COLS,
            MIN_PANE_ROWS
        ));
        let ok = Rect {
            x: 0,
            y: 0,
            cols: 9,
            rows: 24,
        };
        assert!(can_split(
            ok,
            Orientation::Vertical,
            MIN_PANE_COLS,
            MIN_PANE_ROWS
        ));
    }

    #[test]
    fn close_pane_y_focus_pane() {
        let a = SessionId(1);
        let mut tab = TabLayout::new(a);
        tab.split_dwindle(SessionId(2), Orientation::Vertical);
        assert!(tab.focus_pane(a));
        assert_eq!(tab.focused(), a);
        let closed = tab.close_pane(SessionId(2));
        assert_eq!(closed, Some(SessionId(2)));
        assert_eq!(tab.leaves(), vec![a]);
        assert_eq!(tab.focused(), a);
    }

    #[test]
    fn split_area_nunca_cero_en_hijos() {
        let area = Rect {
            x: 0,
            y: 0,
            cols: 3,
            rows: 3,
        };
        let (a, b) = split_rect(area, Orientation::Vertical, 0.5);
        assert!(a.cols >= 1);
        assert!(b.cols >= 1);
        assert!(a.rows >= 1);
        assert!(b.rows >= 1);
    }
}
