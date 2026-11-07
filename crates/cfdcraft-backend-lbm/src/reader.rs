//! A reader of the written command stream that shares no code with the writer. It parses each line
//! as JSON, takes the values out of the commands by name, and compares them with what the study
//! says, so a value that the writer lost or changed is a finding rather than a surprise at run
//! time.

use crate::markers::{
    BODY_FACE, FIELD_VELOCITY, FRAME_BYTES_PER_CELL, FRAME_MEMORY_BYTES, LINE_COUNT, LINE_FIELD,
    LINE_FROM, LINE_TO, MAX_FRAMES, MIN_FRAMES, PARAM_FORCES, PARAM_FRAME, PARAM_HEAT, PARAM_LINE,
    PARAM_POINTS, PARAM_STEP, PROBE_COMMAND, SLAB_CELLS_Z, STEP_FRAMES,
};
use cfdcraft_backend::CaseTree;
use cfdcraft_model::{
    BoundaryKind, Diagnostic, Face, InletProfile, Location, MonitorField, ResolvedShape,
    ResolvedStudy, ResolvedThermal, ResolvedTime, SurfaceQuantity,
    diagnostics::{pointer_token, setting_label},
    speed_from_vector,
};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, path::Path};

const FILE: &str = "commands.jsonl";

/// Two numbers that should be the same differ by at most this fraction of the larger.
const TOLERANCE: f64 = 1e-12;

/// The setup commands a case of this study holds, in order, up to the solve: the surface of a body
/// is imported before the domain names it. The probe calls after the solve are compared apart.
fn sequence(study: &ResolvedStudy) -> Vec<&'static str> {
    let mut names = vec!["model.units", "model.material.create"];
    if study.domain.shape.is_some() {
        names.push("geometry.import.mesh");
    }
    names.extend([
        "model.fluid_domain.create",
        "model.step.create",
        "solve.fluid",
    ]);
    names
}

const NOT_RUNNABLE: &str = "The case does not match the study, so it must not be run. Compile it again; if this repeats, the writer has a fault.";

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Number(f64),
    Text(String),
}

impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(value) => write!(f, "{value}"),
            Self::Text(text) => f.write_str(text),
        }
    }
}

/// Whether a value read from the case is the one the study asks for. Numbers are compared to a
/// tolerance, because some are derived (a viscosity from a dynamic viscosity and a density).
fn same(want: &Val, found: &Val) -> bool {
    match (want, found) {
        (Val::Number(a), Val::Number(b)) => (a - b).abs() <= TOLERANCE * a.abs().max(b.abs()),
        (Val::Text(a), Val::Text(b)) => a == b,
        _ => false,
    }
}

/// A value found in the case and the line it was found on.
struct Found {
    value: Val,
    line: u64,
}

/// A value the case must hold, with the setting it comes from in the user's words.
struct Want {
    key: String,
    setting: String,
    pointer: String,
    value: Val,
}

struct Call {
    name: String,
    params: Value,
    line: u64,
}

fn unreadable(line: u64, message: &str) -> Diagnostic {
    let mut found = Diagnostic::error(
        "CFD-RT-002",
        "case file",
        "",
        format!("The written case file {FILE} could not be read back: {message}."),
    )
    .with_hint(NOT_RUNNABLE);
    found.location = Some(Location {
        file: FILE.to_owned(),
        line,
        entry: None,
    });
    found
}

/// The commands of the stream, in order, with the line each is on.
fn read_calls(text: &str) -> Result<Vec<Call>, Box<Diagnostic>> {
    let bad = |line: u64, message: &str| Box::new(unreadable(line, message));
    let mut calls = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = u64::try_from(index + 1).unwrap_or(u64::MAX);
        let value: Value =
            serde_json::from_str(raw).map_err(|_| bad(line, "the line is not valid JSON"))?;
        match value.get("method").and_then(Value::as_str) {
            Some("initialize") => {}
            Some("tools/call") => {
                let arguments = value.pointer("/params/arguments");
                let entries: Vec<&Value> =
                    match value.pointer("/params/name").and_then(Value::as_str) {
                        Some("batch") => arguments
                            .and_then(|a| a.get("commands"))
                            .and_then(Value::as_array)
                            .map(|list| list.iter().collect::<Vec<&Value>>())
                            .ok_or_else(|| bad(line, "a batch holds no list of commands"))?,
                        Some("execute") => arguments.into_iter().collect(),
                        _ => return Err(bad(line, "the call names no known tool")),
                    };
                for entry in entries {
                    let name = entry
                        .get("command")
                        .and_then(Value::as_str)
                        .ok_or_else(|| bad(line, "a command has no name"))?;
                    calls.push(Call {
                        name: name.to_owned(),
                        params: entry.get("params").cloned().unwrap_or(Value::Null),
                        line,
                    });
                }
            }
            _ => return Err(bad(line, "the line is not a known request")),
        }
    }
    Ok(calls)
}

fn number(params: &Value, pointer: &str) -> Option<f64> {
    params.pointer(pointer)?.as_f64()
}

fn triple(params: &Value, pointer: &str) -> Option<[f64; 3]> {
    let items = params.pointer(pointer)?.as_array()?;
    match items.as_slice() {
        [x, y, z] => Some([x.as_f64()?, y.as_f64()?, z.as_f64()?]),
        _ => None,
    }
}

/// The sides of the slab, as the solver names them and as the study names them.
const SIDES: [(&str, &str, Option<Face>); 6] = [
    ("x_min", "XMin", Some(Face::XMin)),
    ("x_max", "XMax", Some(Face::XMax)),
    ("y_min", "YMin", Some(Face::YMin)),
    ("y_max", "YMax", Some(Face::YMax)),
    ("z_min", "ZMin", None),
    ("z_max", "ZMax", None),
];

/// The flag as a number, so that a missing member and a present one both read back as a fact.
fn flag(present: bool) -> Val {
    Val::Number(if present { 1.0 } else { 0.0 })
}

/// Every value the commands hold, by a key that names the setting and not the command.
fn read_facts(calls: &[Call]) -> BTreeMap<String, Found> {
    let mut out = BTreeMap::new();
    let mut put = |key: String, value: Val, line: u64| {
        out.insert(key, Found { value, line });
    };
    for call in calls {
        let (params, line) = (&call.params, call.line);
        match call.name.as_str() {
            "model.material.create" => {
                let density = number(params, "/density");
                let viscosity = number(params, "/fluid/viscosity");
                if let (Some(density), Some(viscosity)) = (density, viscosity)
                    && density > 0.0
                {
                    put(
                        "/fluid/kinematic_viscosity".to_owned(),
                        Val::Number(viscosity / density),
                        line,
                    );
                    if let (Some(k), Some(c)) = (
                        number(params, "/thermal/conductivity"),
                        number(params, "/thermal/specific_heat"),
                    ) && k > 0.0
                        && c > 0.0
                    {
                        put(
                            "/physics/buoyancy/prandtl".to_owned(),
                            Val::Number((viscosity / density) / (k / (density * c))),
                            line,
                        );
                    }
                }
                put(
                    "/fluid/heat".to_owned(),
                    flag(params.pointer("/thermal").is_some()),
                    line,
                );
            }
            "geometry.import.mesh" => {
                if let Some(path) = params.pointer("/path").and_then(Value::as_str) {
                    put(
                        "/obstacle/file".to_owned(),
                        Val::Text(path.to_owned()),
                        line,
                    );
                }
            }
            "model.fluid_domain.create" => {
                if let (Some(low), Some(high), Some(spacing)) = (
                    triple(params, "/min"),
                    triple(params, "/max"),
                    number(params, "/spacing"),
                ) {
                    put("/mesh/spacing".to_owned(), Val::Number(spacing), line);
                    for (axis, index) in [("x", 0), ("y", 1)] {
                        let length = high[index] - low[index];
                        put(format!("/domain/length_{axis}"), Val::Number(length), line);
                        if spacing > 0.0 {
                            let cells = (length / spacing).round();
                            put(format!("/mesh/cells_{axis}"), Val::Number(cells), line);
                            // The extent must be a whole number of cells.
                            put(
                                format!("/mesh/fit_{axis}"),
                                Val::Number(cells * spacing),
                                line,
                            );
                        }
                    }
                    if spacing > 0.0 {
                        put(
                            "/mesh/slab_cells".to_owned(),
                            Val::Number(((high[2] - low[2]) / spacing).round()),
                            line,
                        );
                    }
                }
                let bodies = params
                    .pointer("/obstacles")
                    .and_then(Value::as_array)
                    .map_or(&[][..], Vec::as_slice);
                put(
                    "/obstacle/count".to_owned(),
                    Val::Number(bodies.len() as f64),
                    line,
                );
                if let Some(name) = bodies.first().and_then(Value::as_str) {
                    put(
                        "/obstacle/name".to_owned(),
                        Val::Text(name.to_owned()),
                        line,
                    );
                }
                for (key, face, side) in SIDES {
                    let kind = params
                        .pointer(&format!("/faces/{key}/type"))
                        .and_then(Value::as_str);
                    let Some(kind) = kind else { continue };
                    let at = format!("/boundary_conditions/by_face/{face}");
                    put(format!("{at}/kind"), Val::Text(kind.to_owned()), line);
                    let vector = triple(params, &format!("/faces/{key}/velocity"));
                    let text = |member: &str, default: &str| {
                        let found = params
                            .pointer(&format!("/faces/{key}/{member}"))
                            .and_then(Value::as_str)
                            .unwrap_or(default);
                        Val::Text(found.to_owned())
                    };
                    match (kind, side) {
                        ("moving_wall", Some(side)) => {
                            if let Some(speed) = vector
                                .and_then(|v| speed_from_vector(BoundaryKind::MovingWall, side, v))
                            {
                                put(format!("{at}/velocity"), Val::Number(speed), line);
                            }
                        }
                        ("velocity", Some(side)) => {
                            if let Some(speed) = vector.and_then(|v| {
                                speed_from_vector(BoundaryKind::VelocityInlet, side, v)
                            }) {
                                put(format!("{at}/velocity"), Val::Number(speed), line);
                            }
                            put(format!("{at}/profile"), text("profile", "uniform"), line);
                        }
                        ("pressure", _) => {
                            if let Some(pressure) =
                                number(params, &format!("/faces/{key}/pressure"))
                            {
                                put(format!("{at}/pressure"), Val::Number(pressure), line);
                            }
                            put(format!("{at}/outflow"), text("outflow", "uniform"), line);
                        }
                        _ => {}
                    }
                    if let Some(held) = number(params, &format!("/thermal/faces/{key}/value")) {
                        put(format!("{at}/temperature"), Val::Number(held), line);
                    }
                }
                put(
                    "/domain/heat".to_owned(),
                    flag(params.pointer("/thermal").is_some()),
                    line,
                );
                if let Some(start) = number(params, "/thermal/initial_temperature") {
                    put(
                        "/physics/buoyancy/reference_temperature".to_owned(),
                        Val::Number(start),
                        line,
                    );
                }
                if let Some(g) = triple(params, "/thermal/gravity") {
                    put(
                        "/physics/buoyancy/gravity".to_owned(),
                        Val::Number(-g[1]),
                        line,
                    );
                    put(
                        "/physics/buoyancy/gravity_off_axis".to_owned(),
                        Val::Number(g[0].abs() + g[2].abs()),
                        line,
                    );
                }
                if let Some(beta) = number(params, "/thermal/expansion") {
                    put("/physics/buoyancy/beta".to_owned(), Val::Number(beta), line);
                }
                if let Some(zero) = number(params, "/thermal/temperature_zero") {
                    put("/thermal/zero".to_owned(), Val::Number(zero), line);
                }
            }
            "model.step.create" => {
                if let Some(end) = number(params, "/procedure/end_time") {
                    put("/solver/end_time".to_owned(), Val::Number(end), line);
                }
                put(
                    "/solver/steady".to_owned(),
                    flag(params.pointer("/procedure/steady").is_some()),
                    line,
                );
                if let Some(frames) = number(params, &format!("/procedure/{STEP_FRAMES}")) {
                    put("/solver/frames".to_owned(), Val::Number(frames), line);
                }
                put(
                    "/solver/time_step_given".to_owned(),
                    flag(params.pointer("/procedure/time_step").is_some()),
                    line,
                );
            }
            "solve.fluid" => {
                if let Some(backend) = params.pointer("/backend").and_then(Value::as_str) {
                    put(
                        "/solver/backend".to_owned(),
                        Val::Text(backend.to_owned()),
                        line,
                    );
                }
            }
            _ => {}
        }
    }
    out
}

/// The word the solver uses for a kind of boundary.
fn kind_word(kind: BoundaryKind) -> &'static str {
    match kind {
        BoundaryKind::NoSlipWall => "wall",
        BoundaryKind::SlipWall | BoundaryKind::Symmetry => "slip",
        BoundaryKind::MovingWall => "moving_wall",
        BoundaryKind::VelocityInlet => "velocity",
        BoundaryKind::PressureOutlet => "pressure",
    }
}

/// What the study requires of the case, worked out here from the study alone.
fn wants(study: &ResolvedStudy) -> Vec<Want> {
    let want = |key: &str, setting: &str, pointer: &str, value: Val| Want {
        key: key.to_owned(),
        setting: setting.to_owned(),
        pointer: pointer.to_owned(),
        value,
    };
    let shaped = study.domain.shape.is_some();
    let physics = study.physics.as_ref();
    let buoyancy = physics.and_then(|p| p.buoyancy);
    let parabolic = study.boundary_conditions.values().any(|b| {
        b.kind == BoundaryKind::VelocityInlet && b.profile == Some(InletProfile::Parabolic)
    });
    let ends = if parabolic || buoyancy.is_some() {
        "periodic"
    } else {
        "slip"
    };
    let mut out = vec![
        want(
            "/fluid/kinematic_viscosity",
            &setting_label("/fluid/kinematic_viscosity"),
            "/fluid/kinematic_viscosity",
            Val::Number(study.fluid.kinematic_viscosity),
        ),
        want(
            "/domain/length_x",
            &setting_label("/domain/length_x"),
            "/domain/length_x",
            Val::Number(study.domain.length_x),
        ),
        want(
            "/domain/length_y",
            &setting_label("/domain/length_y"),
            "/domain/length_y",
            Val::Number(study.domain.length_y),
        ),
        want(
            "/mesh/fit_x",
            "cells along the length",
            "/mesh/cells_x",
            Val::Number(study.domain.length_x),
        ),
        want(
            "/mesh/fit_y",
            "cells along the height",
            "/mesh/cells_y",
            Val::Number(study.domain.length_y),
        ),
        want(
            "/mesh/slab_cells",
            "cells through the slab",
            "",
            Val::Number(f64::from(SLAB_CELLS_Z)),
        ),
        want(
            "/solver/backend",
            "solver processor",
            "",
            Val::Text("cpu".to_owned()),
        ),
        want(
            "/solver/steady",
            "steady rule",
            "/physics/time",
            flag(!matches!(
                physics.map(|p| p.time),
                Some(ResolvedTime::Transient { .. })
            )),
        ),
        want(
            "/solver/time_step_given",
            "time step",
            "",
            flag(buoyancy.is_some() && thermal_difference(study)),
        ),
        want(
            "/obstacle/count",
            "number of bodies",
            "/domain/shape",
            Val::Number(if shaped { 1.0 } else { 0.0 }),
        ),
        want(
            "/fluid/heat",
            "heat in the fluid material",
            "/physics/buoyancy",
            flag(buoyancy.is_some()),
        ),
        want(
            "/domain/heat",
            "heat in the domain",
            "/physics/buoyancy",
            flag(buoyancy.is_some()),
        ),
    ];
    if !shaped {
        // A shape takes its own cell size, which only has to fit every extent in whole cells.
        out.push(want(
            "/mesh/spacing",
            "cell size",
            "/mesh/cells_x",
            Val::Number(study.domain.length_x / study.mesh.cells_x as f64),
        ));
        out.push(want(
            "/mesh/cells_x",
            &setting_label("/mesh/cells_x"),
            "/mesh/cells_x",
            Val::Number(study.mesh.cells_x as f64),
        ));
        out.push(want(
            "/mesh/cells_y",
            &setting_label("/mesh/cells_y"),
            "/mesh/cells_y",
            Val::Number(study.mesh.cells_y as f64),
        ));
    }
    match study.domain.shape {
        Some(ResolvedShape::BackwardStep { .. }) => {
            out.push(want(
                "/obstacle/name",
                "name of the body",
                "/domain/shape",
                Val::Text("Step".to_owned()),
            ));
            out.push(want(
                "/obstacle/file",
                "file of the body",
                "/domain/shape",
                Val::Text("obstacles/step.3mf".to_owned()),
            ));
            out.push(want(
                "/obstacle/present",
                "surface file of the body in the case",
                "/domain/shape",
                flag(true),
            ));
        }
        Some(ResolvedShape::CylinderChannel { .. }) => {
            out.push(want(
                "/obstacle/name",
                "name of the body",
                "/domain/shape",
                Val::Text("Cylinder".to_owned()),
            ));
            out.push(want(
                "/obstacle/file",
                "file of the body",
                "/domain/shape",
                Val::Text("obstacles/cylinder.3mf".to_owned()),
            ));
            out.push(want(
                "/obstacle/present",
                "surface file of the body in the case",
                "/domain/shape",
                flag(true),
            ));
        }
        None => {}
    }
    if let Some(b) = buoyancy {
        let held: Vec<f64> = study
            .boundary_conditions
            .values()
            .filter_map(|boundary| match boundary.thermal {
                Some(ResolvedThermal::FixedTemperature { value }) => Some(value),
                _ => None,
            })
            .collect();
        let zero = held.iter().copied().fold(b.reference_temperature, f64::min);
        for (key, setting, value) in [
            ("/physics/buoyancy/prandtl", "Prandtl number", b.prandtl),
            ("/physics/buoyancy/gravity", "gravity", b.gravity),
            ("/physics/buoyancy/beta", "expansion coefficient", b.beta),
            (
                "/physics/buoyancy/reference_temperature",
                "reference temperature",
                b.reference_temperature,
            ),
            (
                "/physics/buoyancy/gravity_off_axis",
                "gravity off the vertical",
                0.0,
            ),
            ("/thermal/zero", "temperature the lattice counts from", zero),
        ] {
            out.push(want(key, setting, "/physics/buoyancy", Val::Number(value)));
        }
    }
    for (name, boundary) in &study.boundary_conditions {
        let token = pointer_token(name);
        let kind = kind_word(boundary.kind);
        for face in &boundary.faces {
            if *face == Face::Obstacle {
                continue;
            }
            let at = format!("/boundary_conditions/by_face/{face:?}");
            let kind_pointer = format!("/boundary_conditions/{token}/kind");
            out.push(want(
                &format!("{at}/kind"),
                &setting_label(&kind_pointer),
                &kind_pointer,
                Val::Text(kind.to_owned()),
            ));
            match boundary.kind {
                BoundaryKind::MovingWall => {
                    let speed_pointer = format!("/boundary_conditions/{token}/velocity");
                    out.push(want(
                        &format!("{at}/velocity"),
                        &setting_label(&speed_pointer),
                        &speed_pointer,
                        Val::Number(boundary.velocity.unwrap_or(0.0)),
                    ));
                }
                BoundaryKind::VelocityInlet => {
                    let speed_pointer = format!("/boundary_conditions/{token}/velocity");
                    let profile_pointer = format!("/boundary_conditions/{token}/profile");
                    let peak = if boundary.profile == Some(InletProfile::Parabolic) {
                        1.5
                    } else {
                        1.0
                    };
                    out.push(want(
                        &format!("{at}/velocity"),
                        &setting_label(&speed_pointer),
                        &speed_pointer,
                        Val::Number(peak * boundary.velocity.unwrap_or(0.0)),
                    ));
                    out.push(want(
                        &format!("{at}/profile"),
                        "inlet profile",
                        &profile_pointer,
                        Val::Text(if peak > 1.0 { "parabolic" } else { "uniform" }.to_owned()),
                    ));
                }
                BoundaryKind::PressureOutlet => {
                    out.push(want(
                        &format!("{at}/pressure"),
                        "outlet pressure",
                        &kind_pointer,
                        Val::Number(0.0),
                    ));
                    out.push(want(
                        &format!("{at}/outflow"),
                        "outlet form",
                        &kind_pointer,
                        Val::Text(if shaped { "convective" } else { "uniform" }.to_owned()),
                    ));
                }
                _ => {}
            }
            if let Some(ResolvedThermal::FixedTemperature { value }) = boundary.thermal
                && buoyancy.is_some()
            {
                out.push(want(
                    &format!("{at}/temperature"),
                    &format!("temperature of the wall at {face:?}"),
                    &format!("/boundary_conditions/{token}/thermal"),
                    Val::Number(value),
                ));
            }
        }
    }
    for (_, face, side) in SIDES {
        if side.is_none() {
            out.push(want(
                &format!("/boundary_conditions/by_face/{face}/kind"),
                &format!("slab side {face}"),
                "",
                Val::Text(ends.to_owned()),
            ));
        }
    }
    out
}

/// Whether the walls of a buoyant study differ in temperature, which is what gives the writer a
/// buoyancy speed to set the time step from.
fn thermal_difference(study: &ResolvedStudy) -> bool {
    let held: Vec<f64> = study
        .boundary_conditions
        .values()
        .filter_map(|boundary| match boundary.thermal {
            Some(ResolvedThermal::FixedTemperature { value }) => Some(value),
            _ => None,
        })
        .collect();
    let high = held.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = held.iter().copied().fold(f64::INFINITY, f64::min);
    high > low
}

fn mismatch(want: &Want, found: Option<&Found>) -> Diagnostic {
    let message = match found {
        None => format!(
            "The {} was not found in the written case, but the study has {}.",
            want.setting, want.value
        ),
        Some(found) => format!(
            "The {} is written as {} in the case, but the study has {}.",
            want.setting, found.value, want.value
        ),
    };
    let mut diagnostic = Diagnostic::error("CFD-RT-001", &want.setting, &want.pointer, message)
        .with_hint(NOT_RUNNABLE);
    diagnostic.location = found.map(|found| Location {
        file: FILE.to_owned(),
        line: found.line,
        entry: None,
    });
    diagnostic
}

/// The values of the written case compared with the study; empty when every one matches.
pub fn read_back(study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic> {
    let Some(file) = tree.files.iter().find(|f| f.rel_path == Path::new(FILE)) else {
        return vec![unreadable(1, "the case has no such file")];
    };
    let Ok(text) = std::str::from_utf8(&file.bytes) else {
        return vec![unreadable(1, "the file is not text")];
    };
    let calls = match read_calls(text) {
        Ok(calls) => calls,
        Err(found) => return vec![*found],
    };
    let mut out = Vec::new();
    // The stream splits at its first probe call: what comes before is the setup and the solve, and
    // what comes after must be probe calls.
    let split = calls
        .iter()
        .position(|call| call.name == PROBE_COMMAND)
        .unwrap_or(calls.len());
    let (calls, probes) = calls.split_at(split);
    let names: Vec<&str> = calls.iter().map(|call| call.name.as_str()).collect();
    let expected = sequence(study);
    if let Some(other) = probes.iter().find(|call| call.name != PROBE_COMMAND) {
        out.push(
            Diagnostic::error(
                "CFD-RT-001",
                "command stream",
                "",
                format!(
                    "The written case runs the command {} after its first probe, and only probes may follow.",
                    clip(&other.name)
                ),
            )
            .with_hint(NOT_RUNNABLE),
        );
    }
    if names != expected {
        out.push(
            Diagnostic::error(
                "CFD-RT-001",
                "command stream",
                "",
                format!(
                    "The written case runs the commands {names:?}, but the study needs {expected:?}."
                ),
            )
            .with_hint(NOT_RUNNABLE),
        );
    }
    let mut facts = read_facts(calls);
    let end_time = facts.remove("/solver/end_time");
    let transient = matches!(
        study.physics.as_ref().map(|p| p.time),
        Some(ResolvedTime::Transient { .. })
    );
    let ends = match &end_time {
        Some(Found {
            value: Val::Number(t),
            ..
        }) => {
            t.is_finite()
                && *t > 0.0
                && (!transient || same(&Val::Number(study.solver.end_time), &Val::Number(*t)))
        }
        _ => false,
    };
    if !ends {
        let value = if transient {
            Val::Number(study.solver.end_time)
        } else {
            Val::Text("a number above zero".to_owned())
        };
        out.push(mismatch(
            &Want {
                key: "/solver/end_time".to_owned(),
                setting: "end time".to_owned(),
                pointer: String::new(),
                value,
            },
            end_time.as_ref(),
        ));
    }
    if study.domain.shape.is_some() {
        for key in ["/mesh/spacing", "/mesh/cells_x", "/mesh/cells_y"] {
            facts.remove(key);
        }
    }
    // The surface the stream imports must be a file of the case, and not empty.
    let imported = facts.get("/obstacle/file").map(|found| {
        let present = matches!(
            &found.value,
            Val::Text(path)
                if tree
                    .files
                    .iter()
                    .any(|f| f.rel_path == Path::new(path) && !f.bytes.is_empty())
        );
        (found.line, present)
    });
    if let Some((line, present)) = imported {
        facts.insert(
            "/obstacle/present".to_owned(),
            Found {
                value: flag(present),
                line,
            },
        );
    }
    out.extend(probe_findings(study, calls, probes));
    let mut wanted = wants(study);
    // How many frames the step records follows from the lattice the stream itself sets up.
    match layout(calls) {
        Some(at) => wanted.push(Want {
            key: "/solver/frames".to_owned(),
            setting: "frames of the step".to_owned(),
            pointer: "/physics/time".to_owned(),
            value: Val::Number(at.frames(transient) as f64),
        }),
        None => {
            facts.remove("/solver/frames");
        }
    }
    for want in &wanted {
        match facts.get(&want.key) {
            Some(found) if same(&want.value, &found.value) => {}
            other => out.push(mismatch(want, other)),
        }
    }
    for (key, found) in &facts {
        if !wanted.iter().any(|want| &want.key == key) {
            out.push(mismatch(
                &Want {
                    key: key.clone(),
                    setting: key.clone(),
                    pointer: String::new(),
                    value: Val::Text("nothing".to_owned()),
                },
                Some(found),
            ));
        }
    }
    out
}

/// The text of a name found in the case, cut short and without control characters, so that a
/// hostile stream cannot flood a report.
fn clip(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(40).collect()
}

/// What the stream says about its own domain, from which the probes are worked out: the extents,
/// the cell size, the thickness of the slab, the body it names and the name of the step. The
/// expectation never reads the writer's lattice.
struct Layout {
    length_x: f64,
    length_y: f64,
    slab: f64,
    spacing: f64,
    body: Option<String>,
    step: String,
}

impl Layout {
    /// The cells of the lattice, the slab included, counted from the extents and the cell size.
    fn cells(&self) -> u64 {
        let along = |length: f64| (length / self.spacing).round().max(1.0) as u64;
        along(self.length_x)
            .saturating_mul(along(self.length_y))
            .saturating_mul(along(self.slab))
    }

    /// How many frames the run must record, by the rule of the solver's manual: ten for a steady
    /// run, and for a transient one as many as fit in the memory budget, between ten and the most
    /// the solver allows.
    fn frames(&self, transient: bool) -> u64 {
        if !transient {
            return MIN_FRAMES;
        }
        (FRAME_MEMORY_BYTES / FRAME_BYTES_PER_CELL / self.cells().max(1))
            .clamp(MIN_FRAMES, MAX_FRAMES)
    }
}

fn layout(setup: &[Call]) -> Option<Layout> {
    let domain = setup
        .iter()
        .find(|call| call.name == "model.fluid_domain.create")?;
    let (low, high) = (
        triple(&domain.params, "/min")?,
        triple(&domain.params, "/max")?,
    );
    let spacing = number(&domain.params, "/spacing").filter(|s| s.is_finite() && *s > 0.0)?;
    let step = setup
        .iter()
        .find(|call| call.name == "model.step.create")?
        .params
        .pointer("/name")?
        .as_str()?
        .to_owned();
    let body = domain
        .params
        .pointer("/obstacles/0")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some(Layout {
        length_x: high[0] - low[0],
        length_y: high[1] - low[1],
        slab: high[2] - low[2],
        spacing,
        body,
        step,
    })
}

/// A point of a point probe that the study plans, and how close the call must ask to it: a point
/// in the flow exactly, a point on a surface within one cell (it is moved off the surface into the
/// fluid).
struct Planned {
    item: String,
    at: [f64; 2],
    on_surface: bool,
}

/// One probe call the study needs, with the words that name it in a report.
enum Ask {
    Line {
        item: String,
        from: [f64; 3],
        to: [f64; 3],
        count: u64,
        field: &'static str,
    },
    Points {
        frame: u64,
        points: Vec<Planned>,
    },
    Force {
        wall: String,
    },
    Heat {
        sides: Vec<&'static str>,
    },
}

impl Ask {
    /// The member that marks the kind of probe.
    fn form(&self) -> &'static str {
        match self {
            Self::Line { .. } => PARAM_LINE,
            Self::Points { .. } => PARAM_POINTS,
            Self::Force { .. } => PARAM_FORCES,
            Self::Heat { .. } => PARAM_HEAT,
        }
    }

    fn item(&self) -> String {
        match self {
            Self::Line { item, .. } => item.clone(),
            Self::Points { frame, .. } => format!("point probes of frame {frame}"),
            Self::Force { .. } => "force probe".to_owned(),
            Self::Heat { .. } => "heat probe".to_owned(),
        }
    }
}

/// The three velocity components along a line, in the order the writer asks for them.
fn line_asks(item: &str, from: [f64; 3], to: [f64; 3], count: u64, out: &mut Vec<Ask>) {
    for field in FIELD_VELOCITY {
        out.push(Ask::Line {
            item: format!("{item}, {field}"),
            from,
            to,
            count,
            field,
        });
    }
}

/// The probe calls the study needs, worked out from the study and the layout of the stream: the
/// centrelines of a study with no shape; every velocity line of the monitor plan; when the plan
/// holds probes, one point call for each frame; the force of the body; the heat of the box sides.
fn expected_probes(study: &ResolvedStudy, at: &Layout) -> Vec<Ask> {
    let z = at.slab / 2.0;
    let mut out = Vec::new();
    if study.domain.shape.is_none() {
        let count = study.monitors.centreline_u_sample_count;
        let (lx, ly) = (at.length_x, at.length_y);
        line_asks(
            "vertical centreline",
            [lx / 2.0, 0.0, z],
            [lx / 2.0, ly, z],
            count,
            &mut out,
        );
        line_asks(
            "horizontal centreline",
            [0.0, ly / 2.0, z],
            [lx, ly / 2.0, z],
            count,
            &mut out,
        );
    }
    let plan = study.monitor_plan();
    for line in &plan.lines {
        if line.field == MonitorField::Velocity {
            let item = line.fo.replace('_', " ");
            let (from, to) = ([line.x0, line.y0, z], [line.x1, line.y1, z]);
            line_asks(&item, from, to, line.samples, &mut out);
        }
    }
    let transient = matches!(
        study.physics.as_ref().map(|p| p.time),
        Some(ResolvedTime::Transient { .. })
    );
    if !plan.probes.is_empty() {
        for frame in 1..=at.frames(transient) {
            let points = plan
                .probes
                .iter()
                .map(|probe| Planned {
                    item: probe.fo.replace('_', " "),
                    at: [probe.x, probe.y],
                    on_surface: probe.boundary.is_some(),
                })
                .collect();
            out.push(Ask::Points { frame, points });
        }
    }
    let on_body = |name: &str| {
        study
            .boundary_conditions
            .get(name)
            .is_some_and(|boundary| boundary.faces == [Face::Obstacle])
    };
    if let Some(body) = &at.body
        && plan.forces.iter().any(|force| on_body(&force.boundary))
    {
        out.push(Ask::Force {
            wall: format!("{body}/{BODY_FACE}"),
        });
    }
    if study.physics.as_ref().is_some_and(|p| p.buoyancy.is_some()) {
        let mut held = [false; 4];
        for surface in &plan.surfaces {
            let faces = study
                .boundary_conditions
                .get(&surface.boundary)
                .map(|boundary| boundary.faces.as_slice());
            if surface.quantity == SurfaceQuantity::HeatFlux
                && let Some(&[face]) = faces
                && let Some(index) = [Face::XMin, Face::XMax, Face::YMin, Face::YMax]
                    .iter()
                    .position(|side| *side == face)
            {
                held[index] = true;
            }
        }
        let sides: Vec<&'static str> = ["x_min", "x_max", "y_min", "y_max"]
            .into_iter()
            .zip(held)
            .filter_map(|(side, held)| held.then_some(side))
            .collect();
        if !sides.is_empty() {
            out.push(Ask::Heat { sides });
        }
    }
    out
}

/// A difference between a probe call and what the study needs, in the study's own words.
fn probe_finding(setting: &str, line: Option<u64>, message: String) -> Diagnostic {
    let mut found = Diagnostic::error("CFD-RT-001", setting, "", message).with_hint(NOT_RUNNABLE);
    found.location = line.map(|line| Location {
        file: FILE.to_owned(),
        line,
        entry: None,
    });
    found
}

/// Whether two numbers of a position are the same to a rounding error.
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

fn near3(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| near(*x, y))
}

/// A call that reads the step the case runs.
fn check_step(item: &str, layout: &Layout, call: &Call, out: &mut Vec<Diagnostic>) {
    let step = call.params.get(PARAM_STEP).and_then(Value::as_str);
    if step != Some(layout.step.as_str()) {
        out.push(probe_finding(
            &format!("{item}, step"),
            Some(call.line),
            format!("The {item} reads a step other than the one the case runs."),
        ));
    }
}

fn compare_line(ask: &Ask, layout: &Layout, call: &Call, out: &mut Vec<Diagnostic>) {
    let Ask::Line {
        item,
        from,
        to,
        count,
        field,
    } = ask
    else {
        return;
    };
    let params = &call.params;
    let said = |member: &str| format!("{item}, {member}");
    let ends = [
        ("start of the line", LINE_FROM, *from),
        ("end of the line", LINE_TO, *to),
    ];
    for (words, member, want) in ends {
        match triple(params, &format!("/{PARAM_LINE}/{member}")) {
            Some(found) if near3(found, want) => {}
            Some(found) => out.push(probe_finding(
                &said(words),
                Some(call.line),
                format!(
                    "The {words} of the {item} is {found:?} in the case, but the study needs {want:?}."
                ),
            )),
            None => out.push(probe_finding(
                &said(words),
                Some(call.line),
                format!("The {words} of the {item} is missing from the case."),
            )),
        }
    }
    let found = number(params, &format!("/{PARAM_LINE}/{LINE_COUNT}"));
    if found != Some(*count as f64) {
        out.push(probe_finding(
            &said("sample count"),
            Some(call.line),
            format!(
                "The sample count of the {item} is {found:?} in the case, but the study needs {count}."
            ),
        ));
    }
    let found = params
        .pointer(&format!("/{PARAM_LINE}/{LINE_FIELD}"))
        .and_then(Value::as_str);
    if found != Some(*field) {
        out.push(probe_finding(
            &said("velocity component"),
            Some(call.line),
            format!(
                "The velocity component of the {item} is {} in the case, but the study needs {field}.",
                found.map_or_else(|| "missing".to_owned(), clip)
            ),
        ));
    }
    check_step(item, layout, call, out);
}

fn compare_points(ask: &Ask, layout: &Layout, call: &Call, out: &mut Vec<Diagnostic>) {
    let Ask::Points { frame, points } = ask else {
        return;
    };
    let item = ask.item();
    let said = |member: &str| format!("{item}, {member}");
    let found = number(&call.params, &format!("/{PARAM_FRAME}"));
    if found != Some(*frame as f64) {
        out.push(probe_finding(
            &said("frame number"),
            Some(call.line),
            format!(
                "The frame number of the {item} is {found:?} in the case, but it must be {frame}."
            ),
        ));
    }
    let asked: Vec<[f64; 3]> = call
        .params
        .get(PARAM_POINTS)
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|point| triple(point, ""))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if asked.len() != points.len() {
        out.push(probe_finding(
            &said("number of points"),
            Some(call.line),
            format!(
                "The {item} asks about {} points in the case, but the study plans {}.",
                asked.len(),
                points.len()
            ),
        ));
    } else {
        let z = layout.slab / 2.0;
        for (planned, found) in points.iter().zip(&asked) {
            let off = if planned.on_surface {
                // A point on a surface is moved into the fluid, by less than one cell.
                (found[0] - planned.at[0]).hypot(found[1] - planned.at[1])
                    > layout.spacing * (1.0 + 1e-9)
            } else {
                !(near(found[0], planned.at[0]) && near(found[1], planned.at[1]))
            };
            if off || !near(found[2], z) {
                out.push(probe_finding(
                    &said(&format!("position of {}", planned.item)),
                    Some(call.line),
                    format!(
                        "The position of {} is {found:?} in the {item} of the case, which is not where the study plans it.",
                        planned.item
                    ),
                ));
            }
        }
    }
    check_step(&item, layout, call, out);
}

/// The names in a list of a probe call, or `None` when the member is not a list of text.
fn names_in(call: &Call, member: &str) -> Option<Vec<String>> {
    call.params
        .get(member)?
        .as_array()?
        .iter()
        .map(|name| name.as_str().map(str::to_owned))
        .collect()
}

fn compare_force(ask: &Ask, layout: &Layout, call: &Call, out: &mut Vec<Diagnostic>) {
    let Ask::Force { wall } = ask else {
        return;
    };
    let found = names_in(call, PARAM_FORCES);
    if found.as_deref() != Some(std::slice::from_ref(wall)) {
        out.push(probe_finding(
            "force probe, wall",
            Some(call.line),
            format!(
                "The wall of the force probe is {:?} in the case, but the study needs {}.",
                found.map(|names| names.iter().map(|n| clip(n)).collect::<Vec<_>>()),
                clip(wall)
            ),
        ));
    }
    check_step("force probe", layout, call, out);
}

fn compare_heat(ask: &Ask, layout: &Layout, call: &Call, out: &mut Vec<Diagnostic>) {
    let Ask::Heat { sides } = ask else {
        return;
    };
    let found = names_in(call, PARAM_HEAT);
    let same = found
        .as_ref()
        .is_some_and(|names| names.iter().map(String::as_str).eq(sides.iter().copied()));
    if !same {
        out.push(probe_finding(
            "heat probe, sides",
            Some(call.line),
            format!(
                "The sides of the heat probe are {:?} in the case, but the study needs {sides:?}.",
                found.map(|names| names.iter().map(|n| clip(n)).collect::<Vec<_>>())
            ),
        ));
    }
    check_step("heat probe", layout, call, out);
}

/// The kind of probe a call is, by the member it holds.
fn form_of(call: &Call) -> Option<&'static str> {
    [PARAM_POINTS, PARAM_LINE, PARAM_FORCES, PARAM_HEAT]
        .into_iter()
        .find(|form| call.params.get(*form).is_some())
}

/// The probe calls of the stream compared with the probes the study needs: each difference, and
/// each call the study does not account for, is a finding.
fn probe_findings(study: &ResolvedStudy, setup: &[Call], probes: &[Call]) -> Vec<Diagnostic> {
    let Some(at) = layout(setup) else {
        return Vec::new();
    };
    let wanted = expected_probes(study, &at);
    let mut out = Vec::new();
    for index in 0..wanted.len().max(probes.len()) {
        match (wanted.get(index), probes.get(index)) {
            (Some(ask), None) => {
                let item = ask.item();
                out.push(probe_finding(
                    &item,
                    None,
                    format!(
                        "The {item} is not asked for in the written case, but the study needs it."
                    ),
                ));
            }
            (None, Some(call)) => out.push(probe_finding(
                "probe call",
                Some(call.line),
                "The written case holds a probe call that the study does not account for."
                    .to_owned(),
            )),
            (Some(ask), Some(call)) if form_of(call) == Some(ask.form()) => match ask {
                Ask::Line { .. } => compare_line(ask, &at, call, &mut out),
                Ask::Points { .. } => compare_points(ask, &at, call, &mut out),
                Ask::Force { .. } => compare_force(ask, &at, call, &mut out),
                Ask::Heat { .. } => compare_heat(ask, &at, call, &mut out),
            },
            (Some(ask), Some(call)) => {
                let item = ask.item();
                out.push(probe_finding(
                    &item,
                    Some(call.line),
                    format!("The {item} is asked for as another kind of probe in the case."),
                ));
            }
            (None, None) => {}
        }
    }
    out
}
