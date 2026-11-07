//! The native output of a run as data: a comma separated table (the history and the volume
//! solution), the volume solution bound to the mesh of the study, and what can be read from it at a
//! point or along a wall. The judge and the result view both read through this one module.

use crate::{
    markers::{
        READ_CAP_BYTES, VOLUME_TEMPERATURE, VOLUME_VELOCITY_X, VOLUME_VELOCITY_Y, VOLUME_X,
        VOLUME_Y,
    },
    mesh::{layout_of, mesh_of},
};
use cfdcraft_blockmesh::Mesh2d;
use cfdcraft_model::ResolvedStudy;
use cfdcraft_shapes::Side;
use std::collections::HashMap;

/// A column name as compared: without quotes or padding, and without regard to case.
fn norm(name: &str) -> String {
    name.trim().trim_matches('"').trim().to_ascii_lowercase()
}

/// A table with a header row. A value that is not a number is kept as not-a-number, so a finite
/// check on the table also refuses a damaged one.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    names: Vec<String>,
    rows: Vec<Vec<f64>>,
}

impl Table {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let header = lines.next().ok_or("the table is empty")?;
        let names: Vec<String> = header.split(',').map(norm).collect();
        let mut rows = Vec::new();
        for line in lines {
            let row: Vec<f64> = line
                .split(',')
                .map(|w| w.trim().parse::<f64>().unwrap_or(f64::NAN))
                .collect();
            if row.len() != names.len() {
                return Err("a row holds another number of values than the header".to_owned());
            }
            rows.push(row);
        }
        Ok(Self { names, rows })
    }

    pub fn column(&self, name: &str) -> Option<usize> {
        let wanted = norm(name);
        self.names.iter().position(|n| *n == wanted)
    }

    pub fn rows(&self) -> &[Vec<f64>] {
        &self.rows
    }

    /// The values of one column.
    pub fn values(&self, name: &str) -> Option<Vec<f64>> {
        let at = self.column(name)?;
        Some(self.rows.iter().map(|r| r[at]).collect())
    }

    /// True when every value of the table is a finite number.
    pub fn all_finite(&self) -> bool {
        self.rows.iter().flatten().all(|v| v.is_finite())
    }
}

/// True when a text is within the read limit.
pub fn within_cap(text: &str) -> bool {
    text.len() as u64 <= READ_CAP_BYTES
}

/// The text of a file that is longer than the read limit: its last bytes, from a whole character
/// on. A table keeps its header line, so its columns can still be named.
pub fn cut_to_cap(text: &str, table: bool) -> String {
    let cap = usize::try_from(READ_CAP_BYTES).unwrap_or(usize::MAX);
    if text.len() <= cap {
        return text.to_owned();
    }
    let mut from = text.len() - cap;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let tail = &text[from..];
    if !table {
        return tail.to_owned();
    }
    // The tail starts inside a line, so it starts again at the next whole one.
    let header = text.lines().next().unwrap_or_default();
    let rest = tail.split_once('\n').map_or("", |(_, rest)| rest);
    format!("{header}\n{rest}")
}

/// The heat gradient integrated over a wall: its area and the integral of the gradient vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallIntegral {
    pub edges: usize,
    pub area: f64,
    pub gradient: [f64; 2],
}

/// The volume solution of a run bound to the mesh of its study.
pub struct Volume {
    mesh: Mesh2d,
    velocity: Vec<[f64; 2]>,
    temperature: Option<Vec<f64>>,
    boxes: Vec<[f64; 4]>,
    eps: f64,
}

impl Volume {
    /// Bind the table to the mesh the study makes. The rows must be the nodes of that mesh in
    /// order (checked by position), and every value used must be finite.
    pub fn bind(table: &Table, study: &ResolvedStudy) -> Result<Self, String> {
        let column = |name: &str| {
            table
                .column(name)
                .ok_or_else(|| format!("the volume file has no column {name}"))
        };
        let (cx, cy) = (column(VOLUME_X)?, column(VOLUME_Y)?);
        let (cu, cv) = (column(VOLUME_VELOCITY_X)?, column(VOLUME_VELOCITY_Y)?);
        let ct = table.column(VOLUME_TEMPERATURE);
        let mesh = layout_of(study)
            .and_then(|layout| mesh_of(&layout))
            .map_err(|_| "the mesh of the study cannot be rebuilt".to_owned())?;
        if table.rows().len() != mesh.nodes.len() {
            return Err("the volume file holds another number of nodes than the mesh".to_owned());
        }
        let range = |at: usize| {
            let (lo, hi) = mesh.nodes.iter().fold((f64::MAX, f64::MIN), |(lo, hi), n| {
                (lo.min(n[at]), hi.max(n[at]))
            });
            hi - lo
        };
        let scale = range(0).max(range(1));
        if !(scale.is_finite() && scale > 0.0) {
            return Err("the mesh of the study has no extent".to_owned());
        }
        for (row, node) in table.rows().iter().zip(&mesh.nodes) {
            if (row[cx] - node[0]).abs() > 1e-6 * scale || (row[cy] - node[1]).abs() > 1e-6 * scale
            {
                return Err("the volume rows are not in the node order of the mesh".to_owned());
            }
        }
        let finite = |at: usize| table.rows().iter().all(|r| r[at].is_finite());
        let usable = [cu, cv].into_iter().chain(ct).all(finite);
        if !usable {
            return Err("the volume file holds a value that is not finite".to_owned());
        }
        let boxes = mesh
            .quads
            .iter()
            .map(|q| {
                let pts = q.map(|i| mesh.nodes[i as usize]);
                let fold = |at: usize, init: f64, pick: fn(f64, f64) -> f64| {
                    pts.iter().fold(init, |m, p| pick(m, p[at]))
                };
                [
                    fold(0, f64::MAX, f64::min),
                    fold(1, f64::MAX, f64::min),
                    fold(0, f64::MIN, f64::max),
                    fold(1, f64::MIN, f64::max),
                ]
            })
            .collect();
        Ok(Self {
            velocity: table.rows().iter().map(|r| [r[cu], r[cv]]).collect(),
            temperature: ct.map(|at| table.rows().iter().map(|r| r[at]).collect()),
            boxes,
            eps: 1e-9 * scale,
            mesh,
        })
    }

    /// The depth of the case along z, in metres.
    pub fn depth(&self) -> f64 {
        self.mesh.thickness
    }

    /// The velocity at a point: the corner values blended by the bilinear map of the cell that
    /// holds the point, or `None` when no cell holds it. At a node this is the node's own value.
    pub fn sample(&self, x: f64, y: f64) -> Option<[f64; 2]> {
        let eps = self.eps;
        for (quad, bb) in self.mesh.quads.iter().zip(&self.boxes) {
            let outside = x < bb[0] - eps || x > bb[2] + eps || y < bb[1] - eps || y > bb[3] + eps;
            if outside {
                continue;
            }
            if let Some(w) = self.weights(quad, x, y) {
                return Some(self.blend(quad, w));
            }
        }
        None
    }

    /// The bilinear weights of the point in the cell, by Newton's method on the cell's map from
    /// the unit square.
    fn weights(&self, quad: &[u32; 4], x: f64, y: f64) -> Option<[f64; 4]> {
        let p = quad.map(|i| self.mesh.nodes[i as usize]);
        let (mut s, mut t) = (0.5_f64, 0.5_f64);
        for _ in 0..40 {
            let w = bilinear(s, t);
            let fx = w.iter().zip(&p).map(|(w, p)| w * p[0]).sum::<f64>() - x;
            let fy = w.iter().zip(&p).map(|(w, p)| w * p[1]).sum::<f64>() - y;
            let dxs = (1.0 - t) * (p[1][0] - p[0][0]) + t * (p[2][0] - p[3][0]);
            let dys = (1.0 - t) * (p[1][1] - p[0][1]) + t * (p[2][1] - p[3][1]);
            let dxt = (1.0 - s) * (p[3][0] - p[0][0]) + s * (p[2][0] - p[1][0]);
            let dyt = (1.0 - s) * (p[3][1] - p[0][1]) + s * (p[2][1] - p[1][1]);
            let det = dxs * dyt - dxt * dys;
            if det == 0.0 || !det.is_finite() {
                return None;
            }
            let ds = (fx * dyt - dxt * fy) / det;
            let dt = (dxs * fy - dys * fx) / det;
            s -= ds;
            t -= dt;
            if ds.abs() + dt.abs() < 1e-14 {
                break;
            }
        }
        let inside = |v: f64| (-1e-9..=1.0 + 1e-9).contains(&v);
        (inside(s) && inside(t)).then(|| bilinear(s.clamp(0.0, 1.0), t.clamp(0.0, 1.0)))
    }

    fn blend(&self, quad: &[u32; 4], w: [f64; 4]) -> [f64; 2] {
        let mix = |at: usize| {
            quad.iter()
                .zip(w)
                .map(|(i, w)| w * self.velocity[*i as usize][at])
                .sum::<f64>()
        };
        [mix(0), mix(1)]
    }

    /// The temperature gradient integrated over the walls on the given sides, times the depth.
    ///
    /// Rule, per boundary edge: the gradient across the wall is the difference between the mean
    /// temperature of the two nodes of the cell's opposite edge and the mean of the wall edge, over
    /// the distance between the two edge midpoints along the wall's normal; the gradient along the
    /// wall is the difference between the wall edge's two nodes over its length. The edge's
    /// contribution is that gradient vector times the edge length. It is a first-order estimate
    /// from the node values, like the wall gradient of a cell-centred code. `None` without a
    /// temperature column, an edge with no cell, or no edges on the sides.
    pub fn wall_gradient(&self, sides: &[Side]) -> Option<WallIntegral> {
        let temperature = self.temperature.as_ref()?;
        let mut cells: HashMap<(u32, u32), usize> = HashMap::new();
        for (at, q) in self.mesh.quads.iter().enumerate() {
            for (i, &a) in q.iter().enumerate() {
                let b = q[(i + 1) % 4];
                cells.insert((a.min(b), a.max(b)), at);
            }
        }
        let node = |i: u32| self.mesh.nodes[i as usize];
        let mut total = WallIntegral {
            edges: 0,
            area: 0.0,
            gradient: [0.0; 2],
        };
        for group in self
            .mesh
            .boundary
            .iter()
            .filter(|g| sides.contains(&g.side))
        {
            for &[a, b] in &group.edges {
                let quad = self.mesh.quads[*cells.get(&(a.min(b), a.max(b)))?];
                let at = quad.iter().position(|n| *n == a)?;
                if quad[(at + 1) % 4] != b {
                    return None;
                }
                let (c, d) = (quad[(at + 2) % 4], quad[(at + 3) % 4]);
                let (pa, pb) = (node(a), node(b));
                let length = (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
                if length == 0.0 {
                    return None;
                }
                let tangent = [(pb[0] - pa[0]) / length, (pb[1] - pa[1]) / length];
                // The cell is on the left of the edge, so the left normal points into it.
                let normal = [-tangent[1], tangent[0]];
                let (pc, pd) = (node(c), node(d));
                let apart = ((pc[0] + pd[0] - pa[0] - pb[0]) * normal[0]
                    + (pc[1] + pd[1] - pa[1] - pb[1]) * normal[1])
                    / 2.0;
                if apart == 0.0 {
                    return None;
                }
                let at_wall = (temperature[a as usize] + temperature[b as usize]) / 2.0;
                let inner = (temperature[c as usize] + temperature[d as usize]) / 2.0;
                let across = (inner - at_wall) / apart;
                let along = (temperature[b as usize] - temperature[a as usize]) / length;
                let depth = self.depth();
                total.edges += 1;
                total.area += length * depth;
                for ((g, n), t) in total.gradient.iter_mut().zip(normal).zip(tangent) {
                    *g += (across * n + along * t) * length * depth;
                }
            }
        }
        (total.edges > 0).then_some(total)
    }
}

fn bilinear(s: f64, t: f64) -> [f64; 4] {
    [(1.0 - s) * (1.0 - t), s * (1.0 - t), s * t, (1.0 - s) * t]
}
