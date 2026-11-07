//! The command stream of a study: one JSON message per line for the solver's headless server.
//!
//! The first line opens the session. The second runs the setup commands in order, and the third
//! runs the solve. The lines after it are the probes: one `execute` call for each answer the
//! result view serves, asked of the solved step. Every line is built from typed values and
//! serialised by `serde_json`, so no text of the study is ever spliced into JSON, and the same
//! study always gives the same bytes.
//! A study the solver cannot represent faithfully is refused with the setting named; nothing is
//! mapped to something narrower.
//!
//! A shape (the step, the cylinder) is written as a closed surface in a file under `obstacles/`,
//! which a command of the stream imports before the domain names it as an obstacle.

use crate::{
    guard::MAX_LINE_BYTES,
    markers::{
        BATCH_TOOL, BODY_FACE, EXECUTE_TOOL, FIELD_VELOCITY, FRAME_BYTES_PER_CELL,
        FRAME_MEMORY_BYTES, LINE_COUNT, LINE_FIELD, LINE_FROM, LINE_TO, MAX_FRAMES, MAX_LINE_COUNT,
        MAX_PROBE_POINTS, MIN_FRAMES, PARAM_FORCES, PARAM_FRAME, PARAM_HEAT, PARAM_LINE,
        PARAM_POINTS, PARAM_STEP, PROBE_COMMAND, PROTOCOL_VERSION, SLAB_CELLS_Z, SOLVE_COMMAND,
        STEP_FRAMES,
    },
    obstacles::{self, Lattice, MIN_OPEN_CELLS},
    thermal,
};
use cfdcraft_backend::{CaseFile, CompileFailure};
use cfdcraft_model::{
    BoundaryKind, Face, InitMode, InletProfile, MonitorField, PlannedProbe, ResolvedBoundary,
    ResolvedShape, ResolvedStudy, ResolvedTime, SurfaceQuantity, TurbulenceModel, velocity_vector,
};
use serde_json::{Map, Value, json};

pub const MATERIAL_NAME: &str = "Fluid";
pub const DOMAIN_NAME: &str = "Flow";
pub const STEP_NAME: &str = "Run";

/// The density of the fluid in kg/m3. A study states only the kinematic viscosity, and the flow
/// does not depend on the density, so the density is fixed and the dynamic viscosity is the
/// kinematic one times it.
pub const REFERENCE_DENSITY: f64 = 1.0;

/// How many times the flow crosses the domain, at the speed of the fastest moving wall or inlet
/// (or, in a closed buoyant cavity, at the speed buoyancy can drive), before a steady run is
/// stopped at the latest. The steady rule ends a run that has settled earlier; the study's
/// own iteration count means nothing to a time-marching solver.
pub const FLOW_THROUGH_TIMES: f64 = 50.0;

/// The relative change over one window of flow time below which the flow counts as steady.
pub const STEADY_TOLERANCE: f64 = 1e-3;

fn unsupported(setting: &str, reason: impl Into<String>) -> CompileFailure {
    CompileFailure::NotSupported {
        setting: setting.to_owned(),
        reason: reason.into(),
    }
}

fn invalid(reason: impl Into<String>) -> CompileFailure {
    CompileFailure::Invalid {
        reason: reason.into(),
    }
}

/// Refuses every setting the solver cannot run, with the setting named.
fn check_supported(study: &ResolvedStudy) -> Result<(), CompileFailure> {
    if let Some(physics) = &study.physics {
        if physics.turbulence.model != TurbulenceModel::Laminar {
            return Err(unsupported(
                "turbulence model",
                "the solver runs laminar flow only",
            ));
        }
        if physics.init == InitMode::Potential {
            return Err(unsupported(
                "start of the flow",
                "the solver starts from a uniform field",
            ));
        }
    }
    if let Some(label) = study.raw_overrides.keys().next() {
        return Err(CompileFailure::Raw {
            label: label.clone(),
            reason: "the solver takes no raw overrides".to_owned(),
        });
    }
    if study.mesh.grading.is_some() {
        return Err(unsupported(
            "wall grading",
            "the solver needs equal square cells",
        ));
    }
    if study.mesh.cells_z != 1 {
        return Err(unsupported(
            "cell count in z",
            "a two-dimensional study has one cell through z",
        ));
    }
    Ok(())
}

/// The name the solver gives a side of the box, or `None` for a surface that is not a side.
fn face_key(face: Face) -> Option<&'static str> {
    match face {
        Face::XMin => Some("x_min"),
        Face::XMax => Some("x_max"),
        Face::YMin => Some("y_min"),
        Face::YMax => Some("y_max"),
        Face::Obstacle => None,
    }
}

/// How much a parabolic inlet's peak exceeds the mean speed the study states: the solver takes the
/// peak, and a parabola over a channel peaks at one and a half times its mean.
pub const PARABOLIC_PEAK_OVER_MEAN: f64 = 1.5;

/// How the two ends of the slab are closed. A parabolic inlet is spread by the solver across every
/// direction of its face that ends at walls and not at a periodic pair, and a slab closed by slip
/// planes might count as such a direction, which would bend the profile across the slab. A
/// periodic pair is the form the solver's own natural-convection and channel cases use for a flat
/// case. UNPROBED: whether slip planes would have done.
pub fn ends_of_the_slab(study: &ResolvedStudy) -> &'static str {
    let parabolic = study.boundary_conditions.values().any(|b| {
        b.kind == BoundaryKind::VelocityInlet && b.profile == Some(InletProfile::Parabolic)
    });
    let buoyant = study.physics.as_ref().is_some_and(|p| p.buoyancy.is_some());
    if parabolic || buoyant {
        "periodic"
    } else {
        "slip"
    }
}

/// The fluid cells across a face of the box, which the solver wants at least two of for an inlet
/// or an outlet. The step's inlet is the channel above the step, not the whole side.
fn open_cells(face: Face, lattice: &Lattice, study: &ResolvedStudy) -> u64 {
    let along = match face {
        Face::XMin | Face::XMax => lattice.cells_y,
        _ => lattice.cells_x,
    };
    match (study.domain.shape, face) {
        (Some(ResolvedShape::BackwardStep { step_height, .. }), Face::XMin) => {
            along.saturating_sub((step_height / lattice.spacing).round() as u64)
        }
        _ => along,
    }
}

fn condition(
    boundary: &ResolvedBoundary,
    face: Face,
    lattice: &Lattice,
    study: &ResolvedStudy,
) -> Result<Value, CompileFailure> {
    match boundary.kind {
        BoundaryKind::NoSlipWall => Ok(json!({"type": "wall"})),
        BoundaryKind::SlipWall | BoundaryKind::Symmetry => Ok(json!({"type": "slip"})),
        BoundaryKind::MovingWall => Ok(json!({
            "type": "moving_wall",
            "velocity": velocity_vector(boundary.kind, face, boundary.velocity.unwrap_or(0.0)),
        })),
        BoundaryKind::VelocityInlet => {
            let mean = boundary
                .velocity
                .filter(|speed| speed.is_finite() && *speed != 0.0)
                .ok_or_else(|| invalid("an inlet needs a finite speed above zero"))?;
            let parabolic = boundary.profile == Some(InletProfile::Parabolic);
            if parabolic && matches!(study.domain.shape, Some(ResolvedShape::BackwardStep { .. })) {
                return Err(unsupported(
                    "inlet profile",
                    "the solver spreads a parabolic profile over the whole height of the box, but the \
                     inlet of a step is the narrower channel above the step",
                ));
            }
            if open_cells(face, lattice, study) < MIN_OPEN_CELLS {
                return Err(unsupported(
                    "inlet size",
                    "the solver needs at least two cells across an inlet",
                ));
            }
            let speed = if parabolic {
                PARABOLIC_PEAK_OVER_MEAN * mean
            } else {
                mean
            };
            Ok(json!({
                "type": "velocity",
                "velocity": velocity_vector(boundary.kind, face, speed),
                "profile": if parabolic { "parabolic" } else { "uniform" },
            }))
        }
        BoundaryKind::PressureOutlet => {
            if open_cells(face, lattice, study) < MIN_OPEN_CELLS {
                return Err(unsupported(
                    "outlet size",
                    "the solver needs at least two cells across an outlet",
                ));
            }
            // A wake that has not recovered reaches the outlet behind a body, and a uniform outlet
            // puts a step into the last layer there. UNPROBED: the choice of the convective form.
            if study.domain.shape.is_some() {
                Ok(json!({"type": "pressure", "pressure": 0.0, "outflow": "convective"}))
            } else {
                Ok(json!({"type": "pressure", "pressure": 0.0}))
            }
        }
    }
}

/// The condition of every side of the slab: the four sides of the study, and a periodic or slip
/// plane on both ends, which makes the slab behave as the two-dimensional case.
fn faces(study: &ResolvedStudy, lattice: &Lattice) -> Result<Value, CompileFailure> {
    let mut map = Map::new();
    for (name, boundary) in &study.boundary_conditions {
        for face in &boundary.faces {
            let Some(key) = face_key(*face) else {
                if boundary.kind != BoundaryKind::NoSlipWall {
                    return Err(unsupported(
                        "obstacle surface",
                        "a body takes only a wall at rest",
                    ));
                }
                continue;
            };
            let step = matches!(study.domain.shape, Some(ResolvedShape::BackwardStep { .. }));
            if step && *face == Face::YMin && boundary.kind != BoundaryKind::NoSlipWall {
                return Err(unsupported(
                    "step surface",
                    "the faces of the step are walls at rest, and the floor shares its boundary with them",
                ));
            }
            let value = condition(boundary, *face, lattice, study)?;
            if map.insert(key.to_owned(), value).is_some() {
                return Err(invalid(format!(
                    "the side {key} is claimed twice, the second time by the boundary {name}"
                )));
            }
        }
    }
    for key in ["x_min", "x_max", "y_min", "y_max"] {
        if !map.contains_key(key) {
            return Err(invalid(format!("the side {key} has no boundary")));
        }
    }
    let end = json!({"type": ends_of_the_slab(study)});
    map.insert("z_min".to_owned(), end.clone());
    map.insert("z_max".to_owned(), end);
    Ok(Value::Object(map))
}

fn is_transient(study: &ResolvedStudy) -> bool {
    matches!(
        study.physics.as_ref().map(|p| p.time),
        Some(ResolvedTime::Transient { .. })
    )
}

/// The speed that drives the flow: the fastest moving wall or inlet, or, when none moves the
/// flow, the speed buoyancy can drive.
fn flow_speed(study: &ResolvedStudy) -> Result<f64, CompileFailure> {
    let driven = study
        .boundary_conditions
        .values()
        .filter(|boundary| {
            matches!(
                boundary.kind,
                BoundaryKind::MovingWall | BoundaryKind::VelocityInlet
            )
        })
        .filter_map(|boundary| boundary.velocity)
        .map(f64::abs)
        .fold(0.0, f64::max);
    if driven > 0.0 {
        return Ok(driven);
    }
    thermal::buoyancy_speed(study).ok_or_else(|| {
        invalid(
            "no wall or inlet moves the flow and no wall temperature difference drives it, so no \
             flow-through time can be chosen for the run",
        )
    })
}

/// The flow time to run: a transient study's own end time, and for a steady study a number of
/// flow-through times.
fn end_time(study: &ResolvedStudy) -> Result<f64, CompileFailure> {
    if is_transient(study) {
        let end = study.solver.end_time;
        if end.is_finite() && end > 0.0 {
            return Ok(end);
        }
        return Err(invalid("a transient run needs an end time above zero"));
    }
    Ok(FLOW_THROUGH_TIMES * study.domain.length_x / flow_speed(study)?)
}

fn request(id: u32, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn call(id: u32, tool: &str, arguments: Value) -> Value {
    request(
        id,
        "tools/call",
        json!({"name": tool, "arguments": arguments}),
    )
}

/// The setup commands in order: units, material, the obstacle's surface when there is one, the
/// domain, the step.
fn setup(study: &ResolvedStudy, lattice: &Lattice) -> Result<Value, CompileFailure> {
    let mut material = json!({
        "name": MATERIAL_NAME,
        "density": REFERENCE_DENSITY,
        "fluid": {
            "type": "newtonian",
            "viscosity": study.fluid.kinematic_viscosity * REFERENCE_DENSITY,
        },
    });
    if let Some(heat) = thermal::material(study, REFERENCE_DENSITY)? {
        material["thermal"] = heat;
    }
    let mut domain = json!({
        "name": DOMAIN_NAME,
        "material": MATERIAL_NAME,
        "min": [0.0, 0.0, 0.0],
        "max": [study.domain.length_x, study.domain.length_y, lattice.slab()],
        "spacing": lattice.spacing,
        "faces": faces(study, lattice)?,
    });
    if let Some(body) = &lattice.obstacle {
        domain["obstacles"] = json!([body.name]);
    }
    if let Some(heat) = thermal::domain(study)? {
        domain["thermal"] = heat;
    }
    let mut procedure = json!({
        "type": "fluid",
        "domain": DOMAIN_NAME,
        "end_time": end_time(study)?,
    });
    procedure[STEP_FRAMES] = json!(frames(study, lattice));
    if !is_transient(study) {
        procedure["steady"] = json!({"tolerance": STEADY_TOLERANCE});
    }
    if let Some(step) = thermal::time_step(study, lattice.spacing) {
        procedure["time_step"] = json!(step);
    }
    let mut commands = vec![
        json!({"command": "model.units", "params": {"system": "m-kg-s-N-Pa"}}),
        json!({"command": "model.material.create", "params": material}),
    ];
    if let Some(body) = &lattice.obstacle {
        commands.push(json!({"command": "geometry.import.mesh", "params": {"path": body.file}}));
    }
    commands.push(json!({"command": "model.fluid_domain.create", "params": domain}));
    commands.push(json!({
        "command": "model.step.create",
        "params": {"name": STEP_NAME, "procedure": procedure},
    }));
    Ok(json!({"commands": commands}))
}

/// How many frames the run records for the point probes to read afterwards: ten for a steady
/// run, whose last state is what counts. A transient run gets as many as the solver allows
/// (`MAX_FRAMES`), or fewer when the recorded frames of a finer lattice would take more than
/// `FRAME_MEMORY_BYTES` at `FRAME_BYTES_PER_CELL` for each cell, and never fewer than ten.
pub fn frames(study: &ResolvedStudy, lattice: &Lattice) -> u64 {
    if !is_transient(study) {
        return MIN_FRAMES;
    }
    let cells = lattice
        .cells_x
        .saturating_mul(lattice.cells_y)
        .saturating_mul(u64::from(SLAB_CELLS_Z))
        .max(1);
    (FRAME_MEMORY_BYTES / FRAME_BYTES_PER_CELL / cells).clamp(MIN_FRAMES, MAX_FRAMES)
}

/// How far a probe that names a surface is moved off it into the fluid, in cells. A cell next to a
/// convex body is certain to be fluid once its centre is more than half the diagonal of a cell
/// (about 0.71 of a cell) outside the outline, and the result view accepts a point within one cell
/// of the planned one, so 0.9 is the fraction that is both.
pub const SURFACE_OFFSET_CELLS: f64 = 0.9;

/// The point a probe is asked at, at height `z`. A point in the flow is asked where it is planned.
/// A point on a surface is moved into the fluid: away from the middle of the body (the mean of
/// the corners of its outline, which is the centre of both outlines this backend writes) when the
/// surface is the body, and along the inward normal of the nearest of its box sides otherwise.
fn point_of(
    study: &ResolvedStudy,
    lattice: &Lattice,
    probe: &PlannedProbe,
    z: f64,
) -> Result<[f64; 3], CompileFailure> {
    let Some(name) = &probe.boundary else {
        return Ok([probe.x, probe.y, z]);
    };
    let boundary = study
        .boundary_conditions
        .get(name)
        .ok_or_else(|| invalid("a probe names a boundary the study does not have"))?;
    let direction = if boundary.faces == [Face::Obstacle] {
        let body = lattice
            .obstacle
            .as_ref()
            .ok_or_else(|| invalid("a probe on a body in a study that has none"))?;
        let corners = body.outline.len() as f64;
        let middle = body.outline.iter().fold([0.0, 0.0], |sum, corner| {
            [sum[0] + corner[0] / corners, sum[1] + corner[1] / corners]
        });
        let (dx, dy) = (probe.x - middle[0], probe.y - middle[1]);
        let length = dx.hypot(dy);
        if !(length.is_finite() && length > 0.0) {
            return Err(invalid(
                "a probe at the middle of a body has no side of the body to be moved to",
            ));
        }
        [dx / length, dy / length]
    } else {
        let (lx, ly) = (study.domain.length_x, study.domain.length_y);
        boundary
            .faces
            .iter()
            .filter_map(|face| match face {
                Face::XMin => Some((probe.x, [1.0, 0.0])),
                Face::XMax => Some((lx - probe.x, [-1.0, 0.0])),
                Face::YMin => Some((probe.y, [0.0, 1.0])),
                Face::YMax => Some((ly - probe.y, [0.0, -1.0])),
                Face::Obstacle => None,
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, inward)| inward)
            .ok_or_else(|| invalid("a probe on a boundary that has no side of the box"))?
    };
    let reach = SURFACE_OFFSET_CELLS * lattice.spacing;
    Ok([
        probe.x + reach * direction[0],
        probe.y + reach * direction[1],
        z,
    ])
}

/// The parameters of one probe call: the step it reads, and the form it asks for.
fn probe(form: &'static str, ask: Value) -> Value {
    json!({(PARAM_STEP): STEP_NAME, (form): ask})
}

/// The three velocity components along a line, each its own call, from the start of the line.
/// The result view joins the three by the geometry of the line, so the ends and the count are
/// the same in all three.
fn velocity_line(
    from: [f64; 3],
    to: [f64; 3],
    count: u64,
    out: &mut Vec<Value>,
) -> Result<(), CompileFailure> {
    if !(2..=MAX_LINE_COUNT).contains(&count) {
        return Err(invalid(format!(
            "a line needs from 2 to {MAX_LINE_COUNT} samples, and this one has {count}"
        )));
    }
    for field in FIELD_VELOCITY {
        out.push(probe(
            PARAM_LINE,
            json!({
                (LINE_FROM): from,
                (LINE_TO): to,
                (LINE_COUNT): count,
                (LINE_FIELD): field,
            }),
        ));
    }
    Ok(())
}

/// The probe calls that follow the solve, in order: for a study with no shape the vertical then
/// the horizontal centreline; every velocity line of the monitor plan in plan order; when the plan
/// holds any probe, one point call for each frame carrying every probe point; one force call for
/// the body when a force of the plan is on it; one heat call for the box sides that hold a
/// heat-flux surface. Each is read at half the thickness of the slab. A monitor the result view
/// cannot serve (a pressure line, a flux or pressure surface, a force on a wall of the box) is
/// not asked for.
fn probes(study: &ResolvedStudy, lattice: &Lattice) -> Result<Vec<Value>, CompileFailure> {
    let z = lattice.slab() / 2.0;
    let plan = study.monitor_plan();
    let mut out = Vec::new();
    if study.domain.shape.is_none() {
        let (lx, ly) = (study.domain.length_x, study.domain.length_y);
        let count = study.monitors.centreline_u_sample_count;
        velocity_line([lx / 2.0, 0.0, z], [lx / 2.0, ly, z], count, &mut out)?;
        velocity_line([0.0, ly / 2.0, z], [lx, ly / 2.0, z], count, &mut out)?;
    }
    for line in &plan.lines {
        if line.field == MonitorField::Velocity {
            let (from, to) = ([line.x0, line.y0, z], [line.x1, line.y1, z]);
            velocity_line(from, to, line.samples, &mut out)?;
        }
    }
    if !plan.probes.is_empty() {
        let points = plan
            .probes
            .iter()
            .map(|planned| point_of(study, lattice, planned, z))
            .collect::<Result<Vec<_>, _>>()?;
        if points.len() > MAX_PROBE_POINTS {
            return Err(invalid(
                "a probe call takes more points than the solver allows",
            ));
        }
        for frame in 1..=frames(study, lattice) {
            let mut call = probe(PARAM_POINTS, json!(points));
            call[PARAM_FRAME] = json!(frame);
            out.push(call);
        }
    }
    let on_body = |name: &str| {
        study
            .boundary_conditions
            .get(name)
            .is_some_and(|boundary| boundary.faces == [Face::Obstacle])
    };
    if let Some(body) = &lattice.obstacle
        && plan.forces.iter().any(|force| on_body(&force.boundary))
    {
        out.push(probe(
            PARAM_FORCES,
            json!([format!("{}/{BODY_FACE}", body.name)]),
        ));
    }
    if study.physics.as_ref().is_some_and(|p| p.buoyancy.is_some()) {
        let held: Vec<&str> = plan
            .surfaces
            .iter()
            .filter(|surface| surface.quantity == SurfaceQuantity::HeatFlux)
            .filter_map(|surface| {
                match study
                    .boundary_conditions
                    .get(&surface.boundary)?
                    .faces
                    .as_slice()
                {
                    [face] => face_key(*face),
                    _ => None,
                }
            })
            .collect();
        let sides: Vec<&str> = ["x_min", "x_max", "y_min", "y_max"]
            .into_iter()
            .filter(|side| held.contains(side))
            .collect();
        if !sides.is_empty() {
            out.push(probe(PARAM_HEAT, json!(sides)));
        }
    }
    Ok(out)
}

/// The bytes of `commands.jsonl` for a study on its lattice.
fn stream_of(study: &ResolvedStudy, lattice: &Lattice) -> Result<Vec<u8>, CompileFailure> {
    let solve = json!({
        "command": SOLVE_COMMAND,
        "params": {(PARAM_STEP): STEP_NAME, "backend": "cpu"},
    });
    let mut lines = vec![
        request(
            1,
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "CFDCraft", "version": "1"},
            }),
        ),
        call(2, BATCH_TOOL, setup(study, lattice)?),
        call(3, EXECUTE_TOOL, solve),
    ];
    // Each probe is its own call, numbered on from the solve.
    for (id, params) in (4_u32..).zip(probes(study, lattice)?) {
        let command = json!({"command": PROBE_COMMAND, "params": params});
        lines.push(call(id, EXECUTE_TOOL, command));
    }
    let mut out = Vec::new();
    for line in &lines {
        let text = serde_json::to_string(line).map_err(|e| invalid(e.to_string()))?;
        if text.len() >= MAX_LINE_BYTES {
            return Err(invalid("a command line is too long for the guard"));
        }
        out.extend_from_slice(text.as_bytes());
        out.push(b'\n');
    }
    Ok(out)
}

/// The files of a case: the command stream, then the surface of the obstacle when there is one.
pub fn files(study: &ResolvedStudy) -> Result<Vec<CaseFile>, CompileFailure> {
    check_supported(study)?;
    let lattice = obstacles::lattice(study)?;
    let mut files = vec![CaseFile {
        rel_path: crate::plan::COMMANDS_FILE.into(),
        bytes: stream_of(study, &lattice)?,
    }];
    files.extend(obstacles::file_of(&lattice)?);
    Ok(files)
}
