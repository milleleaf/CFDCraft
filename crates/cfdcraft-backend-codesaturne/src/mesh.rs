//! The mesh of a case: the two-dimensional block mesh extruded one cell through z and written as
//! Gmsh ASCII with a physical group for every part of the boundary and one for the two faces
//! along z.
//!
//! Gmsh format 2.2 is used because it is the simplest text form the solver imports: each element
//! line carries its type, two tags (the physical group and the geometric entity, equal here) and
//! its node numbers. Nodes of the lower layer are numbered first, the upper layer follows. The
//! groups are numbered in the order of [`SIDE_ORDER`] for the parts the shape has, then the z
//! faces, then the cells.

use crate::{
    boundaries::{SIDE_ORDER, side_group},
    setup::{Z_FACES_GROUP, number},
};
use cfdcraft_backend::CompileFailure;
use cfdcraft_blockmesh::{BlockMeshError, Mesh2d, build};
use cfdcraft_model::{ResolvedShape, ResolvedStudy};
use cfdcraft_shapes::{DomainSpec, Layout, MeshRequest, ShapeSpec};
use std::fmt::Write;

/// The group of the cells.
pub const FLUID_GROUP: &str = "fluid";

/// The largest cell count written. The case is held in memory as text, which takes a few hundred
/// bytes per cell, so a larger mesh is refused before anything is built.
pub const MAX_CELLS: u64 = 8_000_000;

/// The Gmsh element type of a four-node quadrilateral.
const QUAD: u32 = 3;

/// The Gmsh element type of an eight-node hexahedron.
const HEXAHEDRON: u32 = 5;

fn invalid(reason: String) -> CompileFailure {
    CompileFailure::Invalid { reason }
}

/// The block layout of a study: a box or the shape the study names.
pub fn layout_of(study: &ResolvedStudy) -> Result<Layout, CompileFailure> {
    if study.mesh.cells_z != 1 {
        return Err(invalid(
            "the mesh needs exactly one cell through z for a two-dimensional case".to_owned(),
        ));
    }
    let domain = DomainSpec {
        length_x: study.domain.length_x,
        length_y: study.domain.length_y,
        length_z: study.domain.length_z,
    };
    let shape = match study.domain.shape {
        None => ShapeSpec::Box,
        Some(ResolvedShape::BackwardStep {
            step_height,
            upstream_length,
            startup_length,
        }) => ShapeSpec::BackwardStep {
            step_height,
            upstream_length,
            startup_length,
        },
        Some(ResolvedShape::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        }) => ShapeSpec::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        },
    };
    let grading = study.mesh.grading;
    let request = MeshRequest {
        cells_x: study.mesh.cells_x,
        cells_y: study.mesh.cells_y,
        cells_z: study.mesh.cells_z,
        level: study.mesh.level,
        ratio_x: grading.and_then(|g| g.x),
        ratio_y: grading.and_then(|g| g.y),
    };
    let layout = cfdcraft_shapes::layout(&domain, &shape, &request)
        .map_err(|error| invalid(error.to_string()))?;
    if layout.cell_count > MAX_CELLS {
        return Err(invalid(format!(
            "the mesh has {} cells, above the limit of {MAX_CELLS}",
            layout.cell_count
        )));
    }
    Ok(layout)
}

/// The two-dimensional mesh of a study.
pub fn mesh_2d(study: &ResolvedStudy) -> Result<Mesh2d, CompileFailure> {
    let layout = layout_of(study)?;
    build(&layout).map_err(|error: BlockMeshError| invalid(error.to_string()))
}

/// The mesh of a study as Gmsh ASCII text.
pub fn gmsh_text(study: &ResolvedStudy) -> Result<String, CompileFailure> {
    let mesh = mesh_2d(study)?;
    let layer = u32::try_from(mesh.nodes.len())
        .map_err(|_| invalid("the mesh has too many nodes".to_owned()))?;
    let thickness = number(mesh.thickness)?;
    let mut text = String::new();
    // Writing into a string cannot fail, so the results of `write!` below are dropped.
    text.push_str("$MeshFormat\n2.2 0 8\n$EndMeshFormat\n");

    // The parts of the boundary the mesh has, in the order of their groups.
    let groups: Vec<(&str, &[[u32; 2]])> = SIDE_ORDER
        .iter()
        .filter_map(|side| {
            let name = side_group(*side)?;
            let found = mesh.boundary.iter().find(|group| group.side == *side)?;
            Some((name, found.edges.as_slice()))
        })
        .collect();
    let z_group = groups.len() + 1;
    let fluid_group = groups.len() + 2;
    let _ = writeln!(text, "$PhysicalNames\n{}", groups.len() + 2);
    for (index, (name, _)) in groups.iter().enumerate() {
        let _ = writeln!(text, "2 {} \"{name}\"", index + 1);
    }
    let _ = writeln!(text, "2 {z_group} \"{Z_FACES_GROUP}\"");
    let _ = writeln!(text, "3 {fluid_group} \"{FLUID_GROUP}\"");
    let _ = writeln!(text, "$EndPhysicalNames");

    let _ = writeln!(text, "$Nodes\n{}", 2 * mesh.nodes.len());
    for (base, z) in [(0, "0"), (layer, thickness.as_str())] {
        for (index, node) in mesh.nodes.iter().enumerate() {
            let id = base + u32::try_from(index).unwrap_or(u32::MAX) + 1;
            let _ = writeln!(text, "{id} {} {} {z}", number(node[0])?, number(node[1])?);
        }
    }
    let _ = writeln!(text, "$EndNodes");

    let boundary_quads: usize = groups.iter().map(|(_, edges)| edges.len()).sum();
    let total = boundary_quads + 2 * mesh.quads.len() + mesh.quads.len();
    let _ = writeln!(text, "$Elements\n{total}");
    let mut id = 0_u64;
    let mut element = |text: &mut String, kind: u32, group: usize, nodes: &[u32]| {
        id += 1;
        let _ = write!(text, "{id} {kind} 2 {group} {group}");
        for node in nodes {
            let _ = write!(text, " {}", node + 1);
        }
        text.push('\n');
    };
    for (index, (_, edges)) in groups.iter().enumerate() {
        for [a, b] in *edges {
            // The cell is on the left of the edge, so this order has the outward normal.
            element(&mut text, QUAD, index + 1, &[*a, *b, b + layer, a + layer]);
        }
    }
    for q in &mesh.quads {
        element(&mut text, QUAD, z_group, &[q[3], q[2], q[1], q[0]]);
    }
    for q in &mesh.quads {
        let top = q.map(|node| node + layer);
        element(&mut text, QUAD, z_group, &top);
    }
    for q in &mesh.quads {
        let top = q.map(|node| node + layer);
        let nodes = [q[0], q[1], q[2], q[3], top[0], top[1], top[2], top[3]];
        element(&mut text, HEXAHEDRON, fluid_group, &nodes);
    }
    let _ = writeln!(text, "$EndElements");
    Ok(text)
}
