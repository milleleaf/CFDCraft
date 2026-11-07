use crate::{
    Dialect, block_mesh, control_dict, dictionary, fields,
    guard::scan_tree,
    patches::{layout_of, patch_plan_with},
    solver_plan::SolverPlan,
    transport,
};
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
    #[error("ESI case requires one cell through z for empty front/back patches")]
    NotTwoDimensional,
    #[error("resolved study is invalid: {0}")]
    Invalid(&'static str),
    #[error("raw override {label}: {reason}")]
    Raw { label: String, reason: String },
}

pub(crate) use crate::patches::{Patch, patch_plan};

/// How a case is written beyond what the study says. One process writes the case exactly as it
/// has always been written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaseOptions {
    /// Number of processes the solver runs on; at least one.
    pub np: u32,
}

impl Default for CaseOptions {
    fn default() -> Self {
        Self { np: 1 }
    }
}

/// Cells a process should have at least, so a small case never pays for decomposition.
const CELLS_PER_PROCESS: u64 = 20_000;

/// The process count for a case: as many as the cell count justifies, never above the host's
/// core cap, never below one.
pub fn np_for(cells: u64, core_cap: u32) -> u32 {
    let by_cells = u32::try_from(cells / CELLS_PER_PROCESS).unwrap_or(u32::MAX);
    by_cells.min(core_cap).max(1)
}

pub fn compile(s: &ResolvedStudy, dialect: Dialect) -> Result<CaseTree, CompileError> {
    compile_with(s, dialect, &CaseOptions::default())
}

pub fn compile_with(
    s: &ResolvedStudy,
    _dialect: Dialect,
    options: &CaseOptions,
) -> Result<CaseTree, CompileError> {
    if options.np == 0 {
        return Err(CompileError::Invalid(
            "the process count must be at least one",
        ));
    }
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
    let layout = layout_of(s)?;
    let patches = patch_plan_with(s, &layout)?;
    let plan = SolverPlan::for_study(s);
    let plain = fields::has_plain_numerics(s, &plan);
    let turbulence = fields::turbulence_inputs(s)?;
    let mut named: Vec<(&str, String)> = vec![("0/U", transport::velocity(s, &patches))];
    if plan.buoyant {
        named.push(("0/p", fields::p_calculated_field(&patches)));
    } else {
        named.push(("0/p", transport::pressure(&patches)));
    }
    named.extend(fields::buoyant_files(s, &patches));
    if let Some((model, inlet)) = &turbulence {
        named.extend(fields::turbulence_files(&patches, *model, inlet));
    }
    named.push((
        "constant/transportProperties",
        transport::transport_properties(s),
    ));
    named.extend(transport::gravity(s).map(|g| ("constant/g", g)));
    named.extend([
        (
            "constant/turbulenceProperties",
            fields::turbulence_properties_for(s),
        ),
        (
            "system/blockMeshDict",
            block_mesh::block_mesh(&layout, &patches),
        ),
        ("system/controlDict", control_dict::control_dict(s)),
        (
            "system/fvSchemes",
            if plain {
                control_dict::fv_schemes()
            } else {
                fields::fv_schemes_for(s, &plan)
            },
        ),
        (
            "system/fvSolution",
            if plain {
                control_dict::fv_solution_with(s.domain.open, &plan)
            } else {
                fields::fv_solution_for(s, &plan)
            },
        ),
    ]);
    let mut files: Vec<CaseFile> = named
        .into_iter()
        .map(|(name, text)| CaseFile {
            rel_path: name.into(),
            bytes: text.into_bytes(),
        })
        .collect();
    crate::raw::apply(&mut files, &s.raw_overrides)?;
    if options.np > 1 {
        let at = files
            .iter()
            .rposition(|f| f.rel_path.starts_with("system"))
            .map_or(files.len(), |i| i + 1);
        files.insert(
            at,
            CaseFile {
                rel_path: "system/decomposeParDict".into(),
                bytes: dictionary(
                    "decomposeParDict",
                    "dictionary",
                    &format!("numberOfSubdomains {};\nmethod scotch;\n", options.np),
                )
                .into_bytes(),
            },
        );
    }
    Ok(CaseTree { files })
}

/// The largest seed file taken from a parent run.
const SEED_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContinueError {
    #[error("the case has no system/controlDict to continue from")]
    NoControlDict,
    #[error("the control dictionary could not be edited: {reason}")]
    Edit { reason: String },
    #[error("seed file {path}: {reason}")]
    BadSeed { path: String, reason: String },
    #[error("{rule}: {file}:{line} was refused by the safety scan")]
    Guard {
        rule: &'static str,
        file: String,
        line: usize,
    },
}

fn bad_seed(path: &Path, reason: &str) -> ContinueError {
    ContinueError::BadSeed {
        path: path.display().to_string(),
        reason: reason.into(),
    }
}

/// The fields a continued case takes from its parent, in the order they are added.
const SEED_FIELDS: [&str; 3] = ["U", "p", "phi"];

/// The time folder and field rank of a seed path, when it has the strict shape `<time>/<U|p|phi>`.
fn seed_parts(path: &Path) -> Result<(&str, usize), ContinueError> {
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_str()),
            _ => return Err(bad_seed(path, "the path must be a plain relative path")),
        }
    }
    let &[Some(time), Some(field)] = parts.as_slice() else {
        return Err(bad_seed(path, "the path must be <time>/<field>"));
    };
    let plain = time.chars().all(|c| c.is_ascii_digit() || c == '.')
        && time.matches('.').count() <= 1
        && time.chars().any(|c| c.is_ascii_digit());
    if !plain || !time.parse::<f64>().is_ok_and(|t| t.is_finite() && t > 0.0) {
        return Err(bad_seed(
            path,
            "the time folder must be a number above zero",
        ));
    }
    match SEED_FIELDS.iter().position(|n| *n == field) {
        Some(rank) => Ok((time, rank)),
        None => Err(bad_seed(path, "only the fields U, p and phi are taken")),
    }
}

/// The compiled case continued from a parent run's latest time: `startFrom latestTime` set
/// through the dictionary editor, the parent's fields added under their time folder, and the whole
/// result passed through the safety scan. The input is never changed and nothing is written.
pub fn continue_case(tree: &CaseTree, seed: &[CaseFile]) -> Result<CaseTree, ContinueError> {
    let mut folder: Option<&str> = None;
    let mut ranks: Vec<(usize, &CaseFile)> = Vec::new();
    for file in seed {
        let (time, rank) = seed_parts(&file.rel_path)?;
        if file.bytes.len() > SEED_MAX_BYTES {
            return Err(bad_seed(&file.rel_path, "the file is larger than 16 MiB"));
        }
        if folder.is_some_and(|f| f != time) {
            return Err(bad_seed(
                &file.rel_path,
                "the seed must come from one time folder",
            ));
        }
        if ranks.iter().any(|(r, _)| *r == rank) {
            return Err(bad_seed(&file.rel_path, "the file is given twice"));
        }
        folder = Some(time);
        ranks.push((rank, file));
    }
    let Some(time) = folder else {
        return Err(bad_seed(Path::new(""), "the seed holds no files"));
    };
    let has = |rank: usize| ranks.iter().any(|(r, _)| *r == rank);
    if !has(0) || !has(1) {
        return Err(bad_seed(Path::new(time), "the seed needs both U and p"));
    }
    let mut next = tree.clone();
    let control = next
        .files
        .iter_mut()
        .find(|f| f.rel_path == Path::new("system/controlDict"))
        .ok_or(ContinueError::NoControlDict)?;
    let text = std::str::from_utf8(&control.bytes).map_err(|_| ContinueError::Edit {
        reason: "system/controlDict is not text".into(),
    })?;
    let edit = |e: cfdcraft_dictedit::EditError| ContinueError::Edit {
        reason: e.to_string(),
    };
    let started = cfdcraft_dictedit::set_entry(text, "startFrom", "latestTime").map_err(edit)?;
    // The write interval is counted from the time the run starts at, whether in steps or in time,
    // so a run continued from a stop between two writes would never write its end time. One write
    // interval as long as the time left puts the first write exactly at the end time.
    let end = cfdcraft_dictedit::get_entry(&started, "endTime")
        .map_err(edit)?
        .and_then(|v| v.trim().parse::<f64>().ok());
    let from = time.parse::<f64>().ok();
    let edited = match end.zip(from) {
        Some((end, from)) if end > from && end.is_finite() => {
            let left = format!("{:?}", end - from);
            let timed =
                cfdcraft_dictedit::set_entry(&started, "writeControl", "runTime").map_err(edit)?;
            cfdcraft_dictedit::set_entry(&timed, "writeInterval", &left).map_err(edit)?
        }
        _ => started,
    };
    control.bytes = edited.into_bytes();
    ranks.sort_by_key(|(rank, _)| *rank);
    for (_, file) in ranks {
        if next.files.iter().any(|f| f.rel_path == file.rel_path) {
            return Err(bad_seed(&file.rel_path, "the case already has this file"));
        }
        next.files.push(file.clone());
    }
    if let Some(r) = scan_tree(&next).into_iter().next() {
        return Err(ContinueError::Guard {
            rule: r.rule.code(),
            file: r.file,
            line: r.line,
        });
    }
    Ok(next)
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
