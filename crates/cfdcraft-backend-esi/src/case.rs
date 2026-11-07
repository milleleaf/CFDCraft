use crate::{Dialect, block_mesh, control_dict, transport};
use cfdcraft_model::{BoundaryKind, Face, ResolvedStudy, resolve::velocity_vector};
use std::{
    collections::BTreeSet,
    io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseFile {
    pub rel_path: PathBuf,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseTree {
    pub files: Vec<CaseFile>,
}
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("ESI case requires one cell through z for empty front/back patches")]
    NotTwoDimensional,
    #[error("resolved study is invalid: {0}")]
    Invalid(&'static str),
}

/// One written OpenFOAM patch. Names come from a fixed table keyed by boundary kind, never from
/// the author's label, so a label cannot reach the case and renaming changes no byte.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Patch {
    pub name: &'static str,
    /// `None` is the front/back `empty` patch.
    pub kind: Option<BoundaryKind>,
    pub faces: Vec<Face>,
    pub velocity: Option<[f64; 3]>,
}

impl Patch {
    /// The mesh patch type written in `blockMeshDict`.
    pub fn mesh_type(&self) -> &'static str {
        match self.kind {
            None => "empty",
            Some(BoundaryKind::VelocityInlet | BoundaryKind::PressureOutlet) => "patch",
            Some(BoundaryKind::MovingWall | BoundaryKind::NoSlipWall | BoundaryKind::SlipWall) => {
                "wall"
            }
            Some(BoundaryKind::Symmetry) => "symmetryPlane",
        }
    }

    /// Vertex tuples of the faces, in canonical face order, as `blockMeshDict` lists them.
    pub fn faces_text(&self) -> String {
        match self.kind {
            None => "(0 3 2 1) (4 5 6 7)".into(),
            Some(_) => self
                .faces
                .iter()
                .map(|f| match f {
                    Face::XMin => "(0 4 7 3)",
                    Face::XMax => "(1 2 6 5)",
                    Face::YMin => "(0 1 5 4)",
                    Face::YMax => "(3 7 6 2)",
                })
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

const KIND_ORDER: [(BoundaryKind, &str); 6] = [
    (BoundaryKind::VelocityInlet, "inlet"),
    (BoundaryKind::PressureOutlet, "outlet"),
    (BoundaryKind::MovingWall, "movingWall"),
    (BoundaryKind::NoSlipWall, "fixedWalls"),
    (BoundaryKind::SlipWall, "slipWalls"),
    (BoundaryKind::Symmetry, "symmetry"),
];

/// Group the resolved boundaries by kind into the fixed patch order, ending with `frontAndBack`.
pub(crate) fn patch_plan(s: &ResolvedStudy) -> Result<Vec<Patch>, CompileError> {
    let mut seen = BTreeSet::new();
    for boundary in s.boundary_conditions.values() {
        for face in &boundary.faces {
            if !seen.insert(*face) {
                return Err(CompileError::Invalid("a box face is assigned twice"));
            }
        }
    }
    if seen.len() != 4 {
        return Err(CompileError::Invalid(
            "every box face must belong to exactly one boundary",
        ));
    }
    let mut patches = Vec::new();
    for (kind, name) in KIND_ORDER {
        let mut faces = Vec::new();
        let mut velocity: Option<[f64; 3]> = None;
        for boundary in s.boundary_conditions.values().filter(|b| b.kind == kind) {
            let speed = match (kind, boundary.velocity) {
                (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall, Some(v))
                    if v.is_finite() && v != 0.0 =>
                {
                    Some(v)
                }
                (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall, _) => {
                    return Err(CompileError::Invalid(
                        "a finite nonzero velocity is required for this boundary kind",
                    ));
                }
                (_, Some(_)) => {
                    return Err(CompileError::Invalid(
                        "this boundary kind carries no velocity",
                    ));
                }
                (_, None) => None,
            };
            for face in &boundary.faces {
                if let Some(speed) = speed {
                    let vector = velocity_vector(kind, *face, speed);
                    if velocity.is_some_and(|v| v != vector) {
                        return Err(CompileError::Invalid(
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
            patches.push(Patch {
                name,
                kind: Some(kind),
                faces,
                velocity,
            });
        }
    }
    patches.push(Patch {
        name: "frontAndBack",
        kind: None,
        faces: Vec::new(),
        velocity: None,
    });
    Ok(patches)
}

pub fn compile(s: &ResolvedStudy, _dialect: Dialect) -> Result<CaseTree, CompileError> {
    if s.mesh.cells_z != 1 {
        return Err(CompileError::NotTwoDimensional);
    }
    if s.schema_version != 1 {
        return Err(CompileError::Invalid("schema version"));
    }
    for value in [
        s.domain.length_x,
        s.domain.length_y,
        s.domain.length_z,
        s.fluid.kinematic_viscosity,
        s.solver.end_time,
        s.solver.write_interval,
        s.solver.convergence.p,
        s.solver.convergence.u,
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(CompileError::Invalid(
                "positive finite physical quantities required",
            ));
        }
    }
    if s.mesh.cells_x == 0 || s.mesh.cells_y == 0 || s.monitors.centreline_u_sample_count < 2 {
        return Err(CompileError::Invalid("cell count or sample count"));
    }
    let patches = patch_plan(s)?;
    let files = [
        ("0/U", transport::velocity(&patches)),
        ("0/p", transport::pressure(&patches)),
        (
            "constant/transportProperties",
            transport::transport_properties(s),
        ),
        (
            "constant/turbulenceProperties",
            transport::turbulence_properties(),
        ),
        ("system/blockMeshDict", block_mesh::block_mesh(s, &patches)),
        ("system/controlDict", control_dict::control_dict(s)),
        ("system/fvSchemes", control_dict::fv_schemes()),
        (
            "system/fvSolution",
            control_dict::fv_solution(s.domain.open),
        ),
    ]
    .into_iter()
    .map(|(name, text)| CaseFile {
        rel_path: name.into(),
        bytes: text.into_bytes(),
    })
    .collect();
    Ok(CaseTree { files })
}

pub fn write_to_disk(tree: &CaseTree, root: &Path) -> io::Result<()> {
    for file in &tree.files {
        if file.rel_path.as_os_str().is_empty()
            || !file
                .rel_path
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "case file must have a safe relative path",
            ));
        }
        let path = root.join(&file.rel_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &file.bytes)?;
    }
    Ok(())
}
