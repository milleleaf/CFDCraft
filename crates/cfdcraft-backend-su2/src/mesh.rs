//! The mesh of a case in the native SU2 text format, and the table that names its boundary markers.
//!
//! Marker names come from a fixed table keyed by boundary kind and from the names the shape owns
//! (`obstacle`, `startup`), never from the author's labels, so a label cannot reach the case and
//! renaming a boundary changes no byte. The nodes and cells come from the block layout through the
//! blockmesh crate; this file only names the boundary groups and writes the text.

use cfdcraft_backend::CompileFailure;
use cfdcraft_blockmesh::Mesh2d;
use cfdcraft_model::{
    BoundaryKind, Face, ResolvedBoundary, ResolvedShape, ResolvedStudy, ResolvedThermal,
    velocity_vector,
};
use cfdcraft_shapes::{DomainSpec, Layout, MeshRequest, ShapeError, ShapeSpec, Side};
use std::{collections::BTreeSet, fmt::Write};

/// One boundary marker of the mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub name: &'static str,
    pub kind: BoundaryKind,
    /// The parts of the boundary the marker holds, in the order its elements are written.
    pub sides: Vec<Side>,
    /// The velocity vector of an inlet or a moving wall.
    pub velocity: Option<[f64; 3]>,
    /// The fixed temperature of an isothermal wall, in kelvin.
    pub temperature: Option<f64>,
}

const KIND_ORDER: [(BoundaryKind, &str); 6] = [
    (BoundaryKind::VelocityInlet, "inlet"),
    (BoundaryKind::PressureOutlet, "outlet"),
    (BoundaryKind::MovingWall, "movingWall"),
    (BoundaryKind::NoSlipWall, "fixedWalls"),
    (BoundaryKind::SlipWall, "slipWalls"),
    (BoundaryKind::Symmetry, "symmetry"),
];

/// The names of the walls held at a fixed temperature, by rank in ascending temperature.
const ISOTHERMAL: [&str; 4] = ["isothermal1", "isothermal2", "isothermal3", "isothermal4"];
/// The marker of the cylinder surface.
const OBSTACLE: &str = "obstacle";
/// The symmetry marker of the start-up section of a step.
const STARTUP: &str = "startup";

pub fn invalid(reason: &str) -> CompileFailure {
    CompileFailure::Invalid {
        reason: reason.to_owned(),
    }
}

fn fixed_temperature(b: &ResolvedBoundary) -> Option<f64> {
    match b.thermal {
        Some(ResolvedThermal::FixedTemperature { value }) => Some(value),
        _ => None,
    }
}

/// The distinct fixed wall temperatures of the study, ascending.
fn wall_temperatures(s: &ResolvedStudy) -> Vec<f64> {
    let mut found: Vec<f64> = s
        .boundary_conditions
        .values()
        .filter_map(fixed_temperature)
        .collect();
    found.sort_by(f64::total_cmp);
    found.dedup();
    found
}

fn side(face: Face) -> Side {
    match face {
        Face::XMin => Side::XMin,
        Face::XMax => Side::XMax,
        Face::YMin => Side::YMin,
        Face::YMax => Side::YMax,
        Face::Obstacle => Side::Obstacle,
    }
}

/// The block layout of a resolved study.
pub fn layout_of(s: &ResolvedStudy) -> Result<Layout, CompileFailure> {
    let domain = DomainSpec {
        length_x: s.domain.length_x,
        length_y: s.domain.length_y,
        length_z: s.domain.length_z,
    };
    let shape = match s.domain.shape {
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
    let ratios = s.mesh.grading.as_ref();
    let request = MeshRequest {
        cells_x: s.mesh.cells_x,
        cells_y: s.mesh.cells_y,
        cells_z: s.mesh.cells_z,
        level: s.mesh.level,
        ratio_x: ratios.and_then(|g| g.x),
        ratio_y: ratios.and_then(|g| g.y),
    };
    cfdcraft_shapes::layout(&domain, &shape, &request).map_err(|e| {
        invalid(match e {
            ShapeError::OddCells { .. } => "a graded axis needs an even cell count",
            ShapeError::BadSize { .. } => "a size or a cell count does not fit the shape",
            ShapeError::BadLevel { .. } => "the refinement level is above the largest level",
            ShapeError::BadGrading { .. } => "the growth ratio must be between 1 and 1.5",
            ShapeError::Unsupported { .. } => "the shape does not offer this mesh request",
        })
    })
}

/// The speed a boundary of `kind` must carry: finite and not zero.
fn driven_speed(kind: BoundaryKind, speed: Option<f64>) -> Result<Option<f64>, CompileFailure> {
    match (kind, speed) {
        (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall, Some(v))
            if v.is_finite() && v != 0.0 =>
        {
            Ok(Some(v))
        }
        (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall, _) => Err(invalid(
            "a finite nonzero velocity is required for this boundary kind",
        )),
        (_, Some(_)) => Err(invalid("this boundary kind carries no velocity")),
        (_, None) => Ok(None),
    }
}

/// The markers of a study in the order they are written: by kind, then the walls held at a fixed
/// temperature, the obstacle and the start-up section.
pub fn markers(s: &ResolvedStudy, layout: &Layout) -> Result<Vec<Marker>, CompileFailure> {
    let mut seen = BTreeSet::new();
    for boundary in s.boundary_conditions.values() {
        for face in &boundary.faces {
            if !seen.insert(*face) {
                return Err(invalid("a face is assigned twice"));
            }
        }
    }
    for boundary in s.boundary_conditions.values() {
        if fixed_temperature(boundary).is_none() {
            continue;
        }
        if boundary.kind != BoundaryKind::NoSlipWall {
            return Err(invalid(
                "a fixed temperature is supported on no-slip walls only",
            ));
        }
        if boundary.faces.contains(&Face::Obstacle) {
            return Err(invalid(
                "a fixed temperature is not supported on the obstacle",
            ));
        }
    }
    let temperatures = wall_temperatures(s);
    if temperatures.len() > ISOTHERMAL.len() {
        return Err(invalid(
            "at most four distinct wall temperatures are supported",
        ));
    }
    let cylinder = matches!(s.domain.shape, Some(ResolvedShape::CylinderChannel { .. }));
    if seen.contains(&Face::Obstacle) && !cylinder {
        return Err(invalid("the obstacle face needs a cylinder channel"));
    }
    if seen.len() != 4 + usize::from(cylinder) {
        return Err(invalid(
            "every face of the shape must belong to exactly one boundary",
        ));
    }
    let mut out = Vec::new();
    for (kind, name) in KIND_ORDER {
        let mut faces = Vec::new();
        let mut velocity: Option<[f64; 3]> = None;
        for boundary in s
            .boundary_conditions
            .values()
            .filter(|b| b.kind == kind && fixed_temperature(b).is_none())
        {
            let speed = driven_speed(kind, boundary.velocity)?;
            for face in boundary.faces.iter().filter(|f| **f != Face::Obstacle) {
                if let Some(speed) = speed {
                    let vector = velocity_vector(kind, *face, speed);
                    if velocity.is_some_and(|v| v != vector) {
                        return Err(invalid(
                            "boundaries of one kind must impose the same velocity vector",
                        ));
                    }
                    velocity = Some(vector);
                }
                faces.push(*face);
            }
        }
        if !faces.is_empty() {
            faces.sort();
            out.push(Marker {
                name,
                kind,
                sides: faces.into_iter().map(side).collect(),
                velocity,
                temperature: None,
            });
        }
    }
    for (name, temperature) in ISOTHERMAL.iter().zip(&temperatures) {
        let mut faces: Vec<Face> = s
            .boundary_conditions
            .values()
            .filter(|b| fixed_temperature(b) == Some(*temperature))
            .flat_map(|b| b.faces.iter().copied())
            .collect();
        faces.sort();
        out.push(Marker {
            name,
            kind: BoundaryKind::NoSlipWall,
            sides: faces.into_iter().map(side).collect(),
            velocity: None,
            temperature: Some(*temperature),
        });
    }
    if let Some(b) = s
        .boundary_conditions
        .values()
        .find(|b| b.faces.contains(&Face::Obstacle))
    {
        if !matches!(b.kind, BoundaryKind::NoSlipWall | BoundaryKind::SlipWall) {
            return Err(invalid("the obstacle must be a wall without a velocity"));
        }
        out.push(Marker {
            name: OBSTACLE,
            kind: b.kind,
            sides: vec![Side::Obstacle],
            velocity: None,
            temperature: None,
        });
    }
    if layout.quads.iter().any(|q| q.side == Side::Startup) {
        out.push(Marker {
            name: STARTUP,
            kind: BoundaryKind::Symmetry,
            sides: vec![Side::Startup],
            velocity: None,
            temperature: None,
        });
    }
    Ok(out)
}

/// The mesh of a layout, with every failure as a compile failure.
pub fn mesh_of(layout: &Layout) -> Result<Mesh2d, CompileFailure> {
    cfdcraft_blockmesh::build(layout).map_err(|e| CompileFailure::Invalid {
        reason: e.to_string(),
    })
}

/// The edges of a marker, in the order its sides are listed.
pub fn marker_edges<'a>(
    mesh: &'a Mesh2d,
    marker: &Marker,
) -> Result<Vec<&'a [u32; 2]>, CompileFailure> {
    let mut edges = Vec::new();
    for wanted in &marker.sides {
        let group = mesh
            .boundary
            .iter()
            .find(|g| g.side == *wanted)
            .ok_or_else(|| invalid("a boundary of the study has no edges in the mesh"))?;
        edges.extend(group.edges.iter());
    }
    if edges.is_empty() {
        return Err(invalid("a boundary of the study has no edges in the mesh"));
    }
    Ok(edges)
}

/// A coordinate with twelve decimals and no sign on zero, so the text is the same on every system.
/// The mesh text and the profile file both write coordinates this way, so a node and its profile
/// row read back as the same number.
pub fn coordinate(v: f64) -> String {
    let text = format!("{v:.12}");
    match text.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_owned(),
        _ => text,
    }
}

/// The mesh in the native format: dimension, cells, nodes, then one block per marker. Cells and
/// nodes are numbered from zero in file order.
pub fn mesh_text(mesh: &Mesh2d, markers: &[Marker]) -> Result<String, CompileFailure> {
    let mut text = String::new();
    let _ = writeln!(text, "NDIME= 2");
    let _ = writeln!(text, "NELEM= {}", mesh.quads.len());
    for (i, q) in mesh.quads.iter().enumerate() {
        let _ = writeln!(text, "9 {} {} {} {} {i}", q[0], q[1], q[2], q[3]);
    }
    let _ = writeln!(text, "NPOIN= {}", mesh.nodes.len());
    for (i, n) in mesh.nodes.iter().enumerate() {
        let _ = writeln!(text, "{} {} {i}", coordinate(n[0]), coordinate(n[1]));
    }
    let _ = writeln!(text, "NMARK= {}", markers.len());
    for marker in markers {
        let edges = marker_edges(mesh, marker)?;
        let _ = writeln!(text, "MARKER_TAG= {}", marker.name);
        let _ = writeln!(text, "MARKER_ELEMS= {}", edges.len());
        for e in edges {
            let _ = writeln!(text, "3 {} {}", e[0], e[1]);
        }
    }
    Ok(text)
}

/// The node numbers of a marker, each once, in the order they first appear along its edges.
pub fn marker_nodes(mesh: &Mesh2d, marker: &Marker) -> Result<Vec<u32>, CompileFailure> {
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::new();
    for e in marker_edges(mesh, marker)? {
        for n in e {
            if seen.insert(*n) {
                nodes.push(*n);
            }
        }
    }
    Ok(nodes)
}
