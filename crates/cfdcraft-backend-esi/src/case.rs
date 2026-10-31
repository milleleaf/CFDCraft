use crate::{Dialect, block_mesh, control_dict, transport};
use cfdcraft_model::ResolvedStudy;
use std::{
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
    #[error("ESI cavity requires one cell through z for empty front/back patches")]
    NotTwoDimensional,
    #[error("resolved study is invalid: {0}")]
    Invalid(&'static str),
}

pub fn compile(s: &ResolvedStudy, _dialect: Dialect) -> Result<CaseTree, CompileError> {
    if s.mesh.cells_z != 1 {
        return Err(CompileError::NotTwoDimensional);
    }
    if s.schema_version != 0 {
        return Err(CompileError::Invalid("schema version"));
    }
    for value in [
        s.domain.length_x,
        s.domain.length_y,
        s.domain.length_z,
        s.nu,
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
    if !s.lid_velocity.is_finite()
        || s.lid_velocity == 0.0
        || s.mesh.cells_x == 0
        || s.mesh.cells_y == 0
        || s.monitors.centreline_u_sample_count < 2
    {
        return Err(CompileError::Invalid(
            "velocity, cell count or sample count",
        ));
    }
    let files = [
        ("0/U", transport::velocity(s)),
        ("0/p", transport::pressure()),
        (
            "constant/transportProperties",
            transport::transport_properties(s),
        ),
        (
            "constant/turbulenceProperties",
            transport::turbulence_properties(),
        ),
        ("system/blockMeshDict", block_mesh::block_mesh(s)),
        ("system/controlDict", control_dict::control_dict(s)),
        ("system/fvSchemes", control_dict::fv_schemes()),
        ("system/fvSolution", control_dict::fv_solution()),
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
