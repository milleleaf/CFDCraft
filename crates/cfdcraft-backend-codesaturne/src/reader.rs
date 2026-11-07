//! A reader of the written case that shares no code with the writer, and the check of what it
//! reads against the study.
//!
//! The setup is read through the subset reader and the mesh through a Gmsh reader of its own. The
//! names the case should hold are spelled out here again, so a rename in the writer is a finding.

use crate::{
    guard::rel_name,
    markers::{ZONE_NAME_ATTRIBUTE, ZONE_NUMBER_ATTRIBUTE},
    xml::{self, Element},
};
use cfdcraft_backend::CaseTree;
use cfdcraft_model::{
    BoundaryKind, Diagnostic, Face, Location, ResolvedShape, ResolvedStudy, TurbulenceModel,
    inlet_turbulence, velocity_vector,
};

const HINT: &str = "The case does not match the study, so it must not be run. Compile it again; if this repeats, the writer has a fault.";

const SETUP: &str = "DATA/setup.xml";
const MESH: &str = "MESH/mesh.msh";

/// The most elements of a written mesh the reader holds in memory.
const MAX_ELEMENTS: usize = 64_000_000;

/// The side zones as the case should name them, with the face of the box each stands on.
const SIDE_ZONES: [(&str, Face); 4] = [
    ("x_min", Face::XMin),
    ("x_max", Face::XMax),
    ("y_min", Face::YMin),
    ("y_max", Face::YMax),
];

/// The zone of the two faces along z.
const Z_ZONE: &str = "z_symmetry";

/// The zone of the obstacle of a cylinder channel.
const OBSTACLE_ZONE: &str = "obstacle";

/// The zone of the symmetry sections of a start-up block.
const STARTUP_ZONE: &str = "startup";

/// The group of the cells.
const FLUID: &str = "fluid";

struct Findings {
    list: Vec<Diagnostic>,
}

impl Findings {
    fn located(&mut self, mut found: Diagnostic, file: &str, line: u32) {
        found.location = Some(Location {
            file: file.to_owned(),
            line: u64::from(line),
            entry: None,
        });
        self.list.push(found.with_hint(HINT));
    }

    fn differs(&mut self, setting: &str, pointer: &str, at: (&str, u32), values: (&str, &str)) {
        let (found, want) = values;
        let message =
            format!("The {setting} is written as {found} in the case, but the study has {want}.");
        self.located(
            Diagnostic::error("CFD-RT-001", setting, pointer, message),
            at.0,
            at.1,
        );
    }

    fn missing(&mut self, setting: &str, pointer: &str, file: &str, want: &str) {
        let message =
            format!("The {setting} was not found in the written case, but the study has {want}.");
        self.located(
            Diagnostic::error("CFD-RT-001", setting, pointer, message),
            file,
            1,
        );
    }

    fn unreadable(&mut self, file: &str, why: &str) {
        let message = format!("The written case file {file} could not be read back: {why}.");
        self.located(
            Diagnostic::error("CFD-RT-002", "case file", "", message),
            file,
            1,
        );
    }
}

fn file<'a>(tree: &'a CaseTree, name: &str) -> Option<&'a [u8]> {
    tree.files
        .iter()
        .find(|f| rel_name(f) == name)
        .map(|f| f.bytes.as_slice())
}

/// The text of a number as the case writes it must read back as the same number.
fn same_number(text: &str, want: f64) -> bool {
    let fold = |x: f64| if x == 0.0 { 0.0 } else { x };
    text.parse::<f64>()
        .is_ok_and(|got| fold(got).to_bits() == fold(want).to_bits())
}

/// Compare one number element with the value the study gives.
fn check_number(
    found: &mut Findings,
    setting: &str,
    pointer: &str,
    element: Option<&Element>,
    want: f64,
) {
    match element {
        None => found.missing(setting, pointer, SETUP, &want.to_string()),
        Some(e) if !same_number(&e.text, want) => found.differs(
            setting,
            pointer,
            (SETUP, e.line),
            (e.text.as_str(), &want.to_string()),
        ),
        Some(_) => {}
    }
}

/// Compare one text with the value the study gives.
fn check_text(
    found: &mut Findings,
    setting: &str,
    pointer: &str,
    got: Option<(&str, u32)>,
    want: &str,
) {
    match got {
        None => found.missing(setting, pointer, SETUP, want),
        Some((text, line)) if text != want => {
            found.differs(setting, pointer, (SETUP, line), (text, want));
        }
        Some(_) => {}
    }
}

fn property<'a>(root: &'a Element, name: &str) -> Option<&'a Element> {
    root.find(&["physical_properties", "fluid_properties"])?
        .children_named("property")
        .find(|p| p.attr("name") == Some(name))?
        .child("initial_value")
}

/// What a boundary of the study should be in the setup: the nature, the wall velocity and, for an
/// inlet, its speed.
struct ZoneWant {
    nature: &'static str,
    vector: [f64; 3],
    inlet_speed: Option<f64>,
}

fn expected_nature(kind: BoundaryKind, face: Face, speed: Option<f64>) -> ZoneWant {
    let (nature, vector, inlet_speed) = match kind {
        BoundaryKind::MovingWall => (
            "wall",
            velocity_vector(kind, face, speed.unwrap_or(0.0)),
            None,
        ),
        BoundaryKind::Symmetry => ("symmetry", [0.0; 3], None),
        BoundaryKind::VelocityInlet => ("inlet", [0.0; 3], speed),
        BoundaryKind::PressureOutlet => ("outlet", [0.0; 3], None),
        _ => ("wall", [0.0; 3], None),
    };
    ZoneWant {
        nature,
        vector,
        inlet_speed,
    }
}

/// The zones the setup should hold, with the code number each should carry: the four sides, the
/// obstacle of a cylinder channel, the start-up section of a step, and the faces along z.
fn expected_zones(study: &ResolvedStudy) -> Vec<(&'static str, usize, ZoneWant)> {
    let by_face = |face: Face| {
        study
            .boundary_conditions
            .values()
            .find(|b| b.faces.contains(&face))
    };
    let mut zones = Vec::new();
    let mut code = 0;
    let mut slot = |label: &'static str, want: Option<ZoneWant>| {
        code += 1;
        if let Some(want) = want {
            zones.push((label, code, want));
        }
    };
    for (label, face) in SIDE_ZONES {
        slot(
            label,
            by_face(face).map(|b| expected_nature(b.kind, face, b.velocity)),
        );
    }
    if matches!(
        study.domain.shape,
        Some(ResolvedShape::CylinderChannel { .. })
    ) {
        slot(
            OBSTACLE_ZONE,
            by_face(Face::Obstacle).map(|b| expected_nature(b.kind, Face::Obstacle, b.velocity)),
        );
    }
    if matches!(
        study.domain.shape,
        Some(ResolvedShape::BackwardStep { startup_length, .. }) if startup_length > 0.0
    ) {
        slot(
            STARTUP_ZONE,
            Some(expected_nature(BoundaryKind::Symmetry, Face::XMin, None)),
        );
    }
    slot(
        Z_ZONE,
        Some(expected_nature(BoundaryKind::Symmetry, Face::XMin, None)),
    );
    zones
}

/// The speed the inflow turbulence values are built from, as the study states it.
fn study_reference_speed(study: &ResolvedStudy) -> Option<f64> {
    let driven = |kind: BoundaryKind| {
        study
            .boundary_conditions
            .values()
            .filter(|b| b.kind == kind)
            .find_map(|b| b.velocity.filter(|v| *v != 0.0).map(f64::abs))
    };
    study
        .physics
        .as_ref()
        .and_then(|p| p.turbulence.reference_velocity)
        .or_else(|| driven(BoundaryKind::VelocityInlet))
        .or_else(|| driven(BoundaryKind::MovingWall))
}

/// The turbulence model of the study as the setup names it, or an empty text for a model the case
/// cannot hold.
fn expected_model(study: &ResolvedStudy) -> &'static str {
    match study
        .physics
        .as_ref()
        .map_or(TurbulenceModel::Laminar, |p| p.turbulence.model)
    {
        TurbulenceModel::Laminar => "off",
        TurbulenceModel::KOmegaSst => "k-omega-SST",
        _ => "",
    }
}

/// The number of an element, if it holds one.
fn number_of(element: Option<&Element>) -> Option<f64> {
    element.and_then(|e| e.text.parse::<f64>().ok())
}

/// The inflow values the solver derives from what the setup holds at an inlet: its relations for
/// the kinetic energy and the specific dissipation rate, spelled out here again from the
/// intensity in percent, the hydraulic diameter and the inlet speed.
fn derived_inflow(percent: f64, diameter: f64, speed: f64) -> (f64, f64) {
    let (cmu, kappa) = (0.09_f64, 0.42_f64);
    let k = 1.5 * (percent * 0.01 * speed).powi(2);
    let epsilon = 10.0 * cmu.powf(0.75) * k.powf(1.5) / (kappa * diameter);
    (k, epsilon / (cmu * k))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

fn check_inlet(
    found: &mut Findings,
    study: &ResolvedStudy,
    detail: &Element,
    setting: &str,
    speed: Option<f64>,
) {
    let pointer = "/boundary_conditions";
    let velocity = detail.child("velocity_pressure");
    for (key, want) in [("choice", "norm"), ("direction", "normal")] {
        let got = velocity.and_then(|v| v.attr(key).map(|c| (c, v.line)));
        check_text(
            found,
            &format!("{setting} velocity {key}"),
            pointer,
            got,
            want,
        );
    }
    let Some(speed) = speed else { return };
    check_number(
        found,
        &format!("{setting} velocity"),
        pointer,
        velocity.and_then(|v| v.child("norm")),
        speed,
    );
    let model = expected_model(study);
    if model == "off" || model.is_empty() {
        return;
    }
    let turbulence = detail.child("turbulence");
    let choice = turbulence.and_then(|t| t.attr("choice").map(|c| (c, t.line)));
    check_text(
        found,
        &format!("{setting} turbulence choice"),
        pointer,
        choice,
        "turbulent_intensity",
    );
    let physics = study.physics.as_ref().map(|p| &p.turbulence);
    let want = match (
        physics.and_then(|t| t.intensity),
        physics.and_then(|t| t.viscosity_ratio),
        study_reference_speed(study),
    ) {
        (Some(intensity), Some(ratio), Some(reference)) => {
            inlet_turbulence(intensity, ratio, reference, study.fluid.kinematic_viscosity)
        }
        _ => None,
    };
    let percent = number_of(turbulence.and_then(|t| t.child("turbulent_intensity")));
    let diameter = number_of(turbulence.and_then(|t| t.child("hydraulic_diameter")));
    let (Some(want), Some(percent), Some(diameter)) = (want, percent, diameter) else {
        found.missing(
            &format!("{setting} turbulence"),
            pointer,
            SETUP,
            "an intensity and a hydraulic diameter that give the study's inflow values",
        );
        return;
    };
    let (k, omega) = derived_inflow(percent, diameter, speed);
    for (name, got, wanted) in [
        ("turbulent kinetic energy", k, want.k),
        ("specific dissipation rate", omega, want.omega),
    ] {
        if !close(got, wanted) {
            found.differs(
                &format!("{setting} {name}"),
                pointer,
                (SETUP, turbulence.map_or(1, |t| t.line)),
                (&got.to_string(), &wanted.to_string()),
            );
        }
    }
}

fn check_zone(
    found: &mut Findings,
    study: &ResolvedStudy,
    root: &Element,
    zone: (&str, usize, ZoneWant),
) {
    let (label, code, want) = zone;
    let conditions = root.child("boundary_conditions");
    let zone = conditions.and_then(|c| {
        c.children_named("boundary")
            .find(|b| b.attr(ZONE_NAME_ATTRIBUTE) == Some(label))
    });
    let setting = format!("boundary zone {label}");
    let pointer = "/boundary_conditions";
    let Some(zone) = zone else {
        found.missing(&setting, pointer, SETUP, &format!("a zone named {label}"));
        return;
    };
    let at = (SETUP, zone.line);
    let nature = zone.attr("nature").unwrap_or("");
    if nature != want.nature {
        found.differs(&setting, pointer, at, (nature, want.nature));
    }
    let number = zone.attr(ZONE_NUMBER_ATTRIBUTE).unwrap_or("");
    if number != code.to_string() {
        found.differs(&setting, pointer, at, (number, &code.to_string()));
    }
    if zone.text != label {
        found.differs(&setting, pointer, at, (zone.text.as_str(), label));
    }
    let detail = conditions.and_then(|c| {
        c.children_named(want.nature)
            .find(|d| d.attr(ZONE_NAME_ATTRIBUTE) == Some(label))
    });
    let Some(detail) = detail else {
        found.missing(&setting, pointer, SETUP, &format!("a {} node", want.nature));
        return;
    };
    if want.nature == "inlet" {
        check_inlet(found, study, detail, &setting, want.inlet_speed);
        return;
    }
    if want.nature != "wall" {
        return;
    }
    let Some(pressure) = detail.child("velocity_pressure") else {
        found.missing(&setting, pointer, SETUP, "a wall velocity setting");
        return;
    };
    let moving = want.vector != [0.0; 3];
    let choice = pressure.attr("choice").unwrap_or("");
    let want_choice = if moving { "on" } else { "off" };
    if choice != want_choice {
        found.differs(
            &setting,
            pointer,
            (SETUP, pressure.line),
            (choice, want_choice),
        );
    }
    if moving {
        for (component, value) in want.vector.iter().enumerate() {
            let text = component.to_string();
            let element = pressure
                .children_named("dirichlet")
                .find(|d| d.attr("component") == Some(text.as_str()));
            check_number(
                found,
                &format!("{setting} velocity"),
                pointer,
                element,
                *value,
            );
        }
    }
}

/// A count written as an integer text must equal `want` rounded, and `want` must be that whole
/// number up to rounding of the division that produced it.
fn check_whole(
    found: &mut Findings,
    setting: &str,
    pointer: &str,
    element: Option<&Element>,
    want: f64,
) {
    let nearest = want.round();
    let exact = (want - nearest).abs() <= 1e-9 * nearest.abs();
    match element {
        None => found.missing(setting, pointer, SETUP, &want.to_string()),
        Some(e) if !(exact && e.text.parse::<f64>().is_ok_and(|got| got == nearest)) => found
            .differs(
                setting,
                pointer,
                (SETUP, e.line),
                (e.text.as_str(), &want.to_string()),
            ),
        Some(_) => {}
    }
}

/// The distinct values of a coordinate over the nodes of the lower layer, ascending.
fn layer_coordinates(mesh: &MeshFacts, axis: usize, scale: f64) -> Vec<f64> {
    let mut found: Vec<f64> = mesh
        .nodes
        .iter()
        .filter(|n| n[2].abs() <= 1e-9 * scale)
        .map(|n| n[axis])
        .collect();
    found.sort_by(f64::total_cmp);
    found.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * scale);
    found
}

/// The monitoring points the study should have in the setup, in order: the two centre lines of a
/// box at the nodes of the mesh, then the study's lines and points.
fn expected_points(study: &ResolvedStudy, mesh: &MeshFacts) -> Vec<[f64; 3]> {
    let (lx, ly, lz) = (
        study.domain.length_x,
        study.domain.length_y,
        study.domain.length_z,
    );
    let z = lz / 2.0;
    let mut points = Vec::new();
    if study.domain.shape.is_none() {
        points.extend(
            layer_coordinates(mesh, 1, lx.max(ly))
                .into_iter()
                .map(|y| [lx / 2.0, y, z]),
        );
        points.extend(
            layer_coordinates(mesh, 0, lx.max(ly))
                .into_iter()
                .map(|x| [x, ly / 2.0, z]),
        );
    }
    let plan = study.monitor_plan();
    for line in &plan.lines {
        let last = line.samples.saturating_sub(1).max(1) as f64;
        for i in 0..line.samples {
            let t = i as f64 / last;
            points.push([
                line.x0 + (line.x1 - line.x0) * t,
                line.y0 + (line.y1 - line.y0) * t,
                z,
            ]);
        }
    }
    for probe in &plan.probes {
        points.push([probe.x, probe.y, z]);
    }
    points
}

/// The output node: the points, how often they are recorded, and how.
fn check_output(found: &mut Findings, study: &ResolvedStudy, root: &Element, mesh: &MeshFacts) {
    let Some(output) = root.find(&["analysis_control", "output"]) else {
        found.missing("monitoring points", "/monitors", SETUP, "an output node");
        return;
    };
    let want = expected_points(study, mesh);
    let got: Vec<&Element> = output.children_named("probe").collect();
    if got.len() != want.len() {
        found.differs(
            "number of monitoring points",
            "/monitors",
            (SETUP, output.line),
            (&got.len().to_string(), &want.len().to_string()),
        );
    } else {
        let scale = study
            .domain
            .length_x
            .max(study.domain.length_y)
            .max(study.domain.length_z);
        for (index, (probe, point)) in got.iter().zip(&want).enumerate() {
            let id = (index + 1).to_string();
            if probe.attr("id") != Some(id.as_str()) || probe.attr("name") != Some(id.as_str()) {
                found.differs(
                    "monitoring point number",
                    "/monitors",
                    (SETUP, probe.line),
                    (probe.attr("id").unwrap_or(""), &id),
                );
            }
            for (tag, want) in ["probe_x", "probe_y", "probe_z"].into_iter().zip(point) {
                let at = number_of(probe.child(tag));
                if !at.is_some_and(|got| within(got, *want, scale)) {
                    found.differs(
                        &format!("monitoring point {id} {tag}"),
                        "/monitors",
                        (SETUP, probe.line),
                        (
                            &probe.child(tag).map_or(String::new(), |e| e.text.clone()),
                            &want.to_string(),
                        ),
                    );
                    break;
                }
            }
        }
    }
    let step = study.effective_delta_t().unwrap_or(1.0);
    check_whole(
        found,
        "recording frequency",
        "/solver/write_interval",
        output.child("probe_recording_frequency"),
        study.solver.write_interval / step,
    );
    for (tag, want) in [
        ("probe_format", "CSV"),
        ("probes_snap", "none"),
        ("probes_interpolation", "yes"),
    ] {
        let at = output
            .child(tag)
            .and_then(|e| e.attr("choice").map(|c| (c, e.line)));
        check_text(found, tag, "/monitors", at, want);
    }
    for name in ["velocity", "pressure"] {
        let variable = root
            .find(&["thermophysical_models", "velocity_pressure"])
            .and_then(|v| {
                v.children_named("variable")
                    .find(|n| n.attr("name") == Some(name))
            });
        let status = variable
            .and_then(|v| v.child("probes_recording"))
            .and_then(|r| r.attr("status").map(|s| (s, r.line)));
        check_text(
            found,
            &format!("{name} recording"),
            "/monitors",
            status,
            "on",
        );
    }
}

fn check_setup(
    found: &mut Findings,
    study: &ResolvedStudy,
    bytes: &[u8],
    mesh: Option<&MeshFacts>,
) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        found.unreadable(SETUP, "it is not valid text");
        return;
    };
    let root = match xml::parse(text) {
        Ok(root) => root,
        Err(error) => {
            found.unreadable(SETUP, &error.to_string());
            return;
        }
    };
    if root.name != "Code_Saturne_GUI" || root.attr("version") != Some("2.0") {
        found.unreadable(
            SETUP,
            "the root element is not a setup of the expected version",
        );
        return;
    }
    check_number(
        found,
        "fluid density",
        "/fluid",
        property(&root, "density"),
        1.0,
    );
    check_number(
        found,
        "kinematic viscosity",
        "/fluid/kinematic_viscosity",
        property(&root, "molecular_viscosity"),
        study.fluid.kinematic_viscosity,
    );
    let time = root.find(&["analysis_control", "time_parameters"]);
    let step = study.effective_delta_t();
    check_whole(
        found,
        "iteration count",
        "/solver/end_time",
        time.and_then(|t| t.child("iterations")),
        step.map_or(study.solver.end_time, |dt| study.solver.end_time / dt),
    );
    let passing = time.and_then(|t| t.child("time_passing"));
    check_text(
        found,
        "time stepping",
        "/physics/time",
        passing.map(|e| (e.text.as_str(), e.line)),
        if step.is_some() { "0" } else { "-1" },
    );
    if let Some(dt) = step {
        check_number(
            found,
            "time step",
            "/physics/time",
            time.and_then(|t| t.child("time_step_ref")),
            dt,
        );
    }
    let turbulence = root.find(&["thermophysical_models", "turbulence"]);
    let model = turbulence.and_then(|t| t.attr("model").map(|m| (m, t.line)));
    let want_model = expected_model(study);
    check_text(
        found,
        "turbulence model",
        "/physics/turbulence/model",
        model,
        want_model,
    );
    if want_model != "off"
        && let Some(speed) = study_reference_speed(study)
    {
        check_number(
            found,
            "turbulence reference velocity",
            "/physics/turbulence/reference_velocity",
            turbulence.and_then(|t| t.child("reference_velocity")),
            speed,
        );
    }
    let algorithm = root
        .find(&["numerical_parameters", "velocity_pressure_algo"])
        .and_then(|a| a.attr("choice").map(|c| (c, a.line)));
    check_text(
        found,
        "pressure coupling",
        "/physics",
        algorithm,
        if step.is_some() { "simplec" } else { "simple" },
    );
    let meshes = root.find(&["solution_domain", "meshes_list"]);
    let dir = meshes
        .and_then(|m| m.child("meshdir"))
        .and_then(|d| d.attr("name").map(|n| (n, d.line)));
    check_text(found, "mesh folder", "/mesh", dir, "MESH");
    let name = meshes
        .and_then(|m| m.child("mesh"))
        .and_then(|d| d.attr("name").map(|n| (n, d.line)));
    check_text(found, "mesh file", "/mesh", name, "mesh.msh");

    for zone in expected_zones(study) {
        check_zone(found, study, &root, zone);
    }
    if let Some(mesh) = mesh {
        check_output(found, study, &root, mesh);
    }
}

/// One element line of a Gmsh file: type, physical group, node numbers.
struct Element3 {
    kind: u32,
    group: u32,
    nodes: Vec<u32>,
}

struct MeshFacts {
    names: Vec<(u32, u32, String)>,
    nodes: Vec<[f64; 3]>,
    elements: Vec<Element3>,
}

fn bad(what: &str) -> String {
    what.to_owned()
}

fn count_line(line: Option<&str>) -> Result<usize, String> {
    line.and_then(|l| l.trim().parse::<usize>().ok())
        .ok_or_else(|| bad("a section does not start with its count"))
}

fn parse_mesh(text: &str) -> Result<MeshFacts, String> {
    let mut lines = text.lines();
    let mut names = Vec::new();
    let mut nodes: Vec<[f64; 3]> = Vec::new();
    let mut elements = Vec::new();
    let mut format_seen = false;
    while let Some(line) = lines.next() {
        match line.trim() {
            "$MeshFormat" => {
                let version = lines.next().unwrap_or("");
                if !version.starts_with("2.2 ") {
                    return Err(bad("the mesh is not in the Gmsh 2.2 text format"));
                }
                format_seen = true;
            }
            "$PhysicalNames" => {
                for _ in 0..count_line(lines.next())? {
                    let line = lines.next().ok_or_else(|| bad("a group name is missing"))?;
                    let mut parts = line.splitn(3, ' ');
                    let dim = parts.next().and_then(|p| p.parse::<u32>().ok());
                    let id = parts.next().and_then(|p| p.parse::<u32>().ok());
                    let name = parts.next().map(|p| p.trim_matches('"').to_owned());
                    let (Some(dim), Some(id), Some(name)) = (dim, id, name) else {
                        return Err(bad("a group name line is malformed"));
                    };
                    names.push((dim, id, name));
                }
            }
            "$Nodes" => {
                let count = count_line(lines.next())?;
                if count > MAX_ELEMENTS {
                    return Err(bad("the mesh has too many nodes to read back"));
                }
                for expected in 1..=count {
                    let line = lines.next().ok_or_else(|| bad("a node line is missing"))?;
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    let [id, x, y, z] = parts[..] else {
                        return Err(bad("a node line is malformed"));
                    };
                    let coordinates = [x, y, z].map(|c| c.parse::<f64>().ok());
                    let [Some(x), Some(y), Some(z)] = coordinates else {
                        return Err(bad("a node coordinate is not a number"));
                    };
                    if id.parse::<usize>().ok() != Some(expected) {
                        return Err(bad("the nodes are not numbered in order"));
                    }
                    nodes.push([x, y, z]);
                }
            }
            "$Elements" => {
                let count = count_line(lines.next())?;
                if count > MAX_ELEMENTS {
                    return Err(bad("the mesh has too many elements to read back"));
                }
                for expected in 1..=count {
                    let line = lines
                        .next()
                        .ok_or_else(|| bad("an element line is missing"))?;
                    let numbers: Vec<u32> = line
                        .split_whitespace()
                        .map(str::parse::<u32>)
                        .collect::<Result<_, _>>()
                        .map_err(|_| bad("an element line holds something other than numbers"))?;
                    let [id, kind, tags, rest @ ..] = &numbers[..] else {
                        return Err(bad("an element line is too short"));
                    };
                    let tags = *tags as usize;
                    if usize::try_from(*id).ok() != Some(expected) || rest.len() <= tags || tags < 1
                    {
                        return Err(bad("an element line is malformed"));
                    }
                    elements.push(Element3 {
                        kind: *kind,
                        group: rest[0],
                        nodes: rest[tags..].to_vec(),
                    });
                }
            }
            _ => {}
        }
    }
    if !format_seen {
        return Err(bad("the mesh has no format section"));
    }
    Ok(MeshFacts {
        names,
        nodes,
        elements,
    })
}

fn within(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-9 * scale
}

fn check_count(found: &mut Findings, setting: &str, got: u64, want: u64) {
    if got != want {
        found.differs(
            setting,
            "/mesh",
            (MESH, 1),
            (&got.to_string(), &want.to_string()),
        );
    }
}

/// Read the mesh text, or say why it cannot be read.
fn read_mesh(found: &mut Findings, bytes: &[u8]) -> Option<MeshFacts> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        found.unreadable(MESH, "it is not valid text");
        return None;
    };
    match parse_mesh(text) {
        Ok(mesh) => Some(mesh),
        Err(why) => {
            found.unreadable(MESH, &why);
            None
        }
    }
}

/// The groups of the mesh in the order of their numbers: the four sides, the obstacle of a
/// cylinder channel, the start-up section of a step, the faces along z, the cells.
fn expected_groups(study: &ResolvedStudy) -> Vec<(&'static str, u32)> {
    let mut groups: Vec<(&'static str, u32)> =
        SIDE_ZONES.iter().map(|(label, _)| (*label, 2)).collect();
    if matches!(
        study.domain.shape,
        Some(ResolvedShape::CylinderChannel { .. })
    ) {
        groups.push((OBSTACLE_ZONE, 2));
    }
    if matches!(
        study.domain.shape,
        Some(ResolvedShape::BackwardStep { startup_length, .. }) if startup_length > 0.0
    ) {
        groups.push((STARTUP_ZONE, 2));
    }
    groups.push((Z_ZONE, 2));
    groups.push((FLUID, 3));
    groups
}

fn check_mesh(found: &mut Findings, study: &ResolvedStudy, mesh: &MeshFacts) {
    let nx = study.mesh.cells_x;
    let ny = study.mesh.cells_y;
    let nz = study.mesh.cells_z;
    let box_domain = study.domain.shape.is_none();
    let cells = if box_domain {
        Some(nx * ny * nz)
    } else {
        study.cell_count().map(|n| n * nz)
    };
    if box_domain {
        check_count(
            found,
            "number of mesh nodes",
            mesh.nodes.len() as u64,
            2 * (nx + 1) * (ny + 1),
        );
    }
    let of_kind = |kind: u32| mesh.elements.iter().filter(|e| e.kind == kind).count() as u64;
    if let Some(cells) = cells {
        check_count(found, "number of cells", of_kind(5), cells);
    }
    for (index, (label, dim)) in expected_groups(study).into_iter().enumerate() {
        let id = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let named = mesh
            .names
            .iter()
            .find(|(d, i, n)| *i == id && n == label && *d == dim);
        if named.is_none() {
            found.missing(
                "mesh group",
                "/mesh",
                MESH,
                &format!("a group named {label}"),
            );
        }
    }
    let z_id = u32::try_from(expected_groups(study).len() - 1).unwrap_or(u32::MAX);
    let faces = |id: u32| {
        mesh.elements
            .iter()
            .filter(|e| e.kind == 3 && e.group == id)
            .count() as u64
    };
    if box_domain {
        let sides = [ny, ny, nx, nx];
        for (index, want) in sides.iter().enumerate() {
            let id = u32::try_from(index + 1).unwrap_or(u32::MAX);
            check_count(found, "faces of a mesh side", faces(id), *want);
        }
    }
    if let Some(cells) = cells {
        check_count(found, "faces along z", faces(z_id), 2 * cells);
    }

    let count = mesh.nodes.len();
    let in_range = mesh
        .elements
        .iter()
        .all(|e| e.nodes.iter().all(|n| (1..=count).contains(&(*n as usize))));
    if !in_range {
        found.differs(
            "mesh connectivity",
            "/mesh",
            (MESH, 1),
            (
                "a node number outside the mesh",
                "node numbers inside the mesh",
            ),
        );
    }
    let extent = |axis: usize| {
        mesh.nodes
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), n| {
                (lo.min(n[axis]), hi.max(n[axis]))
            })
    };
    for (axis, setting, length) in [
        (0, "domain length in x", study.domain.length_x),
        (1, "domain length in y", study.domain.length_y),
        (2, "domain length in z", study.domain.length_z),
    ] {
        let (lo, hi) = extent(axis);
        if count > 0 && !(within(lo, 0.0, length) && within(hi, length, length)) {
            found.differs(
                setting,
                &format!("/domain/length_{}", ["x", "y", "z"][axis]),
                (MESH, 1),
                (&format!("{lo} to {hi}"), &format!("0 to {length}")),
            );
        }
    }
}

/// The values of the written case read back through a reader of its own and compared with the
/// study; empty when every value matches.
pub fn read_back(study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic> {
    let mut found = Findings { list: Vec::new() };
    let mesh = match file(tree, MESH) {
        Some(bytes) => read_mesh(&mut found, bytes),
        None => {
            found.unreadable(MESH, "the file is missing");
            None
        }
    };
    match file(tree, SETUP) {
        Some(bytes) => check_setup(&mut found, study, bytes, mesh.as_ref()),
        None => found.unreadable(SETUP, "the file is missing"),
    }
    if let Some(mesh) = &mesh {
        check_mesh(&mut found, study, mesh);
    }
    found.list
}
