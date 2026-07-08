//! Navegación espacial entre panes (tmux/wezterm).

use crate::session::SessionId;

use super::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

fn is_adjacent(me: &Rect, other: &Rect, dir: Direction) -> bool {
    match dir {
        Direction::Up => other.y + other.rows + 1 == me.y,
        Direction::Down => me.y + me.rows + 1 == other.y,
        Direction::Left => other.x + other.cols + 1 == me.x,
        Direction::Right => me.x + me.cols + 1 == other.x,
    }
}

fn perpendicular_overlap(me: &Rect, other: &Rect, dir: Direction) -> usize {
    match dir {
        Direction::Up | Direction::Down => {
            let left = me.x.max(other.x);
            let right = (me.x + me.cols).min(other.x + other.cols);
            right.saturating_sub(left)
        }
        Direction::Left | Direction::Right => {
            let top = me.y.max(other.y);
            let bottom = (me.y + me.rows).min(other.y + other.rows);
            bottom.saturating_sub(top)
        }
    }
}

fn mru_rank(id: SessionId, mru: &[SessionId]) -> usize {
    mru.iter().position(|&x| x == id).unwrap_or(usize::MAX)
}

/// Vecino geométrico en la dirección dada; empate por overlap → MRU.
pub fn spatial_neighbor(
    rects: &[(SessionId, Rect)],
    focused: SessionId,
    dir: Direction,
    mru: &[SessionId],
) -> Option<SessionId> {
    let (_, me) = rects.iter().find(|(id, _)| *id == focused)?;
    let mut best: Option<(SessionId, usize, usize)> = None;

    for (id, other) in rects {
        if *id == focused {
            continue;
        }
        if !is_adjacent(me, other, dir) {
            continue;
        }
        let overlap = perpendicular_overlap(me, other, dir);
        if overlap == 0 {
            continue;
        }
        let rank = mru_rank(*id, mru);
        match best {
            None => best = Some((*id, overlap, rank)),
            Some((_, best_overlap, best_rank)) => {
                if overlap > best_overlap || (overlap == best_overlap && rank < best_rank) {
                    best = Some((*id, overlap, rank));
                }
            }
        }
    }
    best.map(|(id, _, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_derecha_desde_izquierda() {
        let rects = vec![
            (
                SessionId(1),
                Rect {
                    x: 0,
                    y: 0,
                    cols: 39,
                    rows: 24,
                },
            ),
            (
                SessionId(2),
                Rect {
                    x: 40,
                    y: 0,
                    cols: 39,
                    rows: 24,
                },
            ),
        ];
        assert_eq!(
            spatial_neighbor(&rects, SessionId(1), Direction::Right, &[]),
            Some(SessionId(2))
        );
    }

    #[test]
    fn spatial_arriba_desde_abajo() {
        let rects = vec![
            (
                SessionId(1),
                Rect {
                    x: 0,
                    y: 0,
                    cols: 80,
                    rows: 11,
                },
            ),
            (
                SessionId(2),
                Rect {
                    x: 0,
                    y: 12,
                    cols: 80,
                    rows: 11,
                },
            ),
        ];
        assert_eq!(
            spatial_neighbor(&rects, SessionId(2), Direction::Up, &[]),
            Some(SessionId(1))
        );
    }
}
