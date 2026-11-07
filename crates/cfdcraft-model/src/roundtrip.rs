//! Round-trip check: every value the study resolved to is read back from the bytes of the written
//! case and compared exactly.
//!
//! The check lives here and knows nothing about any solver's file format. A dialect supplies a
//! [`CaseReader`] that turns written files into a flat map of fact path to value; this module
//! decides what the map must contain and says what differs. It does no I/O.

use std::{collections::BTreeMap, fmt};

use serde_json::Value;

use crate::{
    Diagnostic, Location, ResolvedStudy,
    diagnostics::{kind_name, pointer_token, setting_label, side_name},
    study::{Face, MonitorField, SurfaceQuantity},
};

/// The kind written for the front and back faces, which every two-dimensional case leaves empty.
pub const EMPTY_KIND: &str = "empty";

/// Resolved-study leaves that no case file holds, and the reason each is checked elsewhere.
///
/// - `/schema_version` is carried by the study hash; no case file records it.
/// - `/solver/convergence` is read only by the evidence judge. The written solver settings use a
///   constant tolerance with no residual control, so the limits cannot be recovered from the case.
/// - `/monitors/settle` holds tolerances for judging a finished run; no case file carries them.
///
/// Settings the study does not parameterise (schemes, solver choices, relaxation) are covered by
/// the golden case bytes, not by facts.
pub const EVIDENCE_ONLY: &[&str] = &["/schema_version", "/solver/convergence", "/monitors/settle"];

/// A written file, by path relative to the case root with `/` separators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A byte range of a written file. `line` is the line the range starts on, counted from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub file: String,
    pub line: u64,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FactValue {
    Number(f64),
    Count(u64),
    Text(String),
    Flag(bool),
}

impl fmt::Display for FactValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(v) => write!(f, "{v}"),
            Self::Count(n) => write!(f, "{n}"),
            Self::Text(t) => f.write_str(&clip(t)),
            Self::Flag(b) => write!(f, "{b}"),
        }
    }
}

/// One value recovered from the written case, with where it was found.
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub value: FactValue,
    /// The tokens that carry the value. Overwriting any one of them changes what is read back.
    pub value_spans: Vec<Span>,
    /// The whole entry that holds the value. Removing it removes the fact.
    pub entry_span: Option<Span>,
}

/// A written file that cannot be read back with confidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError {
    pub file: String,
    pub line: u64,
    pub message: String,
}

/// Turns the written files of one dialect into the normalised map of fact path to value.
///
/// Facts are keyed by neutral paths such as `/fluid/kinematic_viscosity`; boundaries are keyed by
/// the box face they sit on, `/boundary_conditions/by_face/<Face>/kind`, so a reader needs no
/// knowledge of the study's labels or the writer's patch names. A reader must parse the bytes
/// itself and share no code with the writer. A fact that is absent from the files is absent from
/// the map; a file that cannot be understood is a [`ReadError`].
pub trait CaseReader {
    fn read(&self, files: &[SourceFile]) -> Result<BTreeMap<String, Fact>, ReadError>;
}

/// A fact the written case must hold, derived from the resolved study alone.
#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub key: String,
    pub value: FactValue,
    /// The resolved-study leaves this fact covers. Empty for a value fixed by the backend.
    pub source_pointers: Vec<String>,
    /// The setting in the user's words.
    pub setting: String,
}

fn expected(key: String, value: FactValue, source_pointers: Vec<String>) -> Expected {
    let setting = source_pointers
        .first()
        .map_or_else(|| key_label(&key), |pointer| setting_label(pointer));
    Expected {
        key,
        value,
        source_pointers,
        setting,
    }
}

/// A monitor fact. `listed` is the study list that holds the monitor, or `None` for the default
/// probes, which the resolved study does not hold.
fn monitor_fact(key: String, value: FactValue, listed: Option<&str>) -> Expected {
    let setting = key_label(&key);
    Expected {
        key,
        value,
        source_pointers: listed.map_or_else(Vec::new, |pointer| vec![pointer.to_owned()]),
        setting,
    }
}

fn field_word(field: MonitorField) -> &'static str {
    match field {
        MonitorField::Velocity => "velocity",
        MonitorField::Pressure => "pressure",
    }
}

fn quantity_word(quantity: SurfaceQuantity) -> &'static str {
    match quantity {
        SurfaceQuantity::Flux => "flux",
        SurfaceQuantity::PressureAverage => "pressure_average",
        SurfaceQuantity::PressureIntegral => "pressure_integral",
    }
}

/// The number a function-object name ends with: `probe_3` gives `3`.
fn monitor_number(fo: &str) -> &str {
    fo.rsplit('_').next().unwrap_or(fo)
}

/// The probes, lines and surfaces the case must watch. The names are positions, so a monitor is
/// found by its number and the study's label stays out of the case.
fn monitor_facts(study: &ResolvedStudy, out: &mut Vec<Expected>) {
    let plan = study.monitor_plan();
    let listed = |list: &'static str| (!plan.defaulted).then_some(list);
    for probe in &plan.probes {
        let n = monitor_number(&probe.fo);
        let from = listed("/monitors/probes");
        for (axis, value) in [("x", probe.x), ("y", probe.y)] {
            out.push(monitor_fact(
                format!("/monitors/probe/{n}/{axis}"),
                FactValue::Number(value),
                from,
            ));
        }
        out.push(monitor_fact(
            format!("/monitors/probe/{n}/field"),
            FactValue::Text(field_word(probe.field).into()),
            from,
        ));
    }
    for line in &plan.lines {
        let n = monitor_number(&line.fo);
        let from = listed("/monitors/lines");
        for (name, value) in [
            ("x0", line.x0),
            ("y0", line.y0),
            ("x1", line.x1),
            ("y1", line.y1),
        ] {
            out.push(monitor_fact(
                format!("/monitors/line/{n}/{name}"),
                FactValue::Number(value),
                from,
            ));
        }
        out.push(monitor_fact(
            format!("/monitors/line/{n}/samples"),
            FactValue::Count(line.samples),
            from,
        ));
        out.push(monitor_fact(
            format!("/monitors/line/{n}/field"),
            FactValue::Text(field_word(line.field).into()),
            from,
        ));
    }
    for surface in &plan.surfaces {
        let n = monitor_number(&surface.fo);
        let from = listed("/monitors/surfaces");
        out.push(monitor_fact(
            format!("/monitors/surface/{n}/kind"),
            FactValue::Text(kind_name(surface.kind).into()),
            from,
        ));
        out.push(monitor_fact(
            format!("/monitors/surface/{n}/quantity"),
            FactValue::Text(quantity_word(surface.quantity).into()),
            from,
        ));
    }
}

/// Every fact the written case must hold for `study`, sorted by key.
pub fn expected_facts(study: &ResolvedStudy) -> Vec<Expected> {
    let mut out = Vec::new();
    let axes = [
        ("x", study.domain.length_x, study.mesh.cells_x),
        ("y", study.domain.length_y, study.mesh.cells_y),
        ("z", study.domain.length_z, study.mesh.cells_z),
    ];
    for (axis, length, cells) in axes {
        out.push(expected(
            format!("/domain/length_{axis}"),
            FactValue::Number(length),
            vec![format!("/domain/length_{axis}")],
        ));
        out.push(expected(
            format!("/domain/origin_{axis}"),
            FactValue::Number(0.0),
            vec![],
        ));
        out.push(expected(
            format!("/mesh/cells_{axis}"),
            FactValue::Count(cells),
            vec![format!("/mesh/cells_{axis}")],
        ));
    }
    out.push(expected(
        "/domain/open".into(),
        FactValue::Flag(study.domain.open),
        vec!["/domain/open".into()],
    ));
    out.push(expected(
        "/mesh/grading".into(),
        FactValue::Text("1 1 1".into()),
        vec![],
    ));
    out.push(expected(
        "/fluid/kinematic_viscosity".into(),
        FactValue::Number(study.fluid.kinematic_viscosity),
        vec!["/fluid/kinematic_viscosity".into()],
    ));
    out.push(expected(
        "/solver/end_time".into(),
        FactValue::Number(study.solver.end_time),
        vec!["/solver/end_time".into()],
    ));
    out.push(expected(
        "/solver/write_interval".into(),
        FactValue::Number(study.solver.write_interval),
        vec!["/solver/write_interval".into()],
    ));
    out.push(expected(
        "/solver/application".into(),
        FactValue::Text("simpleFoam".into()),
        vec![],
    ));
    out.push(expected(
        "/solver/turbulence".into(),
        FactValue::Text("laminar".into()),
        vec![],
    ));
    for profile in ["vertical", "horizontal"] {
        out.push(expected(
            format!("/monitors/centreline_u_sample_count/{profile}"),
            FactValue::Count(study.monitors.centreline_u_sample_count),
            vec!["/monitors/centreline_u_sample_count".into()],
        ));
    }
    monitor_facts(study, &mut out);
    for (name, boundary) in &study.boundary_conditions {
        let base = format!("/boundary_conditions/{}", pointer_token(name));
        for face in &boundary.faces {
            out.push(expected(
                format!("/boundary_conditions/by_face/{face:?}/kind"),
                FactValue::Text(kind_name(boundary.kind).into()),
                vec![format!("{base}/kind"), format!("{base}/faces")],
            ));
            if let Some(speed) = boundary.velocity {
                out.push(expected(
                    format!("/boundary_conditions/by_face/{face:?}/velocity"),
                    FactValue::Number(speed),
                    vec![format!("{base}/velocity")],
                ));
            }
        }
    }
    for face in ["ZMin", "ZMax"] {
        out.push(expected(
            format!("/boundary_conditions/by_face/{face}/kind"),
            FactValue::Text(EMPTY_KIND.into()),
            vec![],
        ));
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// Leaves of the resolved study (as JSON) that no expected fact covers and that are not on the
/// evidence-only list. Empty means a new resolved field cannot be forgotten.
pub fn uncovered_leaves(resolved: &Value, expected: &[Expected]) -> Vec<String> {
    let covered: Vec<&str> = expected
        .iter()
        .flat_map(|e| e.source_pointers.iter().map(String::as_str))
        .chain(EVIDENCE_ONLY.iter().copied())
        .collect();
    let mut out = Vec::new();
    walk(resolved, String::new(), &covered, &mut out);
    out
}

fn walk(value: &Value, path: String, covered: &[&str], out: &mut Vec<String>) {
    if !path.is_empty()
        && covered
            .iter()
            .any(|c| path == *c || path.strip_prefix(c).is_some_and(|r| r.starts_with('/')))
    {
        return;
    }
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                walk(
                    child,
                    format!("{path}/{}", pointer_token(key)),
                    covered,
                    out,
                );
            }
        }
        _ => out.push(path),
    }
}

/// Compare what the reader recovers from `files` with what `study` requires. Empty means every
/// case-resident value was read back and matches exactly. Fails closed: a file the reader cannot
/// understand, a value that is missing and a value the study does not account for are all errors.
pub fn check_roundtrip(
    reader: &dyn CaseReader,
    files: &[SourceFile],
    study: &ResolvedStudy,
) -> Vec<Diagnostic> {
    let facts = match reader.read(files) {
        Ok(facts) => facts,
        Err(error) => return vec![unreadable(&error)],
    };
    let wanted = expected_facts(study);
    let mut found = Vec::new();
    for want in &wanted {
        match facts.get(&want.key) {
            None => found.push(
                diagnostic(
                    want,
                    format!(
                        "The {}{} was not found in the written case, but the study has {}.",
                        want.setting,
                        place(&want.key),
                        want.value
                    ),
                    None,
                    files,
                )
                .with_hint(NOT_RUNNABLE),
            ),
            Some(fact) if !same(&want.value, &fact.value) => found.push(
                diagnostic(
                    want,
                    format!(
                        "The {}{} is written as {} in the case, but the study has {}.",
                        want.setting,
                        place(&want.key),
                        fact.value,
                        want.value
                    ),
                    Some(fact),
                    files,
                )
                .with_hint(NOT_RUNNABLE),
            ),
            Some(_) => {}
        }
    }
    for (key, fact) in &facts {
        if wanted.iter().any(|w| &w.key == key) {
            continue;
        }
        let mut d = Diagnostic::error(
            "CFD-RT-001",
            key_label(key),
            "",
            format!(
                "The written case holds a {}{} of {} that the study does not account for.",
                key_label(key),
                place(key),
                fact.value
            ),
        )
        .with_hint(NOT_RUNNABLE);
        d.location = location(fact, files);
        found.push(d);
    }
    found
}

const NOT_RUNNABLE: &str = "The case does not match the study, so it must not be run. Compile it again; if this repeats, the writer has a fault.";

fn unreadable(error: &ReadError) -> Diagnostic {
    let mut d = Diagnostic::error(
        "CFD-RT-002",
        "case file",
        "",
        format!(
            "The written case file {} could not be read back: {}.",
            error.file, error.message
        ),
    )
    .with_hint(NOT_RUNNABLE);
    d.location = Some(Location {
        file: error.file.clone(),
        line: error.line,
        entry: None,
    });
    d
}

fn diagnostic(
    want: &Expected,
    message: String,
    fact: Option<&Fact>,
    files: &[SourceFile],
) -> Diagnostic {
    let pointer = want.source_pointers.first().cloned().unwrap_or_default();
    let mut d = Diagnostic::error("CFD-RT-001", want.setting.clone(), pointer, message);
    d.location = fact.and_then(|f| location(f, files));
    d
}

fn location(fact: &Fact, files: &[SourceFile]) -> Option<Location> {
    let at = fact.value_spans.first().or(fact.entry_span.as_ref())?;
    let entry = fact.entry_span.as_ref().and_then(|span| {
        let file = files.iter().find(|f| f.path == span.file)?;
        let bytes = file.bytes.get(span.start..span.end.min(file.bytes.len()))?;
        Some(clip(String::from_utf8_lossy(bytes).trim()))
    });
    Some(Location {
        file: at.file.clone(),
        line: at.line,
        entry,
    })
}

/// Exact comparison. Numbers match bit for bit after folding negative zero into zero, because the
/// writer prints the shortest decimal that reads back to the same number.
fn same(want: &FactValue, found: &FactValue) -> bool {
    let fold = |x: f64| if x == 0.0 { 0.0_f64 } else { x };
    match (want, found) {
        (FactValue::Number(a), FactValue::Number(b)) => fold(*a).to_bits() == fold(*b).to_bits(),
        (FactValue::Count(a), FactValue::Count(b)) => a == b,
        (FactValue::Text(a), FactValue::Text(b)) => a == b,
        (FactValue::Flag(a), FactValue::Flag(b)) => a == b,
        _ => false,
    }
}

fn clip(text: &str) -> String {
    const MAX: usize = 120;
    if text.chars().count() <= MAX {
        text.to_owned()
    } else {
        let head: String = text.chars().take(MAX).collect();
        format!("{head}...")
    }
}

/// The side of the domain or profile a key is about, as a phrase to follow the setting.
fn place(key: &str) -> String {
    let side = |name: &str| match name {
        "XMin" => Some(side_name(Face::XMin)),
        "XMax" => Some(side_name(Face::XMax)),
        "YMin" => Some(side_name(Face::YMin)),
        "YMax" => Some(side_name(Face::YMax)),
        "ZMin" => Some("the front of the domain (ZMin)"),
        "ZMax" => Some("the back of the domain (ZMax)"),
        _ => None,
    };
    if let Some(face) = key
        .strip_prefix("/boundary_conditions/by_face/")
        .and_then(|rest| rest.split('/').next())
        .and_then(side)
    {
        return format!(" on {face}");
    }
    key.strip_prefix("/monitors/centreline_u_sample_count/")
        .map_or_else(String::new, |profile| {
            format!(" for the {profile} centreline")
        })
}

/// The user's words for a fact that has no setting in the study.
fn key_label(key: &str) -> String {
    match key {
        "/mesh/grading" => "mesh grading".into(),
        "/solver/application" => "solver application".into(),
        "/solver/turbulence" => "turbulence model".into(),
        _ => {
            if let Some(axis) = key.strip_prefix("/domain/origin_") {
                format!("domain origin in {axis}")
            } else if let Some(words) = monitor_key_label(key) {
                words
            } else if key.starts_with("/boundary_conditions/by_face/Z") {
                if key.contains("/ZMin/") {
                    "front face boundary kind".into()
                } else {
                    "back face boundary kind".into()
                }
            } else if key.starts_with("/boundary_conditions/by_face/") {
                if key.ends_with("/velocity") {
                    "boundary velocity".into()
                } else {
                    "boundary kind".into()
                }
            } else {
                setting_label(key)
            }
        }
    }
}

/// `/monitors/probe/2/x` reads as `probe 2 x`; a surface's `kind` is its boundary kind.
fn monitor_key_label(key: &str) -> Option<String> {
    let rest = key.strip_prefix("/monitors/")?;
    let mut parts = rest.split('/');
    let (kind @ ("probe" | "line" | "surface"), Some(n), Some(member), None) =
        (parts.next()?, parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let member = match member {
        "kind" => "boundary kind",
        other => other,
    };
    Some(format!("{kind} {n} {member}"))
}
