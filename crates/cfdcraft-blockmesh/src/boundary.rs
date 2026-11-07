//! The boundary edges of each group, taken from the block edges the layout's faces name.

use crate::grid::BlockGrid;
use crate::{BlockMeshError, BoundaryEdges};
use cfdcraft_shapes::{Layout, Side};
use std::collections::{BTreeMap, HashMap};

/// For each block edge (its two corners, smaller first): how many blocks have it and its node
/// pairs counter-clockwise around the first block that has it.
type EdgeMap = HashMap<(usize, usize), (usize, Vec<[u32; 2]>)>;

/// Group the edges of the boundary faces by the side they belong to.
///
/// A face of the layout stands on one edge of the plane shape. That edge must belong to exactly
/// one block, and its grid edges go to the face's side, each running with its cell on the left.
/// The front and back faces are skipped: a flat mesh has no such boundary.
pub(crate) fn group(
    layout: &Layout,
    canon: &[usize],
    blocks: &[BlockGrid],
) -> Result<Vec<BoundaryEdges>, BlockMeshError> {
    let mut map: EdgeMap = HashMap::new();
    for grid in blocks {
        for ([a, b], ids) in grid.sides() {
            let entry = map
                .entry((a.min(b), a.max(b)))
                .or_insert_with(|| (0, Vec::new()));
            entry.0 += 1;
            if entry.0 == 1 {
                entry.1 = ids.windows(2).map(|w| [w[0], w[1]]).collect();
            }
        }
    }
    let mut groups: BTreeMap<Side, Vec<[u32; 2]>> = BTreeMap::new();
    for (k, q) in layout.quads.iter().enumerate() {
        if q.side == Side::FrontBack {
            continue;
        }
        let mut pair: Vec<usize> = Vec::with_capacity(4);
        for &v in &q.vertices {
            let c = canon.get(v).copied().ok_or(BlockMeshError::MissingVertex)?;
            if !pair.contains(&c) {
                pair.push(c);
            }
        }
        let &[a, b] = pair.as_slice() else {
            return Err(BlockMeshError::UnmatchedBoundary { quad: k });
        };
        match map.get(&(a.min(b), a.max(b))) {
            Some((1, edges)) => groups.entry(q.side).or_default().extend_from_slice(edges),
            _ => return Err(BlockMeshError::UnmatchedBoundary { quad: k }),
        }
    }
    Ok(groups
        .into_iter()
        .map(|(side, edges)| BoundaryEdges { side, edges })
        .collect())
}
