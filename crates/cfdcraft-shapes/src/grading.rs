//! Cell grading along one block axis.
//!
//! A section request becomes a list of [`Segment`]s, the multi-grading triples a block mesh
//! dictionary understands. The expansion of a segment is the size of its last cell divided by the
//! size of its first cell. Refining by a level multiplies the cells of a section by 2^level and
//! takes the 2^level-th root of its per-cell growth ratio, so every coarse cell edge is also a
//! fine cell edge.

use crate::{MAX_LEVEL, Segment};

/// The whole axis as one evenly spaced section.
pub fn uniform() -> Vec<Segment> {
    vec![Segment {
        length_fraction: 1.0,
        cell_fraction: 1.0,
        expansion: 1.0,
    }]
}

/// The per-cell growth ratio at `level`, given the ratio at level 0.
pub fn refine(ratio: f64, level: u32) -> f64 {
    ratio.powf(1.0 / f64::from(1u32 << level.min(MAX_LEVEL)))
}

/// Growth of the last cell over the first cell of a section of `cells` cells with ratio `ratio`.
fn expansion(cells: u64, ratio: f64) -> f64 {
    ratio.powf(cells.saturating_sub(1) as f64)
}

fn is_one(ratio: f64) -> bool {
    (ratio - 1.0).abs() < f64::EPSILON
}

/// Two halves that each grow from a wall toward the middle: the smallest cells are at both ends.
/// `cells` is the effective count of the axis and must be even; `ratio` is the per-cell growth
/// ratio already refined for the level.
pub fn two_sided(cells: u64, ratio: f64) -> Vec<Segment> {
    if is_one(ratio) {
        return uniform();
    }
    let e = expansion(cells / 2, ratio);
    vec![
        Segment {
            length_fraction: 0.5,
            cell_fraction: 0.5,
            expansion: e,
        },
        Segment {
            length_fraction: 0.5,
            cell_fraction: 0.5,
            expansion: 1.0 / e,
        },
    ]
}

/// One section whose smallest cells are at the start of the axis (`toward_start`) or at its end.
pub fn one_sided(cells: u64, ratio: f64, toward_start: bool) -> Vec<Segment> {
    if is_one(ratio) {
        return uniform();
    }
    let e = expansion(cells, ratio);
    vec![Segment {
        length_fraction: 1.0,
        cell_fraction: 1.0,
        expansion: if toward_start { e } else { 1.0 / e },
    }]
}

/// The positions of the `cells + 1` cell edges along an axis of `length` metres.
pub fn edges(segments: &[Segment], cells: u64, length: f64) -> Vec<f64> {
    let mut out = vec![0.0];
    let mut start = 0.0;
    let mut done = 0u64;
    let last = segments.len().saturating_sub(1);
    for (i, s) in segments.iter().enumerate() {
        let left = cells - done;
        let n = if i == last {
            left
        } else {
            ((s.cell_fraction * cells as f64).round() as u64).min(left)
        };
        if n == 0 {
            continue;
        }
        let section = s.length_fraction * length;
        let growth = if n > 1 {
            s.expansion.powf(1.0 / (n - 1) as f64)
        } else {
            1.0
        };
        let first = if is_one(growth) {
            section / n as f64
        } else {
            section * (growth - 1.0) / (growth.powf(n as f64) - 1.0)
        };
        let (mut pos, mut size) = (start, first);
        for _ in 0..n {
            pos += size;
            size *= growth;
            out.push(pos);
        }
        start += section;
        if let Some(edge) = out.last_mut() {
            *edge = start;
        }
        done += n;
    }
    out
}
