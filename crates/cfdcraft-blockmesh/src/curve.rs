//! The curve of one block edge: a straight line or a circular arc.

use std::f64::consts::TAU;

/// A block edge in the plane, parameterised from 0 at its first end to 1 at its second.
pub(crate) enum Curve {
    Line {
        from: [f64; 2],
        to: [f64; 2],
    },
    /// An arc: the angle runs from `start` by `sweep` (negative for clockwise). Angle is
    /// proportional to the parameter, so the parameter is the fraction of the arc length.
    Arc {
        centre: [f64; 2],
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

impl Curve {
    pub(crate) fn line(from: [f64; 2], to: [f64; 2]) -> Self {
        Self::Line { from, to }
    }

    /// The arc from `from` to `to` through `through`; `None` when the three points are on a line.
    pub(crate) fn arc(from: [f64; 2], to: [f64; 2], through: [f64; 2]) -> Option<Self> {
        // Work relative to `from`: the circle through the origin, p and q.
        let p = [through[0] - from[0], through[1] - from[1]];
        let q = [to[0] - from[0], to[1] - from[1]];
        let (pp, qq) = (p[0] * p[0] + p[1] * p[1], q[0] * q[0] + q[1] * q[1]);
        let d = 2.0 * (p[0] * q[1] - p[1] * q[0]);
        if d.is_nan() || d.abs() <= f64::EPSILON * (pp + qq) {
            return None;
        }
        let u = [(q[1] * pp - p[1] * qq) / d, (p[0] * qq - q[0] * pp) / d];
        let angle = |x: [f64; 2]| (x[1] - u[1]).atan2(x[0] - u[0]);
        let start = angle([0.0, 0.0]);
        let (to_through, to_end) = (
            (angle(p) - start).rem_euclid(TAU),
            (angle(q) - start).rem_euclid(TAU),
        );
        // Going counter-clockwise from `from`, `through` must come before `to`; if not, the arc
        // runs the other way round.
        let sweep = if to_through < to_end {
            to_end
        } else {
            to_end - TAU
        };
        Some(Self::Arc {
            centre: [from[0] + u[0], from[1] + u[1]],
            radius: u[0].hypot(u[1]),
            start,
            sweep,
        })
    }

    /// The point at parameter `t`.
    pub(crate) fn at(&self, t: f64) -> [f64; 2] {
        match *self {
            Self::Line { from, to } => std::array::from_fn(|k| from[k] * (1.0 - t) + to[k] * t),
            Self::Arc {
                centre,
                radius,
                start,
                sweep,
            } => {
                let angle = start + sweep * t;
                [
                    centre[0] + radius * angle.cos(),
                    centre[1] + radius * angle.sin(),
                ]
            }
        }
    }
}
