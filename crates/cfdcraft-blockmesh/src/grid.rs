//! Structured grids of the blocks, with the nodes of shared edges merged.

use crate::curve::Curve;
use crate::{BlockMeshError, MAX_CELLS};
use cfdcraft_shapes::{Layout, Segment, edges};
use std::collections::HashMap;

/// A cell with an area at or below this (in square metres) is refused.
const MIN_AREA: f64 = 1e-12;
/// Relative to the size of the layout: how far two blocks may differ on a shared edge node.
const EDGE_TOL: f64 = 1e-9;

/// The four sides of a block, counter-clockwise: the two corners and the node ids along the side.
pub(crate) type Sides = [([usize; 2], Vec<u32>); 4];

/// A block seen in the plane: corners counter-clockwise, direction 0 from corner 0 to 1 and
/// direction 1 from corner 0 to 3.
pub(crate) struct FlatBlock {
    pub corners: [usize; 4],
    pub cells: [usize; 2],
    pub grading: [Vec<Segment>; 2],
}

/// The layout seen in the plane.
pub(crate) struct Flat {
    /// For each vertex, the vertex of the lower plane it lies above (itself when it is lower).
    pub canon: Vec<usize>,
    pub blocks: Vec<FlatBlock>,
    /// The point an arc passes through, by the two vertices it joins (smaller first).
    pub arcs: HashMap<(usize, usize), [f64; 2]>,
    pub thickness: f64,
    /// The larger side of the box around the layout.
    pub scale: f64,
}

/// The node ids of one block's grid, row by row (direction 0 runs fastest).
pub(crate) struct BlockGrid {
    pub corners: [usize; 4],
    pub nx: usize,
    pub ny: usize,
    pub ids: Vec<u32>,
}

impl BlockGrid {
    /// The four sides counter-clockwise, so the block lies to the left of each.
    pub(crate) fn sides(&self) -> Sides {
        let (w, nx, ny) = (self.nx + 1, self.nx, self.ny);
        let at = |i: usize, j: usize| self.ids[j * w + i];
        let bottom = (0..=nx).map(|i| at(i, 0)).collect();
        let right = (0..=ny).map(|j| at(nx, j)).collect();
        let top = (0..=nx).rev().map(|i| at(i, ny)).collect();
        let left = (0..=ny).rev().map(|j| at(0, j)).collect();
        let [c0, c1, c2, c3] = self.corners;
        [
            ([c0, c1], bottom),
            ([c1, c2], right),
            ([c2, c3], top),
            ([c3, c0], left),
        ]
    }
}

/// Every grid joined into one mesh.
pub(crate) struct Filled {
    pub nodes: Vec<[f64; 2]>,
    pub quads: Vec<[u32; 4]>,
    pub blocks: Vec<BlockGrid>,
}

fn too_many(cells: u64) -> BlockMeshError {
    BlockMeshError::TooManyCells {
        cells,
        limit: MAX_CELLS,
    }
}

fn to_usize(v: u64) -> Result<usize, BlockMeshError> {
    usize::try_from(v).map_err(|_| too_many(v))
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * (1.0 + a.abs())
}

/// The signed area of a polygon, counter-clockwise positive.
fn shoelace(p: &[[f64; 2]]) -> f64 {
    let mut twice = 0.0;
    for (k, a) in p.iter().enumerate() {
        let b = p[(k + 1) % p.len()];
        twice += a[0] * b[1] - b[0] * a[1];
    }
    twice / 2.0
}

/// Look at the layout as a flat shape: which vertices are one point above the other, and each
/// block's corners and counts, turned counter-clockwise when they are not.
pub(crate) fn flatten(layout: &Layout) -> Result<Flat, BlockMeshError> {
    let n = layout.vertices.len();
    let mut canon: Vec<usize> = (0..n).collect();
    let mut total = 0u64;
    for (b, block) in layout.blocks.iter().enumerate() {
        let not_flat = |reason: &'static str| BlockMeshError::NotFlat { block: b, reason };
        if block.vertices.iter().any(|&v| v >= n) {
            return Err(BlockMeshError::MissingVertex);
        }
        if block.cells[2] != 1 {
            return Err(not_flat("it has more than one cell through its depth"));
        }
        let [nx, ny, _] = block.cells;
        if nx == 0 || ny == 0 {
            return Err(not_flat("it has a direction without cells"));
        }
        total = nx
            .checked_mul(ny)
            .and_then(|c| total.checked_add(c))
            .ok_or_else(|| too_many(u64::MAX))?;
        if total > MAX_CELLS {
            return Err(too_many(total));
        }
        let (low, high) = block.vertices.split_at(4);
        for (&l, &h) in low.iter().zip(high) {
            let (p, q) = (layout.vertices[l], layout.vertices[h]);
            if !same(p[0], q[0]) || !same(p[1], q[1]) {
                return Err(not_flat("its upper face is not above its lower face"));
            }
            canon[h] = l;
        }
    }
    if total != layout.cell_count {
        return Err(BlockMeshError::CountMismatch {
            blocks: total,
            layout: layout.cell_count,
        });
    }
    let mut blocks = Vec::with_capacity(layout.blocks.len());
    for (b, block) in layout.blocks.iter().enumerate() {
        let [c0, c1, c2, c3] = std::array::from_fn(|k| block.vertices[k]);
        if [c0, c1, c2, c3].iter().any(|&c| canon[c] != c) {
            return Err(BlockMeshError::NotFlat {
                block: b,
                reason: "a lower corner is an upper corner of another block",
            });
        }
        let mut distinct = [c0, c1, c2, c3];
        distinct.sort_unstable();
        if distinct.windows(2).any(|w| w[0] == w[1]) {
            return Err(BlockMeshError::DegenerateBlock { block: b });
        }
        let pt = |c: usize| [layout.vertices[c][0], layout.vertices[c][1]];
        let area = shoelace(&[pt(c0), pt(c1), pt(c2), pt(c3)]);
        if area.is_nan() || area == 0.0 {
            return Err(BlockMeshError::DegenerateBlock { block: b });
        }
        let [nx, ny, _] = block.cells;
        let [gx, gy] = [&block.grading[0], &block.grading[1]];
        let (nx, ny) = (to_usize(nx)?, to_usize(ny)?);
        blocks.push(if area > 0.0 {
            FlatBlock {
                corners: [c0, c1, c2, c3],
                cells: [nx, ny],
                grading: [gx.clone(), gy.clone()],
            }
        } else {
            // Clockwise: swap the two directions so the corners run counter-clockwise.
            FlatBlock {
                corners: [c0, c3, c2, c1],
                cells: [ny, nx],
                grading: [gy.clone(), gx.clone()],
            }
        });
    }
    let mut arcs = HashMap::new();
    for a in &layout.arcs {
        let (Some(&from), Some(&to)) = (canon.get(a.from), canon.get(a.to)) else {
            return Err(BlockMeshError::MissingVertex);
        };
        // The arcs of the upper plane join the same two points: the first one found stands.
        arcs.entry((from.min(to), from.max(to)))
            .or_insert([a.through[0], a.through[1]]);
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for v in &layout.vertices {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let thickness = if hi[2] >= lo[2] { hi[2] - lo[2] } else { 0.0 };
    let scale = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    Ok(Flat {
        canon,
        blocks,
        arcs,
        thickness,
        scale: if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        },
    })
}

/// The nodes made so far, and which are shared by corner and by edge.
struct Nodes {
    points: Vec<[f64; 2]>,
    corners: HashMap<usize, u32>,
    /// Ids along an edge from its smaller vertex to its larger one.
    edges: HashMap<(usize, usize), Vec<u32>>,
    tol: f64,
}

impl Nodes {
    fn push(&mut self, p: [f64; 2]) -> Result<u32, BlockMeshError> {
        let id = u32::try_from(self.points.len())
            .map_err(|_| too_many(u64::try_from(self.points.len()).unwrap_or(u64::MAX)))?;
        self.points.push(p);
        Ok(id)
    }

    fn corner(&mut self, vertex: usize, p: [f64; 2]) -> Result<u32, BlockMeshError> {
        if let Some(&id) = self.corners.get(&vertex) {
            return Ok(id);
        }
        let id = self.push(p)?;
        self.corners.insert(vertex, id);
        Ok(id)
    }

    /// The ids along the edge from vertex `u` to vertex `v` whose points are `pts` (both ends
    /// included). A second block on the same edge gets the same ids, and must place the same points.
    fn edge(&mut self, u: usize, v: usize, pts: &[[f64; 2]]) -> Result<Vec<u32>, BlockMeshError> {
        let key = (u.min(v), u.max(v));
        let mismatch = BlockMeshError::EdgeMismatch { a: key.0, b: key.1 };
        let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else {
            return Err(mismatch);
        };
        if pts.len() < 2 {
            return Err(mismatch);
        }
        let forward = u <= v;
        let along: Vec<[f64; 2]> = if forward {
            pts.to_vec()
        } else {
            pts.iter().rev().copied().collect()
        };
        let ids = if let Some(ids) = self.edges.get(&key).cloned() {
            if ids.len() != along.len() {
                return Err(mismatch);
            }
            for (&id, p) in ids.iter().zip(&along) {
                let q = self.points[id as usize];
                if (q[0] - p[0]).abs() > self.tol || (q[1] - p[1]).abs() > self.tol {
                    return Err(mismatch);
                }
            }
            ids
        } else {
            let (from, to) = if forward {
                (first, last)
            } else {
                (last, first)
            };
            let lo = self.corner(key.0, from)?;
            let hi = self.corner(key.1, to)?;
            let mut ids = vec![lo];
            for p in &along[1..along.len() - 1] {
                ids.push(self.push(*p)?);
            }
            ids.push(hi);
            self.edges.insert(key, ids.clone());
            ids
        };
        Ok(if forward {
            ids
        } else {
            ids.into_iter().rev().collect()
        })
    }
}

/// The fractions 0 to 1 at which a graded direction places its `cells` + 1 cell edges.
fn fractions(
    block: usize,
    direction: usize,
    grading: &[Segment],
    cells: usize,
) -> Result<Vec<f64>, BlockMeshError> {
    let s = edges(grading, cells as u64, 1.0);
    let sound = s.len() == cells + 1
        && s.iter().all(|v| v.is_finite())
        && s.windows(2).all(|w| w[1] > w[0]);
    if sound {
        Ok(s)
    } else {
        Err(BlockMeshError::BadGrading {
            block,
            direction,
            cells: cells as u64,
        })
    }
}

/// The points of the block edge from vertex `a` to vertex `b` at the fractions `s` of its length:
/// an arc when the layout names one for the pair, else a straight line. The ends are exact.
fn edge_points(
    flat: &Flat,
    (a, pa): (usize, [f64; 2]),
    (b, pb): (usize, [f64; 2]),
    s: &[f64],
) -> Result<Vec<[f64; 2]>, BlockMeshError> {
    let curve = match flat.arcs.get(&(a.min(b), a.max(b))) {
        Some(&through) => {
            Curve::arc(pa, pb, through).ok_or(BlockMeshError::DegenerateArc { from: a, to: b })?
        }
        None => Curve::line(pa, pb),
    };
    let last = s.len().saturating_sub(1);
    Ok(s.iter()
        .enumerate()
        .map(|(k, &t)| {
            if k == 0 {
                pa
            } else if k == last {
                pb
            } else {
                curve.at(t)
            }
        })
        .collect())
}

/// The four edge curves of a block at a point of the unit square, blended: the transfinite (Coons)
/// interpolation. `p` are the corners, the rest the points of the edges at the same fractions.
fn coons(
    (s, t): (f64, f64),
    p: &[[f64; 2]; 4],
    [bottom, top, left, right]: [[f64; 2]; 4],
) -> [f64; 2] {
    std::array::from_fn(|k| {
        (1.0 - t) * bottom[k] + t * top[k] + (1.0 - s) * left[k] + s * right[k]
            - ((1.0 - s) * (1.0 - t) * p[0][k]
                + s * (1.0 - t) * p[1][k]
                + s * t * p[2][k]
                + (1.0 - s) * t * p[3][k])
    })
}

fn quad_area(points: &[[f64; 2]], q: [u32; 4]) -> f64 {
    shoelace(&q.map(|id| points[id as usize]))
}

/// Join every block's grid into one mesh.
pub(crate) fn fill(layout: &Layout, flat: &Flat) -> Result<Filled, BlockMeshError> {
    let mut nodes = Nodes {
        points: Vec::new(),
        corners: HashMap::new(),
        edges: HashMap::new(),
        tol: EDGE_TOL * flat.scale,
    };
    let mut quads = Vec::with_capacity(usize::try_from(layout.cell_count).unwrap_or(0));
    let mut grids = Vec::with_capacity(flat.blocks.len());
    for (b, block) in flat.blocks.iter().enumerate() {
        grids.push(fill_block(layout, flat, b, block, &mut nodes, &mut quads)?);
    }
    Ok(Filled {
        nodes: nodes.points,
        quads,
        blocks: grids,
    })
}

fn fill_block(
    layout: &Layout,
    flat: &Flat,
    b: usize,
    block: &FlatBlock,
    nodes: &mut Nodes,
    quads: &mut Vec<[u32; 4]>,
) -> Result<BlockGrid, BlockMeshError> {
    let [nx, ny] = block.cells;
    let [c0, c1, c2, c3] = block.corners;
    let pt = |c: usize| [layout.vertices[c][0], layout.vertices[c][1]];
    let corner_points = [pt(c0), pt(c1), pt(c2), pt(c3)];
    let sx = fractions(b, 0, &block.grading[0], nx)?;
    let sy = fractions(b, 1, &block.grading[1], ny)?;
    let bottom = edge_points(flat, (c0, pt(c0)), (c1, pt(c1)), &sx)?;
    let top = edge_points(flat, (c3, pt(c3)), (c2, pt(c2)), &sx)?;
    let left = edge_points(flat, (c0, pt(c0)), (c3, pt(c3)), &sy)?;
    let right = edge_points(flat, (c1, pt(c1)), (c2, pt(c2)), &sy)?;
    let bottom_ids = nodes.edge(c0, c1, &bottom)?;
    let top_ids = nodes.edge(c3, c2, &top)?;
    let left_ids = nodes.edge(c0, c3, &left)?;
    let right_ids = nodes.edge(c1, c2, &right)?;

    let w = nx + 1;
    let mut ids = vec![0u32; w * (ny + 1)];
    for (i, (&lo, &hi)) in bottom_ids.iter().zip(&top_ids).enumerate() {
        ids[i] = lo;
        ids[ny * w + i] = hi;
    }
    for (j, (&l, &r)) in left_ids.iter().zip(&right_ids).enumerate() {
        ids[j * w] = l;
        ids[j * w + nx] = r;
    }
    for j in 1..ny {
        for i in 1..nx {
            let at = (sx[i], sy[j]);
            let p = coons(at, &corner_points, [bottom[i], top[i], left[j], right[j]]);
            ids[j * w + i] = nodes.push(p)?;
        }
    }
    for j in 0..ny {
        for i in 0..nx {
            let q = [
                ids[j * w + i],
                ids[j * w + i + 1],
                ids[(j + 1) * w + i + 1],
                ids[(j + 1) * w + i],
            ];
            let area = quad_area(&nodes.points, q);
            if area.is_nan() || area <= MIN_AREA {
                return Err(BlockMeshError::DegenerateCell { block: b, i, j });
            }
            quads.push(q);
        }
    }
    Ok(BlockGrid {
        corners: block.corners,
        nx,
        ny,
        ids,
    })
}
