//! Reads a written SU2 case back and compares what it says with the study.
//!
//! The reader parses the bytes of the case itself and shares no writing code: it splits the
//! configuration into keys and values, reads the mesh text and the profile file line by line, and
//! recovers facts such as the viscosity, the iteration count, which marker is the lid, and how many
//! cells there are. Each fact is compared with what the study requires. A value that is missing, a
//! value that differs and a file that cannot be understood are all errors.
//!
//! The study's monitors (probes, lines, surfaces) are not facts of the case: SU2 has no object for
//! them, and they are read from the solution file after a run.

use crate::{
    config::{TIME_MARCHING, initial_temperature, model_of, reference_speed, speed_and_direction},
    inlet::{COLUMNS, COLUMNS_TURBULENCE, inlet_extent, parabolic_requested, profile_speed},
    markers::{CONFIG_FILE, INLET_FILE, MESH_FILE, STANDARD_GRAVITY},
    mesh::{Marker, layout_of, markers},
};
use cfdcraft_backend::CaseTree;
use cfdcraft_model::{BoundaryKind, Diagnostic, ResolvedStudy, TurbulenceModel};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const NOT_RUNNABLE: &str = "The case does not match the study, so it must not be run. Compile it again; if this repeats, the writer has a fault.";

/// A file of the case that cannot be read back with confidence.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReadError {
    #[error("the case has no {file}")]
    Missing { file: &'static str },
    #[error("line {line} of {file} is not a key and a value")]
    NotAnEntry { file: &'static str, line: usize },
    #[error("line {line} of {file}: {what}")]
    Bad {
        file: &'static str,
        line: usize,
        what: &'static str,
    },
}

/// A value read from, or required of, the case.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Num(f64),
    Count(u64),
    Text(String),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Num(v) => write!(f, "{v}"),
            Self::Count(n) => write!(f, "{n}"),
            Self::Text(t) => f.write_str(t),
        }
    }
}

/// A fact the case must hold, derived from the study alone.
struct Want {
    key: String,
    setting: String,
    pointer: String,
    value: Value,
    /// Relative tolerance of a number; zero means exact.
    tolerance: f64,
}

fn same(want: &Want, found: &Value) -> bool {
    match (&want.value, found) {
        (Value::Num(a), Value::Num(b)) => {
            let fold = |x: f64| if x == 0.0 { 0.0 } else { x };
            if want.tolerance > 0.0 {
                (a - b).abs() <= want.tolerance * a.abs().max(b.abs())
            } else {
                fold(*a).to_bits() == fold(*b).to_bits()
            }
        }
        (Value::Count(a), Value::Count(b)) => a == b,
        (Value::Text(a), Value::Text(b)) => a == b,
        _ => false,
    }
}

fn text(file: &[u8]) -> String {
    String::from_utf8_lossy(file).into_owned()
}

fn file<'a>(tree: &'a CaseTree, name: &str) -> Option<&'a [u8]> {
    tree.files
        .iter()
        .find(|f| f.rel_path.to_str() == Some(name))
        .map(|f| f.bytes.as_slice())
}

/// The words of a value: its parentheses and commas count as spaces.
fn words(value: &str) -> Vec<String> {
    value
        .replace(['(', ')', ','], " ")
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn parse_config(source: &str) -> Result<BTreeMap<String, String>, ReadError> {
    let mut out = BTreeMap::new();
    for (i, line) in source.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(ReadError::NotAnEntry {
            file: CONFIG_FILE,
            line: i + 1,
        })?;
        if out
            .insert(key.trim().to_owned(), value.trim().to_owned())
            .is_some()
        {
            return Err(ReadError::Bad {
                file: CONFIG_FILE,
                line: i + 1,
                what: "a key appears twice",
            });
        }
    }
    Ok(out)
}

/// One marker as the mesh text holds it.
struct MeshMarker {
    tag: String,
    declared: u64,
    edges: Vec<[u64; 2]>,
}

/// What the mesh text holds.
struct MeshRead {
    dimension: Option<u64>,
    cells_declared: Option<u64>,
    cells: u64,
    cell_nodes_max: u64,
    points_declared: Option<u64>,
    points: Vec<[f64; 2]>,
    markers: Vec<MeshMarker>,
}

impl MeshRead {
    /// The first way the declared counts and the lines read disagree, or `ok`.
    fn consistency(&self) -> String {
        if self.cells_declared != Some(self.cells) {
            return "the number of cells declared is not the number of cell lines".into();
        }
        let points = self.points.len() as u64;
        if self.points_declared != Some(points) {
            return "the number of nodes declared is not the number of node lines".into();
        }
        if self.cells > 0 && self.cell_nodes_max >= points {
            return "a cell names a node that does not exist".into();
        }
        for m in &self.markers {
            if m.declared != m.edges.len() as u64 {
                return format!("marker {} declares a different number of edges", m.tag);
            }
            if m.edges.iter().flatten().any(|n| *n >= points) {
                return format!("marker {} names a node that does not exist", m.tag);
            }
        }
        "ok".into()
    }

    fn marker(&self, tag: &str) -> Option<&MeshMarker> {
        self.markers.iter().find(|m| m.tag == tag)
    }
}

enum Section {
    Other,
    Cells,
    Points,
    Edges,
}

fn parse_mesh(source: &str) -> Result<MeshRead, ReadError> {
    let bad = |line: usize, what: &'static str| ReadError::Bad {
        file: MESH_FILE,
        line,
        what,
    };
    let mut mesh = MeshRead {
        dimension: None,
        cells_declared: None,
        cells: 0,
        cell_nodes_max: 0,
        points_declared: None,
        points: Vec::new(),
        markers: Vec::new(),
    };
    let mut section = Section::Other;
    for (i, line) in source.lines().enumerate() {
        let n = i + 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let count = value.trim().parse::<u64>().ok();
            match key.trim() {
                "NDIME" => mesh.dimension = count,
                "NELEM" => {
                    mesh.cells_declared = count;
                    section = Section::Cells;
                    continue;
                }
                "NPOIN" => {
                    mesh.points_declared = count;
                    section = Section::Points;
                    continue;
                }
                "NMARK" => {}
                "MARKER_TAG" => mesh.markers.push(MeshMarker {
                    tag: value.trim().to_owned(),
                    declared: 0,
                    edges: Vec::new(),
                }),
                "MARKER_ELEMS" => {
                    let last = mesh
                        .markers
                        .last_mut()
                        .ok_or_else(|| bad(n, "marker elements come before a marker tag"))?;
                    last.declared = count.ok_or_else(|| bad(n, "a count is not a number"))?;
                    section = Section::Edges;
                    continue;
                }
                _ => return Err(bad(n, "an unknown keyword")),
            }
            section = Section::Other;
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        match section {
            Section::Cells => {
                let ids: Vec<u64> = tokens
                    .iter()
                    .skip(1)
                    .take(4)
                    .filter_map(|t| t.parse().ok())
                    .collect();
                if tokens.first() != Some(&"9") || ids.len() != 4 {
                    return Err(bad(n, "a cell line is not a quadrilateral"));
                }
                mesh.cells += 1;
                mesh.cell_nodes_max = ids.iter().copied().fold(mesh.cell_nodes_max, u64::max);
            }
            Section::Points => {
                let xy: Vec<f64> = tokens
                    .iter()
                    .take(2)
                    .filter_map(|t| t.parse().ok())
                    .collect();
                let &[x, y] = xy.as_slice() else {
                    return Err(bad(n, "a node line has no two coordinates"));
                };
                mesh.points.push([x, y]);
            }
            Section::Edges => {
                let ids: Vec<u64> = tokens
                    .iter()
                    .skip(1)
                    .filter_map(|t| t.parse().ok())
                    .collect();
                let (Some(&"3"), &[a, b]) = (tokens.first(), ids.as_slice()) else {
                    return Err(bad(n, "an edge line is not a line element"));
                };
                let last = mesh
                    .markers
                    .last_mut()
                    .ok_or_else(|| bad(n, "an edge comes before a marker tag"))?;
                last.edges.push([a, b]);
            }
            Section::Other => return Err(bad(n, "a line outside any section")),
        }
    }
    Ok(mesh)
}

/// The profile file as it is written.
struct ProfileRead {
    tag: String,
    rows_declared: Option<u64>,
    columns_declared: Option<u64>,
    header: String,
    rows: Vec<Vec<f64>>,
}

fn parse_profile(source: &str) -> Result<ProfileRead, ReadError> {
    let bad = |line: usize, what: &'static str| ReadError::Bad {
        file: INLET_FILE,
        line,
        what,
    };
    let mut profile = ProfileRead {
        tag: String::new(),
        rows_declared: None,
        columns_declared: None,
        header: String::new(),
        rows: Vec::new(),
    };
    for (i, line) in source.lines().enumerate() {
        let n = i + 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            profile.header = line.to_owned();
        } else if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            match key.trim() {
                "NMARK" => {}
                "MARKER_TAG" => profile.tag = value.to_owned(),
                "NROW" => profile.rows_declared = value.parse().ok(),
                "NCOL" => profile.columns_declared = value.parse().ok(),
                _ => return Err(bad(n, "an unknown keyword")),
            }
        } else {
            let row: Vec<f64> = line
                .split_whitespace()
                .map(|t| t.parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| bad(n, "a row holds a word that is not a number"))?;
            profile.rows.push(row);
        }
    }
    Ok(profile)
}

/// What the profile file says about the inlet, as one word: `parabolic` when it holds the
/// polynomial at every node of the inlet marker, else the first thing that is wrong.
fn profile_status(
    study: &ResolvedStudy,
    inlet: &Marker,
    profile: &ProfileRead,
    mesh: &MeshRead,
) -> String {
    if profile.tag != inlet.name {
        return format!("the profile names marker {}", profile.tag);
    }
    let Some(marker) = mesh.marker(inlet.name) else {
        return "the inlet marker is not in the mesh".into();
    };
    let nodes: BTreeSet<u64> = marker.edges.iter().flatten().copied().collect();
    let positions: BTreeSet<(u64, u64)> = nodes
        .iter()
        .filter_map(|n| mesh.points.get(*n as usize))
        .map(|p| (p[0].to_bits(), p[1].to_bits()))
        .collect();
    let turbulent = model_of(study) == TurbulenceModel::KOmegaSst;
    let columns = if turbulent { 8 } else { 6 };
    let expected_header = if turbulent {
        format!("{COLUMNS}{COLUMNS_TURBULENCE}")
    } else {
        COLUMNS.to_owned()
    };
    if profile.header != expected_header || profile.columns_declared != Some(columns) {
        return "the profile columns are not the ones the solver reads".into();
    }
    if profile.rows_declared != Some(profile.rows.len() as u64)
        || profile.rows.len() != nodes.len()
        || positions.len() != nodes.len()
    {
        return "the profile does not have one row for each node of the inlet".into();
    }
    let (mean, direction) = speed_and_direction(inlet.velocity.unwrap_or([0.0; 3]));
    let (origin, height) = inlet_extent(study);
    for row in &profile.rows {
        let &[x, y, _, v, nx, ny, ..] = row.as_slice() else {
            return "a profile row is too short".into();
        };
        if row.len() != columns as usize || !positions.contains(&(x.to_bits(), y.to_bits())) {
            return "a profile row is not at a node of the inlet".into();
        }
        let want = profile_speed(mean, origin, height, y).max(0.0);
        if (v - want).abs() > 1e-9 * mean.max(1e-300) || nx != direction[0] || ny != direction[1] {
            return "a profile row does not follow the parabola".into();
        }
    }
    "parabolic".into()
}

fn want(
    out: &mut Vec<Want>,
    key: &str,
    setting: &str,
    pointer: &str,
    value: Value,
    tolerance: f64,
) {
    out.push(Want {
        key: key.to_owned(),
        setting: setting.to_owned(),
        pointer: pointer.to_owned(),
        value,
        tolerance,
    });
}

fn text_value(t: &str) -> Value {
    Value::Text(t.to_owned())
}

/// The kind of a marker as the configuration keys that name it.
fn signature(m: &Marker) -> &'static str {
    match (m.kind, m.temperature) {
        (BoundaryKind::VelocityInlet, _) => "MARKER_INLET",
        (BoundaryKind::PressureOutlet, _) => "MARKER_OUTLET",
        (BoundaryKind::MovingWall, _) => "MARKER_HEATFLUX+MARKER_MOVING",
        (BoundaryKind::NoSlipWall, Some(_)) => "MARKER_ISOTHERMAL",
        (BoundaryKind::NoSlipWall, None) => "MARKER_HEATFLUX",
        (BoundaryKind::SlipWall, _) => "MARKER_EULER",
        (BoundaryKind::Symmetry, _) => "MARKER_SYM",
    }
}

/// How many whole numbers of seconds or iterations a quantity is, as the writer counts them.
fn count_of(v: f64) -> Value {
    Value::Count(v.round().max(0.0) as u64)
}

/// The facts the case must hold for a study, sorted by key.
fn expected(study: &ResolvedStudy, list: &[Marker], cells: u64) -> Vec<Want> {
    let mut out = Vec::new();
    let physics = study.physics.as_ref();
    let sst = model_of(study) == TurbulenceModel::KOmegaSst;
    let buoyancy = physics.and_then(|p| p.buoyancy);
    let step = study.effective_delta_t();
    want(
        &mut out,
        "solver",
        "solver kind",
        "/physics/turbulence/model",
        text_value(if sst { "INC_RANS" } else { "INC_NAVIER_STOKES" }),
        0.0,
    );
    want(
        &mut out,
        "turbulence",
        "turbulence model",
        "/physics/turbulence/model",
        text_value(if sst { "SST" } else { "NONE" }),
        0.0,
    );
    want(
        &mut out,
        "density_model",
        "density model",
        "/physics/buoyancy",
        text_value(if buoyancy.is_some() {
            "BOUSSINESQ"
        } else {
            "CONSTANT"
        }),
        0.0,
    );
    want(
        &mut out,
        "energy",
        "energy equation",
        "/physics/buoyancy",
        text_value(if buoyancy.is_some() { "YES" } else { "NO" }),
        0.0,
    );
    want(
        &mut out,
        "viscosity",
        "viscosity",
        "/fluid/kinematic_viscosity",
        Value::Num(study.fluid.kinematic_viscosity),
        0.0,
    );
    want(&mut out, "density", "density", "", Value::Num(1.0), 0.0);
    let start = if sst {
        reference_speed(study).unwrap_or(0.0)
    } else {
        0.0
    };
    want(
        &mut out,
        "start_velocity",
        "start velocity",
        "",
        Value::Num(start),
        0.0,
    );
    let write = study.solver.write_interval;
    match step {
        None => {
            want(
                &mut out,
                "time_marching",
                "time marching",
                "/physics/time",
                text_value("steady"),
                0.0,
            );
            want(
                &mut out,
                "iterations",
                "iteration count",
                "/solver/end_time",
                count_of(study.solver.end_time),
                0.0,
            );
            want(
                &mut out,
                "write_interval",
                "write interval",
                "/solver/write_interval",
                count_of(write),
                0.0,
            );
            let limit = study.solver.convergence.p.min(study.solver.convergence.u);
            let target = format!("{:.4}", limit.log10()).parse().unwrap_or(f64::NAN);
            want(
                &mut out,
                "residual_target",
                "convergence limit",
                "/solver/convergence",
                Value::Num(target),
                0.0,
            );
        }
        Some(dt) => {
            let end = study.solver.end_time;
            want(
                &mut out,
                "time_marching",
                "time marching",
                "/physics/time",
                text_value(TIME_MARCHING),
                0.0,
            );
            want(
                &mut out,
                "time_step",
                "time step",
                "/physics/time",
                Value::Num(dt),
                0.0,
            );
            want(
                &mut out,
                "end_time",
                "end time",
                "/solver/end_time",
                Value::Num(end),
                0.0,
            );
            want(
                &mut out,
                "time_steps",
                "number of time steps",
                "/solver/end_time",
                count_of(end / dt),
                0.0,
            );
            want(
                &mut out,
                "write_interval",
                "write interval",
                "/solver/write_interval",
                count_of((write / dt).max(1.0)),
                0.0,
            );
        }
    }
    if let Some(b) = buoyancy {
        let temperature = initial_temperature(study);
        want(
            &mut out,
            "reference_temperature",
            "buoyancy reference temperature",
            "/physics/buoyancy/reference_temperature",
            Value::Num(temperature),
            0.0,
        );
        want(
            &mut out,
            "buoyancy_product",
            "buoyancy (gravity times expansion coefficient)",
            "/physics/buoyancy",
            Value::Num(b.gravity * b.beta),
            1e-12,
        );
        want(
            &mut out,
            "prandtl",
            "buoyancy Prandtl number",
            "/physics/buoyancy/prandtl",
            Value::Num(b.prandtl),
            0.0,
        );
        want(
            &mut out,
            "turbulent_prandtl",
            "buoyancy turbulent Prandtl number",
            "/physics/buoyancy/turbulent_prandtl",
            Value::Num(b.turbulent_prandtl),
            0.0,
        );
    }
    if let Some(t) = physics.map(|p| &p.turbulence).filter(|_| sst) {
        want(
            &mut out,
            "turbulence_intensity",
            "turbulence intensity",
            "/physics/turbulence/intensity",
            Value::Num(t.intensity.unwrap_or(f64::NAN)),
            0.0,
        );
        want(
            &mut out,
            "viscosity_ratio",
            "turbulence viscosity ratio",
            "/physics/turbulence/viscosity_ratio",
            Value::Num(t.viscosity_ratio.unwrap_or(f64::NAN)),
            0.0,
        );
    }
    marker_facts(study, list, &mut out);
    let mut names: Vec<&str> = list.iter().map(|m| m.name).collect();
    names.sort_unstable();
    want(
        &mut out,
        "mesh/markers",
        "boundary marker names",
        "/boundary_conditions",
        text_value(&names.join(",")),
        0.0,
    );
    want(
        &mut out,
        "mesh/dimension",
        "mesh dimension",
        "",
        Value::Count(2),
        0.0,
    );
    want(
        &mut out,
        "mesh/cells",
        "cell count",
        "/mesh",
        Value::Count(cells),
        0.0,
    );
    want(
        &mut out,
        "mesh/consistent",
        "mesh structure",
        "/mesh",
        text_value("ok"),
        0.0,
    );
    out
}

fn marker_facts(study: &ResolvedStudy, list: &[Marker], out: &mut Vec<Want>) {
    let parabolic = parabolic_requested(study);
    want(
        out,
        "inlet/profile",
        "inlet profile",
        "/boundary_conditions",
        text_value(if parabolic { "parabolic" } else { "uniform" }),
        0.0,
    );
    for m in list {
        let key = |member: &str| format!("marker/{}/{member}", m.name);
        let pointer = "/boundary_conditions";
        want(
            out,
            &key("bc"),
            "boundary kind",
            pointer,
            text_value(signature(m)),
            0.0,
        );
        let (speed, d) = speed_and_direction(m.velocity.unwrap_or([0.0; 3]));
        match m.kind {
            BoundaryKind::VelocityInlet => {
                for (member, setting, value) in [
                    ("speed", "inlet speed", speed),
                    ("direction_x", "inlet direction", d[0]),
                    ("direction_y", "inlet direction", d[1]),
                ] {
                    want(out, &key(member), setting, pointer, Value::Num(value), 0.0);
                }
            }
            BoundaryKind::MovingWall => {
                let v = m.velocity.unwrap_or([0.0; 3]);
                for (member, value) in [("velocity_x", v[0]), ("velocity_y", v[1])] {
                    want(
                        out,
                        &key(member),
                        "moving wall speed",
                        pointer,
                        Value::Num(value),
                        0.0,
                    );
                }
            }
            _ => {}
        }
        if let Some(t) = m.temperature {
            want(
                out,
                &key("temperature"),
                "wall temperature",
                pointer,
                Value::Num(t),
                0.0,
            );
        }
    }
}

fn number(cfg: &BTreeMap<String, String>, key: &str) -> Option<Value> {
    let raw = cfg.get(key)?;
    Some(
        raw.parse::<f64>()
            .map_or_else(|_| Value::Text(raw.clone()), Value::Num),
    )
}

fn count(cfg: &BTreeMap<String, String>, key: &str) -> Option<Value> {
    let raw = cfg.get(key)?;
    Some(
        raw.parse::<u64>()
            .map_or_else(|_| Value::Text(raw.clone()), Value::Count),
    )
}

fn plain(cfg: &BTreeMap<String, String>, key: &str) -> Option<Value> {
    cfg.get(key).map(|v| Value::Text(v.clone()))
}

/// The groups of `stride` words a list key holds, each as its words.
fn groups(cfg: &BTreeMap<String, String>, key: &str, stride: usize) -> Vec<Vec<String>> {
    cfg.get(key)
        .map(|v| {
            words(v)
                .chunks_exact(stride)
                .map(<[String]>::to_vec)
                .collect()
        })
        .unwrap_or_default()
}

/// The numbers of a group, after its name.
fn group_numbers(group: &[String]) -> Vec<Value> {
    group
        .iter()
        .skip(1)
        .map(|w| {
            w.parse::<f64>()
                .map_or_else(|_| Value::Text(w.clone()), Value::Num)
        })
        .collect()
}

/// Every fact the written case holds, by key.
fn found_facts(
    cfg: &BTreeMap<String, String>,
    mesh: &MeshRead,
    profile: Option<&ProfileRead>,
    study: &ResolvedStudy,
    list: &[Marker],
) -> BTreeMap<String, Value> {
    let mut found = BTreeMap::new();
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(v) = value {
            found.insert(key.to_owned(), v);
        }
    };
    put("solver", plain(cfg, "SOLVER"));
    put("turbulence", plain(cfg, "KIND_TURB_MODEL"));
    put("density_model", plain(cfg, "INC_DENSITY_MODEL"));
    put("energy", plain(cfg, "INC_ENERGY_EQUATION"));
    put("viscosity", number(cfg, "MU_CONSTANT"));
    put("density", number(cfg, "INC_DENSITY_INIT"));
    let start = groups(cfg, "INC_VELOCITY_INIT", 3);
    put(
        "start_velocity",
        start.first().map(|g| {
            g[0].parse::<f64>()
                .map_or_else(|_| Value::Text(g[0].clone()), Value::Num)
        }),
    );
    let transient = cfg.get("TIME_DOMAIN").is_some_and(|v| v == "YES");
    put(
        "time_marching",
        if transient {
            plain(cfg, "TIME_MARCHING")
        } else {
            Some(Value::Text("steady".into()))
        },
    );
    put("iterations", count(cfg, "ITER"));
    put("write_interval", count(cfg, "OUTPUT_WRT_FREQ"));
    put("residual_target", number(cfg, "CONV_RESIDUAL_MINVAL"));
    put("time_step", number(cfg, "TIME_STEP"));
    put("end_time", number(cfg, "MAX_TIME"));
    put("time_steps", count(cfg, "TIME_ITER"));
    put("reference_temperature", number(cfg, "INC_TEMPERATURE_INIT"));
    put(
        "buoyancy_product",
        number(cfg, "THERMAL_EXPANSION_COEFF").map(|v| match v {
            Value::Num(c) => Value::Num(c * STANDARD_GRAVITY),
            other => other,
        }),
    );
    put("prandtl", number(cfg, "PRANDTL_LAM"));
    put("turbulent_prandtl", number(cfg, "PRANDTL_TURB"));
    put(
        "turbulence_intensity",
        number(cfg, "FREESTREAM_TURBULENCEINTENSITY"),
    );
    put(
        "viscosity_ratio",
        number(cfg, "FREESTREAM_TURB2LAMVISCRATIO"),
    );
    // Which keys name each marker, the numbers they carry and whether the mesh holds the marker.
    let keyed: [(&str, usize); 7] = [
        ("MARKER_EULER", 1),
        ("MARKER_HEATFLUX", 2),
        ("MARKER_INLET", 6),
        ("MARKER_ISOTHERMAL", 2),
        ("MARKER_MOVING", 1),
        ("MARKER_OUTLET", 2),
        ("MARKER_SYM", 1),
    ];
    let listed: Vec<(&str, Vec<Vec<String>>)> = keyed
        .iter()
        .map(|(key, stride)| (*key, groups(cfg, key, *stride)))
        .collect();
    let rates = groups(cfg, "SURFACE_TRANSLATION_RATE", 3);
    for m in list {
        let key = |member: &str| format!("marker/{}/{member}", m.name);
        let naming: Vec<&str> = listed
            .iter()
            .filter(|(_, g)| g.iter().any(|group| group[0] == m.name))
            .map(|(k, _)| *k)
            .collect();
        let mut kind = naming.join("+");
        if mesh.marker(m.name).is_none() {
            kind.push_str(" (no mesh marker)");
        }
        put(&key("bc"), Some(Value::Text(kind)));
        let group_of = |name: &str| {
            listed
                .iter()
                .find(|(k, _)| *k == name)
                .and_then(|(_, g)| g.iter().find(|group| group[0] == m.name))
        };
        if let Some(group) = group_of("MARKER_INLET") {
            let numbers = group_numbers(group);
            for (member, at) in [("speed", 1), ("direction_x", 2), ("direction_y", 3)] {
                put(&key(member), numbers.get(at).cloned());
            }
        }
        if let Some(at) = listed
            .iter()
            .find(|(k, _)| *k == "MARKER_MOVING")
            .and_then(|(_, g)| g.iter().position(|group| group[0] == m.name))
            && let Some(rate) = rates.get(at)
        {
            let v: Vec<Value> = rate
                .iter()
                .map(|w| {
                    w.parse::<f64>()
                        .map_or_else(|_| Value::Text(w.clone()), Value::Num)
                })
                .collect();
            put(&key("velocity_x"), v.first().cloned());
            put(&key("velocity_y"), v.get(1).cloned());
        }
        if let Some(group) = group_of("MARKER_ISOTHERMAL") {
            put(&key("temperature"), group_numbers(group).first().cloned());
        }
    }
    let mut tags: Vec<&str> = mesh.markers.iter().map(|m| m.tag.as_str()).collect();
    tags.sort_unstable();
    put("mesh/markers", Some(Value::Text(tags.join(","))));
    put("mesh/dimension", mesh.dimension.map(Value::Count));
    put("mesh/cells", mesh.cells_declared.map(Value::Count));
    put("mesh/consistent", Some(Value::Text(mesh.consistency())));
    let uses_file = cfg
        .get("SPECIFIED_INLET_PROFILE")
        .is_some_and(|v| v == "YES");
    let inlet = list.iter().find(|m| m.kind == BoundaryKind::VelocityInlet);
    let status = match (uses_file, profile, inlet) {
        (false, _, _) => "uniform".to_owned(),
        (true, None, _) => "the case has no inlet profile file".to_owned(),
        (true, Some(_), None) => "the study has no inlet to profile".to_owned(),
        (true, Some(p), Some(inlet)) => profile_status(study, inlet, p, mesh),
    };
    put("inlet/profile", Some(Value::Text(status)));
    found
}

/// The three files of a case, parsed.
struct Parsed {
    cfg: BTreeMap<String, String>,
    mesh: MeshRead,
    profile: Option<ProfileRead>,
}

fn parse_case(tree: &CaseTree) -> Result<Parsed, ReadError> {
    let config = file(tree, CONFIG_FILE).ok_or(ReadError::Missing { file: CONFIG_FILE })?;
    let mesh = file(tree, MESH_FILE).ok_or(ReadError::Missing { file: MESH_FILE })?;
    Ok(Parsed {
        cfg: parse_config(&text(config))?,
        mesh: parse_mesh(&text(mesh))?,
        profile: file(tree, INLET_FILE)
            .map(|b| parse_profile(&text(b)))
            .transpose()?,
    })
}

fn unreadable(error: &ReadError) -> Diagnostic {
    Diagnostic::error(
        "CFD-RT-002",
        "case file",
        "",
        format!("The written case could not be read back: {error}."),
    )
    .with_hint(NOT_RUNNABLE)
}

/// Compare what is read from the case with what the study requires. Empty means every fact was
/// read back and matches.
pub fn read_back(study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic> {
    let layout = match layout_of(study) {
        Ok(l) => l,
        Err(e) => {
            return vec![Diagnostic::error(
                "CFD-RT-001",
                "mesh",
                "/mesh",
                e.to_string(),
            )];
        }
    };
    let list = match markers(study, &layout) {
        Ok(l) => l,
        Err(e) => {
            return vec![Diagnostic::error(
                "CFD-RT-001",
                "boundary conditions",
                "/boundary_conditions",
                e.to_string(),
            )];
        }
    };
    let Parsed { cfg, mesh, profile } = match parse_case(tree) {
        Ok(p) => p,
        Err(e) => return vec![unreadable(&e)],
    };
    let found = found_facts(&cfg, &mesh, profile.as_ref(), study, &list);
    let mut out = Vec::new();
    for w in expected(study, &list, layout.cell_count) {
        let d = match found.get(&w.key) {
            None => Diagnostic::error(
                "CFD-RT-001",
                w.setting.clone(),
                w.pointer.clone(),
                format!(
                    "The {} was not found in the written case, but the study has {}.",
                    w.setting, w.value
                ),
            ),
            Some(f) if !same(&w, f) => Diagnostic::error(
                "CFD-RT-001",
                w.setting.clone(),
                w.pointer.clone(),
                format!(
                    "The {} is written as {f} in the case, but the study has {}.",
                    w.setting, w.value
                ),
            ),
            Some(_) => continue,
        };
        out.push(d.with_hint(NOT_RUNNABLE));
    }
    out
}
