//! Whole-study diagnostics: every problem in a study, reported together, in the author's own
//! words, with a stable code and a JSON pointer into the authored study.
//!
//! The model owns the answer to "is this study valid". It does no I/O.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Quantity, StudyError,
    quantity::Dimension,
    raw::{MAX_RAW_OVERRIDES, RawBackend},
    study::{BoundaryKind, Face, Study},
    units::si_value,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Severity {
    /// Blocks: nothing is compiled or run while an error stands.
    Error,
    /// Reported and kept in the run record; never blocks.
    Warning,
}

/// Where a finding about a written file lives, as opposed to a setting in the study.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Location {
    pub file: String,
    pub line: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

/// Every diagnostic code, with a one-line meaning. The only list tests and agents match on.
pub const CODES: &[(&str, &str)] = &[
    ("CFD-STUDY-001", "the study text is not a readable study"),
    (
        "CFD-STUDY-002",
        "the study's schema version is not supported",
    ),
    ("CFD-STUDY-003", "the study file could not be read"),
    (
        "CFD-UNIT-001",
        "a quantity is given in a unit of the wrong kind",
    ),
    ("CFD-UNIT-002", "a quantity is not a finite number"),
    (
        "CFD-VAL-001",
        "a length or the viscosity is not greater than zero",
    ),
    ("CFD-VAL-002", "a boundary velocity is zero"),
    ("CFD-VAL-003", "a cell count is zero"),
    (
        "CFD-VAL-004",
        "the end time or write interval is not a usable whole iteration count",
    ),
    ("CFD-VAL-005", "fewer than two centreline samples"),
    (
        "CFD-VAL-006",
        "a convergence limit is not finite and greater than zero",
    ),
    (
        "CFD-BC-001",
        "a side of the domain has no boundary condition",
    ),
    (
        "CFD-BC-002",
        "a side of the domain has more than one boundary condition",
    ),
    ("CFD-BC-003", "a boundary condition lists no faces"),
    (
        "CFD-BC-004",
        "a velocity inlet or moving wall has no velocity",
    ),
    (
        "CFD-BC-005",
        "a boundary kind that takes no velocity has one",
    ),
    ("CFD-BC-006", "a boundary label breaks the naming rule"),
    (
        "CFD-DOM-001",
        "the domain is open but has no pressure outlet, so the flow cannot leave",
    ),
    (
        "CFD-DOM-002",
        "the domain is closed but has an inlet or outlet",
    ),
    ("CFD-MODEL-001", "the backend cannot run this boundary kind"),
    (
        "CFD-MODEL-002",
        "boundaries of one kind impose different velocities and the backend allows one",
    ),
    (
        "CFD-MODEL-003",
        "compile refused a study that the checks accepted",
    ),
    (
        "CFD-MESH-001",
        "the backend needs exactly one cell through z",
    ),
    ("CFD-WARN-001", "laminar solver at a high Reynolds number"),
    ("CFD-WARN-002", "very coarse mesh"),
    ("CFD-GUARD-001", "a preprocessor directive in a case file"),
    ("CFD-GUARD-002", "a coded type or keyword in a case file"),
    ("CFD-GUARD-003", "a system call in a case file"),
    (
        "CFD-GUARD-004",
        "a library that is not allowed in a case file",
    ),
    ("CFD-GUARD-005", "a switch block in a case file"),
    ("CFD-GUARD-006", "an include outside the case"),
    ("CFD-GUARD-007", "a file or path the case tree rules forbid"),
    ("CFD-GUARD-008", "an unterminated construct in a case file"),
    (
        "CFD-GUARD-009",
        "variable expansion other than a plain name in a case file",
    ),
    (
        "CFD-RT-001",
        "a value read back from the case differs from the study",
    ),
    ("CFD-RT-002", "a case file could not be read back"),
    ("CFD-RT-003", "the exported case differs from the study"),
    (
        "CFD-RUN-001",
        "the smoke run is limited to a few iterations",
    ),
    (
        "CFD-RUN-002",
        "the reference gate needs a lid-driven cavity",
    ),
    ("CFD-INFRA-001", "the compute host is busy"),
    ("CFD-INFRA-002", "the connection to the compute host failed"),
    ("CFD-INFRA-003", "the container engine failed"),
    ("CFD-INFRA-004", "the run timed out"),
    ("CFD-INFRA-005", "the run could not be set up"),
    ("CFD-INFRA-006", "the run was cancelled"),
    ("CFD-WS-001", "the workspace name is not valid"),
    ("CFD-WS-002", "the study was not found in the workspace"),
    (
        "CFD-WS-003",
        "a study of that name already exists in the workspace",
    ),
    (
        "CFD-CLI-001",
        "the output directory is not empty or cannot be used",
    ),
    ("CFD-RAW-001", "a raw override label breaks the naming rule"),
    (
        "CFD-RAW-002",
        "a raw override's file is not a case file the study may name",
    ),
    ("CFD-RAW-003", "a raw override's entry path is malformed"),
    (
        "CFD-RAW-004",
        "a raw override's value is empty or holds a character that could end or extend the entry",
    ),
    (
        "CFD-RAW-005",
        "a raw override is tagged for another backend",
    ),
    ("CFD-RAW-006", "the study has too many raw overrides"),
    (
        "CFD-RAW-007",
        "the written case has no such file or block for a raw override, raised by the writer",
    ),
    ("CFD-MON-001", "a monitor label breaks the naming rule"),
    ("CFD-MON-002", "a probe or line point is outside the domain"),
    (
        "CFD-MON-003",
        "a line has fewer than 2 or more than 2000 samples, or no length",
    ),
    (
        "CFD-MON-004",
        "a surface monitor names a boundary the study does not have",
    ),
    ("CFD-MON-005", "the study has too many monitors of a kind"),
    ("CFD-MON-006", "a settle setting is outside its range"),
    (
        "CFD-RUN-003",
        "the solver hit a floating-point trap or a non-finite number",
    ),
    ("CFD-RUN-004", "a residual blew up"),
    ("CFD-RUN-005", "the Courant number is over its limit"),
    ("CFD-RUN-006", "the continuity error is growing"),
    (
        "CFD-RUN-007",
        "the run was stopped on request and its latest time kept",
    ),
    (
        "CFD-RUN-008",
        "the run could not be confirmed stopped and was marked interrupted",
    ),
    ("CFD-INFRA-007", "a detached start failed"),
    ("CFD-INFRA-008", "the host state does not match the queue"),
    ("CFD-INFRA-009", "the compute host stayed unreachable"),
];
// END OF CATALOGUE: code literals below this line are the ones the model emits.

pub fn is_known_code(code: &str) -> bool {
    CODES.iter().any(|(known, _)| *known == code)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    /// Stable identifier, `CFD-<FAMILY>-<three digits>`. Tests and agents match on this.
    pub code: String,
    pub severity: Severity,
    /// The setting in the user's words, for example "lid velocity".
    pub setting: String,
    /// RFC 6901 JSON pointer into the authored study. Empty for findings about a written file.
    pub pointer: String,
    /// Plain language for an engineer who is not a CFD specialist.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        code: &str,
        setting: impl Into<String>,
        pointer: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_owned(),
            severity,
            setting: setting.into(),
            pointer: pointer.into(),
            message: message.into(),
            hint: None,
            location: None,
        }
    }

    pub fn error(
        code: &str,
        setting: impl Into<String>,
        pointer: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Error, code, setting, pointer, message)
    }

    pub fn warning(
        code: &str,
        setting: impl Into<String>,
        pointer: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(Severity::Warning, code, setting, pointer, message)
    }

    /// A study that could not even be read, as a diagnostic.
    pub fn from_study_error(error: &StudyError) -> Self {
        match error {
            StudyError::UnsupportedSchemaVersion { found } => Self::error(
                "CFD-STUDY-002",
                "schema version",
                "/schema_version",
                format!(
                    "This study uses schema version {found}, which this release cannot read. \
                     It reads versions 0 and 1."
                ),
            )
            .with_hint("Save the study again from a release that writes version 1."),
            StudyError::Json(error) => Self::error(
                "CFD-STUDY-001",
                "study file",
                "",
                format!("The study could not be read: {error}"),
            ),
        }
    }

    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// What a backend can run. The model checks a study against it before anything is compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub boundary_kinds: Vec<BoundaryKind>,
    /// The backend writes a two-dimensional case: exactly one cell through z.
    pub requires_single_cell_z: bool,
    /// The backend writes one patch per boundary kind, so boundaries of one kind must impose
    /// the same velocity.
    pub same_kind_boundaries_must_agree: bool,
    /// The backend whose raw overrides this backend runs. `None` accepts any tag.
    pub raw_backend: Option<RawBackend>,
}

impl Capabilities {
    pub fn unrestricted() -> Self {
        Self {
            boundary_kinds: ALL_KINDS.to_vec(),
            requires_single_cell_z: false,
            same_kind_boundaries_must_agree: false,
            raw_backend: None,
        }
    }
}

pub const ALL_KINDS: [BoundaryKind; 6] = [
    BoundaryKind::VelocityInlet,
    BoundaryKind::PressureOutlet,
    BoundaryKind::NoSlipWall,
    BoundaryKind::SlipWall,
    BoundaryKind::MovingWall,
    BoundaryKind::Symmetry,
];

const FACES: [Face; 4] = [Face::XMin, Face::XMax, Face::YMin, Face::YMax];

pub(crate) fn side_name(face: Face) -> &'static str {
    match face {
        Face::XMin => "the left side of the domain (XMin)",
        Face::XMax => "the right side of the domain (XMax)",
        Face::YMin => "the bottom of the domain (YMin)",
        Face::YMax => "the top of the domain (YMax)",
    }
}

/// RFC 6901 escaping of one reference token.
pub fn pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Every finding for `study`, sorted by pointer then code.
pub fn diagnose(study: &Study, capabilities: &Capabilities) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    if study.schema_version != 1 {
        found.push(Diagnostic::error(
            "CFD-STUDY-002",
            "schema version",
            "/schema_version",
            format!(
                "This study uses schema version {}, which this release cannot read.",
                study.schema_version
            ),
        ));
    }
    values(study, &mut found);
    monitors(study, &mut found);
    boundaries(study, &mut found);
    face_coverage(study, &mut found);
    domain_consistency(study, &mut found);
    capability_limits(study, capabilities, &mut found);
    raw_overrides(study, capabilities, &mut found);
    warnings(study, &mut found);
    found.sort_by(|a, b| (&a.pointer, &a.code, &a.message).cmp(&(&b.pointer, &b.code, &b.message)));
    found
}

fn noun(dimension: Dimension) -> (&'static str, &'static str) {
    match dimension {
        Dimension::Length => ("a length", "m or mm"),
        Dimension::Velocity => ("a speed", "m/s"),
        Dimension::KinematicViscosity => ("a kinematic viscosity", "m2/s"),
        Dimension::Pressure => ("a pressure", "Pa"),
        Dimension::Temperature => ("a temperature", "K"),
        Dimension::Time => ("a time", "s"),
        Dimension::Dimensionless => ("a plain number", "1"),
        Dimension::AngularVelocity => ("a rotation rate", "rpm"),
    }
}

/// Check one authored quantity without converting it for real. `Some(value)` is its SI value.
fn quantity(
    found: &mut Vec<Diagnostic>,
    q: &Quantity,
    dimension: Dimension,
    setting: &str,
    pointer: &str,
) -> Option<f64> {
    if q.unit.dimension() != dimension {
        let symbol = serde_json::to_value(q.unit)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let (wanted, units) = noun(dimension);
        found.push(
            Diagnostic::error(
                "CFD-UNIT-001",
                setting,
                pointer,
                format!(
                    "The {setting} is given in {symbol}, which is {}, but {wanted} is needed.",
                    noun(q.unit.dimension()).0
                ),
            )
            .with_hint(format!("Give it in {units}.")),
        );
        return None;
    }
    let value = si_value(q);
    if !value.is_finite() {
        found.push(
            Diagnostic::error(
                "CFD-UNIT-002",
                setting,
                pointer,
                format!("The {setting} is not a finite number."),
            )
            .with_hint("Enter an ordinary number."),
        );
        return None;
    }
    Some(value)
}

fn positive(found: &mut Vec<Diagnostic>, value: Option<f64>, setting: &str, pointer: &str) {
    if value.is_some_and(|v| v <= 0.0) {
        found.push(
            Diagnostic::error(
                "CFD-VAL-001",
                setting,
                pointer,
                format!("The {setting} must be greater than zero."),
            )
            .with_hint("Enter a positive value."),
        );
    }
}

fn whole_count(
    found: &mut Vec<Diagnostic>,
    value: Option<f64>,
    setting: &str,
    pointer: &str,
) -> Option<f64> {
    let value = value?;
    if value <= 0.0 || value.fract() != 0.0 {
        found.push(
            Diagnostic::error(
                "CFD-VAL-004",
                setting,
                pointer,
                format!(
                    "The {setting} counts solver iterations, so it must be a whole number greater \
                     than zero."
                ),
            )
            .with_hint("Use a value such as 500 s: each second stands for one iteration."),
        );
        return None;
    }
    Some(value)
}

fn values(study: &Study, found: &mut Vec<Diagnostic>) {
    for (axis, q) in [
        ("x", &study.domain.length_x),
        ("y", &study.domain.length_y),
        ("z", &study.domain.length_z),
    ] {
        let setting = format!("domain length in {axis}");
        let value = quantity(
            found,
            q,
            Dimension::Length,
            &setting,
            &format!("/domain/length_{axis}"),
        );
        positive(found, value, &setting, &format!("/domain/length_{axis}"));
    }
    let nu = quantity(
        found,
        &study.fluid.kinematic_viscosity,
        Dimension::KinematicViscosity,
        "kinematic viscosity",
        "/fluid/kinematic_viscosity",
    );
    positive(
        found,
        nu,
        "kinematic viscosity",
        "/fluid/kinematic_viscosity",
    );
    for (axis, count) in [
        ("x", study.mesh.cells_x),
        ("y", study.mesh.cells_y),
        ("z", study.mesh.cells_z),
    ] {
        if count == 0 {
            found.push(
                Diagnostic::error(
                    "CFD-VAL-003",
                    format!("cell count in {axis}"),
                    format!("/mesh/cells_{axis}"),
                    format!("The mesh needs at least one cell in {axis}."),
                )
                .with_hint("Enter a whole number of cells, one or more."),
            );
        }
    }
    let end = quantity(
        found,
        &study.solver.end_time,
        Dimension::Time,
        "end time",
        "/solver/end_time",
    );
    let interval = quantity(
        found,
        &study.solver.write_interval,
        Dimension::Time,
        "write interval",
        "/solver/write_interval",
    );
    let end = whole_count(found, end, "end time", "/solver/end_time");
    let interval = whole_count(found, interval, "write interval", "/solver/write_interval");
    if let (Some(end), Some(interval)) = (end, interval)
        && (interval > end || end % interval != 0.0)
    {
        found.push(
            Diagnostic::error(
                "CFD-VAL-004",
                "write interval",
                "/solver/write_interval",
                "The write interval must divide the end time exactly, so the last results are \
                 written.",
            )
            .with_hint("Pick an interval such as half or a tenth of the end time."),
        );
    }
    for (name, pointer, limit) in [
        (
            "pressure",
            "/solver/convergence/p",
            study.solver.convergence.p,
        ),
        (
            "velocity",
            "/solver/convergence/U",
            study.solver.convergence.u,
        ),
    ] {
        if !limit.is_finite() || limit <= 0.0 {
            found.push(
                Diagnostic::error(
                    "CFD-VAL-006",
                    format!("convergence limit for {name}"),
                    pointer,
                    format!("The convergence limit for {name} must be a number greater than zero."),
                )
                .with_hint("A value such as 1e-6 is typical."),
            );
        }
    }
    if study.monitors.centreline_u_sample_count < 2 {
        found.push(
            Diagnostic::error(
                "CFD-VAL-005",
                "centreline sample count",
                "/monitors/centreline_u_sample_count",
                "A centreline profile needs at least two sample points.",
            )
            .with_hint("Use 32 or more for a useful profile."),
        );
    }
}

const MAX_PROBES: usize = 16;
const MAX_LINES: usize = 8;
const MAX_SURFACES: usize = 8;
const MIN_LINE_SAMPLES: u64 = 2;
const MAX_LINE_SAMPLES: u64 = 2000;
/// How far outside the domain a point may sit and still count as on its edge, in metres.
const EDGE_TOLERANCE: f64 = 1e-12;

/// The SI length of a domain side when it is a finite length; no findings (`values` reports).
fn side_length(q: &Quantity) -> Option<f64> {
    let v = si_value(q);
    (q.unit.dimension() == Dimension::Length && v.is_finite()).then_some(v)
}

/// One probe or line coordinate: a length, and inside `0..=limit` (the edge counts as inside).
fn point_in_domain(
    found: &mut Vec<Diagnostic>,
    q: &Quantity,
    what: &str,
    axis: &str,
    limit: Option<f64>,
    pointer: String,
) -> Option<f64> {
    let setting = format!("{what} {axis}");
    let value = quantity(found, q, Dimension::Length, &setting, &pointer)?;
    if let Some(limit) = limit
        && !(-EDGE_TOLERANCE..=limit + EDGE_TOLERANCE).contains(&value)
    {
        found.push(
            Diagnostic::error(
                "CFD-MON-002",
                setting.clone(),
                pointer,
                format!("The {setting} lies outside the domain, which spans 0 to {limit} m."),
            )
            .with_hint("Move the point inside the domain, or onto its edge."),
        );
    }
    Some(value)
}

/// The monitors: plain labels, points inside the domain, sensible lines, known boundaries, a
/// bounded count and settle tolerances in range. A rejected label is never echoed.
fn monitors(study: &Study, found: &mut Vec<Diagnostic>) {
    let m = &study.monitors;
    let width = side_length(&study.domain.length_x);
    let height = side_length(&study.domain.length_y);
    let labels = m
        .probes
        .keys()
        .map(|n| ("probes", n))
        .chain(m.lines.keys().map(|n| ("lines", n)))
        .chain(m.surfaces.keys().map(|n| ("surfaces", n)));
    for (kind, name) in labels {
        if !valid_boundary_name(name) {
            found.push(
                Diagnostic::error(
                    "CFD-MON-001",
                    "monitor label",
                    format!("/monitors/{kind}/{}", pointer_token(name)),
                    "A monitor label must start with a letter and use only letters, digits, \
                     hyphens and underscores, up to 32 characters.",
                )
                .with_hint("Rename the monitor, for example to \"centre\" or \"lid-flux\"."),
            );
        }
    }
    for (name, probe) in &m.probes {
        let base = format!("/monitors/probes/{}", pointer_token(name));
        let what = format!("probe {}", label(name));
        point_in_domain(found, &probe.x, &what, "x", width, format!("{base}/x"));
        point_in_domain(found, &probe.y, &what, "y", height, format!("{base}/y"));
    }
    for (name, line) in &m.lines {
        let base = format!("/monitors/lines/{}", pointer_token(name));
        let what = format!("line {}", label(name));
        let x0 = point_in_domain(found, &line.x0, &what, "x0", width, format!("{base}/x0"));
        let y0 = point_in_domain(found, &line.y0, &what, "y0", height, format!("{base}/y0"));
        let x1 = point_in_domain(found, &line.x1, &what, "x1", width, format!("{base}/x1"));
        let y1 = point_in_domain(found, &line.y1, &what, "y1", height, format!("{base}/y1"));
        if !(MIN_LINE_SAMPLES..=MAX_LINE_SAMPLES).contains(&line.samples) {
            found.push(
                Diagnostic::error(
                    "CFD-MON-003",
                    format!("{what} samples"),
                    format!("{base}/samples"),
                    format!(
                        "The {what} needs between {MIN_LINE_SAMPLES} and {MAX_LINE_SAMPLES} \
                         sample points."
                    ),
                )
                .with_hint("Use 16 to 100 for a readable profile."),
            );
        }
        if x0.is_some() && x0 == x1 && y0.is_some() && y0 == y1 {
            found.push(
                Diagnostic::error(
                    "CFD-MON-003",
                    what.clone(),
                    base.clone(),
                    format!("The {what} starts and ends at the same point, so it has no length."),
                )
                .with_hint("Give the line two different ends."),
            );
        }
    }
    for (name, surface) in &m.surfaces {
        if !study.boundary_conditions.contains_key(&surface.boundary) {
            let what = format!("surface {} boundary", label(name));
            found.push(
                Diagnostic::error(
                    "CFD-MON-004",
                    what.clone(),
                    format!("/monitors/surfaces/{}/boundary", pointer_token(name)),
                    format!("The {what} is not one of the study's boundaries."),
                )
                .with_hint("Name one of the boundary labels of this study."),
            );
        }
    }
    for (kind, count, limit) in [
        ("probes", m.probes.len(), MAX_PROBES),
        ("lines", m.lines.len(), MAX_LINES),
        ("surfaces", m.surfaces.len(), MAX_SURFACES),
    ] {
        if count > limit {
            found.push(
                Diagnostic::error(
                    "CFD-MON-005",
                    format!("monitored {kind}"),
                    format!("/monitors/{kind}"),
                    format!("The study lists {count} {kind}, but at most {limit} are allowed."),
                )
                .with_hint("Remove the ones you no longer need."),
            );
        }
    }
    if let Some(settle) = &m.settle {
        let fraction = |v: Option<f64>| v.map(|v| v > 0.0 && v <= 1.0);
        for (name, in_range, range) in [
            (
                "tail_fraction",
                fraction(settle.tail_fraction),
                "above 0 and at most 1",
            ),
            (
                "tail_floor",
                settle.tail_floor.map(|v| (5..=100_000).contains(&v)),
                "from 5 to 100000",
            ),
            (
                "spread_rel",
                fraction(settle.spread_rel),
                "above 0 and at most 1",
            ),
            (
                "drift_rel",
                fraction(settle.drift_rel),
                "above 0 and at most 1",
            ),
        ] {
            if in_range == Some(false) {
                let pointer = format!("/monitors/settle/{name}");
                let setting = setting_label(&pointer);
                found.push(
                    Diagnostic::error(
                        "CFD-MON-006",
                        setting.clone(),
                        pointer,
                        format!("The {setting} must be {range}."),
                    )
                    .with_hint("Leave it out to use the built-in value."),
                );
            }
        }
    }
}

fn boundaries(study: &Study, found: &mut Vec<Diagnostic>) {
    for (name, boundary) in &study.boundary_conditions {
        let base = format!("/boundary_conditions/{}", pointer_token(name));
        let shown = label(name);
        if !valid_boundary_name(name) {
            found.push(
                Diagnostic::error(
                    "CFD-BC-006",
                    "boundary label",
                    base.clone(),
                    "A boundary label must start with a letter and use only letters, digits, \
                     hyphens and underscores, up to 32 characters.",
                )
                .with_hint("Rename the boundary, for example to \"inlet\" or \"left-wall\"."),
            );
        }
        if boundary.faces.is_empty() {
            found.push(
                Diagnostic::error(
                    "CFD-BC-003",
                    format!("{shown} faces"),
                    format!("{base}/faces"),
                    format!("The boundary \"{shown}\" lists no faces, so it applies nowhere."),
                )
                .with_hint("List at least one of XMin, XMax, YMin, YMax."),
            );
        }
        let setting = format!("{shown} velocity");
        let pointer = format!("{base}/velocity");
        match (boundary.kind, &boundary.velocity) {
            (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall, Some(q)) => {
                if quantity(found, q, Dimension::Velocity, &setting, &pointer) == Some(0.0) {
                    found.push(
                        Diagnostic::error(
                            "CFD-VAL-002",
                            setting,
                            pointer,
                            format!("The {shown} velocity is zero, so nothing would move."),
                        )
                        .with_hint("Enter a speed other than zero (a negative value reverses it)."),
                    );
                }
            }
            (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall, None) => found.push(
                Diagnostic::error(
                    "CFD-BC-004",
                    setting,
                    pointer,
                    format!(
                        "The boundary \"{shown}\" is a {} and needs a velocity.",
                        kind_name(boundary.kind)
                    ),
                )
                .with_hint("Add a velocity with a value and a unit such as m/s."),
            ),
            (_, Some(_)) => found.push(
                Diagnostic::error(
                    "CFD-BC-005",
                    setting,
                    pointer,
                    format!(
                        "The boundary \"{shown}\" is a {}, which takes no velocity.",
                        kind_name(boundary.kind)
                    ),
                )
                .with_hint("Remove the velocity, or change the boundary to a velocity inlet or moving wall."),
            ),
            (_, None) => {}
        }
    }
}

fn domain_consistency(study: &Study, found: &mut Vec<Diagnostic>) {
    let outlets = study
        .boundary_conditions
        .values()
        .any(|b| b.kind == BoundaryKind::PressureOutlet);
    if study.domain.open && !outlets {
        found.push(
            Diagnostic::error(
                "CFD-DOM-001",
                "open-domain flag",
                "/domain/open",
                "The domain is marked open but has no pressure outlet, so the flow has nowhere \
                 to leave.",
            )
            .with_hint("Make one boundary a pressure outlet, or set the domain to closed."),
        );
    }
    if !study.domain.open {
        for (name, boundary) in &study.boundary_conditions {
            if matches!(
                boundary.kind,
                BoundaryKind::VelocityInlet | BoundaryKind::PressureOutlet
            ) {
                let shown = label(name);
                found.push(
                    Diagnostic::error(
                        "CFD-DOM-002",
                        format!("{shown} boundary kind"),
                        format!("/boundary_conditions/{}/kind", pointer_token(name)),
                        format!(
                            "The domain is closed, but the boundary \"{shown}\" is a {}. A \
                             closed domain has only walls.",
                            kind_name(boundary.kind)
                        ),
                    )
                    .with_hint("Set the domain to open, or make this boundary a wall."),
                );
            }
        }
    }
}

/// The SI speed of a boundary velocity when it is a finite, nonzero speed; no findings.
fn speed(boundary: &crate::study::BoundaryCondition) -> Option<f64> {
    let q = boundary.velocity.as_ref()?;
    let v = si_value(q);
    (q.unit.dimension() == Dimension::Velocity && v.is_finite() && v != 0.0).then_some(v)
}

fn capability_limits(study: &Study, capabilities: &Capabilities, found: &mut Vec<Diagnostic>) {
    for (name, boundary) in &study.boundary_conditions {
        if !capabilities.boundary_kinds.contains(&boundary.kind) {
            let shown = label(name);
            found.push(
                Diagnostic::error(
                    "CFD-MODEL-001",
                    format!("{shown} boundary kind"),
                    format!("/boundary_conditions/{}/kind", pointer_token(name)),
                    format!(
                        "The selected solver setup cannot run a {} (boundary \"{shown}\").",
                        kind_name(boundary.kind)
                    ),
                )
                .with_hint("Choose another kind for this boundary."),
            );
        }
    }
    if capabilities.requires_single_cell_z && study.mesh.cells_z != 1 {
        found.push(
            Diagnostic::error(
                "CFD-MESH-001",
                "cell count in z",
                "/mesh/cells_z",
                format!(
                    "The selected solver setup runs a flat, two-dimensional case, so the mesh \
                     needs exactly one cell through z, not {}.",
                    study.mesh.cells_z
                ),
            )
            .with_hint("Set the cell count in z to 1."),
        );
    }
    if capabilities.same_kind_boundaries_must_agree {
        for kind in [BoundaryKind::VelocityInlet, BoundaryKind::MovingWall] {
            let mut first: Option<(&str, [f64; 3])> = None;
            'kind: for (name, boundary) in &study.boundary_conditions {
                if boundary.kind != kind {
                    continue;
                }
                let Some(v) = speed(boundary) else { continue };
                let faces: BTreeSet<Face> = boundary.faces.iter().copied().collect();
                for face in faces {
                    let vector = crate::resolve::velocity_vector(kind, face, v);
                    match first {
                        None => first = Some((name, vector)),
                        Some((other, expected)) if expected != vector => {
                            let names = if other == name {
                                format!("\"{}\"", label(name))
                            } else {
                                format!("\"{}\" and \"{}\"", label(other), label(name))
                            };
                            found.push(
                                Diagnostic::error(
                                    "CFD-MODEL-002",
                                    format!("{} velocity", label(name)),
                                    format!(
                                        "/boundary_conditions/{}/velocity",
                                        pointer_token(name)
                                    ),
                                    format!(
                                        "The {0}s {names} move the flow differently, but the \
                                         selected solver setup applies one velocity to all {0}s.",
                                        kind_name(kind)
                                    ),
                                )
                                .with_hint(
                                    "Give them the same speed on matching sides, or use another \
                                     kind for one of them.",
                                ),
                            );
                            break 'kind;
                        }
                        Some(_) => {}
                    }
                }
            }
        }
    }
}

/// The raw overrides: each is checked for a plain label, a case file the study may name, a
/// well-formed entry path, a value that cannot end or extend the entry, and a backend tag the
/// backend runs. Rejected text is never echoed: the label names the override.
fn raw_overrides(study: &Study, capabilities: &Capabilities, found: &mut Vec<Diagnostic>) {
    if study.raw_overrides.len() > MAX_RAW_OVERRIDES {
        found.push(
            Diagnostic::error(
                "CFD-RAW-006",
                "raw overrides",
                "/raw_overrides",
                format!(
                    "The study has {} raw overrides, but at most {MAX_RAW_OVERRIDES} are allowed.",
                    study.raw_overrides.len()
                ),
            )
            .with_hint("Remove the ones you no longer need."),
        );
    }
    for (name, item) in &study.raw_overrides {
        let base = format!("/raw_overrides/{}", pointer_token(name));
        let shown = label(name);
        let setting = format!("raw override {shown}");
        if !valid_boundary_name(name) {
            found.push(
                Diagnostic::error(
                    "CFD-RAW-001",
                    "raw override label",
                    base.clone(),
                    "A raw override label must start with a letter and use only letters, digits, \
                     hyphens and underscores, up to 32 characters.",
                )
                .with_hint("Rename the override, for example to \"relax-u\"."),
            );
        }
        if !valid_raw_file(&item.file) {
            found.push(
                Diagnostic::error(
                    "CFD-RAW-002",
                    format!("{setting} file"),
                    format!("{base}/file"),
                    format!(
                        "The file of the raw override \"{shown}\" must be a case file under \
                         system, constant or 0, such as system/fvSolution."
                    ),
                )
                .with_hint("Use a relative name with one or two plain folder or file names."),
            );
        }
        if !valid_raw_entry(&item.entry) {
            found.push(
                Diagnostic::error(
                    "CFD-RAW-003",
                    format!("{setting} entry"),
                    format!("{base}/entry"),
                    format!(
                        "The entry of the raw override \"{shown}\" must be one to six names \
                         joined by /, each starting with a letter, for example \
                         relaxationFactors/equations/U."
                    ),
                )
                .with_hint("Use letters, digits and underscores only in each name."),
            );
        }
        if !valid_raw_value(&item.value) {
            found.push(
                Diagnostic::error(
                    "CFD-RAW-004",
                    format!("{setting} value"),
                    format!("{base}/value"),
                    format!(
                        "The value of the raw override \"{shown}\" must be 1 to 200 printable \
                         characters without ; {{ }} # $ \" or a backslash."
                    ),
                )
                .with_hint("Give the plain text that goes after the entry name, such as 0.5."),
            );
        }
        if let Some(own) = capabilities.raw_backend
            && item.backend != own
        {
            found.push(
                Diagnostic::error(
                    "CFD-RAW-005",
                    format!("{setting} backend"),
                    format!("{base}/backend"),
                    format!(
                        "The raw override \"{shown}\" is written for the {} backend, but the \
                         selected solver setup runs {own} overrides.",
                        item.backend
                    ),
                )
                .with_hint("Tag the override for the backend you run, or remove it."),
            );
        }
    }
}

/// `system/`, `constant/` or `0/`, then one or two plain names.
fn valid_raw_file(file: &str) -> bool {
    let mut parts = file.split('/');
    let top = matches!(parts.next(), Some("system" | "constant" | "0"));
    let rest: Vec<&str> = parts.collect();
    let plain = |part: &&str| {
        !part.is_empty()
            && *part != "."
            && !part.contains("..")
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    top && file.len() <= 100 && (1..=2).contains(&rest.len()) && rest.iter().all(plain)
}

/// One to six names joined by `/`, each `[A-Za-z][A-Za-z0-9_]{0,63}`.
fn valid_raw_entry(entry: &str) -> bool {
    let name = |part: &str| {
        let mut chars = part.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && part.len() <= 64
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    (1..=6).contains(&entry.split('/').count()) && entry.split('/').all(name)
}

/// 1 to 200 printable ASCII characters, none of which can end the entry, open or close a block,
/// start a directive, expand a variable, open a string or escape.
fn valid_raw_value(value: &str) -> bool {
    (1..=200).contains(&value.len())
        && value.chars().all(|c| {
            (c.is_ascii_graphic() || c == ' ')
                && !matches!(c, ';' | '{' | '}' | '#' | '$' | '"' | '\\')
        })
}

fn warnings(study: &Study, found: &mut Vec<Diagnostic>) {
    let metres = |q: &Quantity, dimension| (q.unit.dimension() == dimension).then(|| si_value(q));
    let top_speed = study
        .boundary_conditions
        .values()
        .filter_map(speed)
        .map(f64::abs)
        .fold(0.0, f64::max);
    if let (Some(length), Some(nu)) = (
        metres(&study.domain.length_y, Dimension::Length),
        metres(
            &study.fluid.kinematic_viscosity,
            Dimension::KinematicViscosity,
        ),
    ) {
        let reynolds = top_speed * length / nu;
        if length > 0.0 && nu > 0.0 && reynolds.is_finite() && reynolds > 2000.0 {
            found.push(
                Diagnostic::warning(
                    "CFD-WARN-001",
                    "kinematic viscosity",
                    "/fluid/kinematic_viscosity",
                    format!(
                        "At this speed, size and viscosity the Reynolds number is about {reynolds:.0}. \
                         Flow above roughly 2000 is usually turbulent, and the laminar solver used \
                         here will not capture that."
                    ),
                )
                .with_hint("Treat the result as a rough picture, or raise the viscosity or lower the speed."),
            );
        }
    }
    for (axis, count) in [("x", study.mesh.cells_x), ("y", study.mesh.cells_y)] {
        if (1..8).contains(&count) {
            found.push(
                Diagnostic::warning(
                    "CFD-WARN-002",
                    format!("cell count in {axis}"),
                    format!("/mesh/cells_{axis}"),
                    format!("Only {count} cells across {axis}: the result will be very rough."),
                )
                .with_hint("Use at least 8 cells, and usually many more."),
            );
        }
    }
}

/// The kind in the author's words, for example "velocity inlet".
pub fn kind_name(kind: BoundaryKind) -> &'static str {
    match kind {
        BoundaryKind::VelocityInlet => "velocity inlet",
        BoundaryKind::PressureOutlet => "pressure outlet",
        BoundaryKind::NoSlipWall => "no-slip wall",
        BoundaryKind::SlipWall => "slip wall",
        BoundaryKind::MovingWall => "moving wall",
        BoundaryKind::Symmetry => "symmetry plane",
    }
}

fn face_coverage(study: &Study, found: &mut Vec<Diagnostic>) {
    let mut claims: BTreeMap<Face, Vec<&str>> = BTreeMap::new();
    for (name, boundary) in &study.boundary_conditions {
        let faces: BTreeSet<Face> = boundary.faces.iter().copied().collect();
        for face in faces {
            claims.entry(face).or_default().push(name);
        }
    }
    for face in FACES {
        let claimants = claims.get(&face).map_or(&[][..], Vec::as_slice);
        match claimants {
            [] => found.push(
                Diagnostic::error(
                    "CFD-BC-001",
                    side_name(face),
                    "/boundary_conditions",
                    format!(
                        "{} has no boundary condition. Every side of the box needs exactly one, \
                         so the solver knows what happens to the flow there.",
                        capitalise(side_name(face))
                    ),
                )
                .with_hint(format!(
                    "Add a boundary condition whose faces list includes {face:?}."
                )),
            ),
            [_] => {}
            [first, ..] => found.push(
                Diagnostic::error(
                    "CFD-BC-002",
                    side_name(face),
                    format!("/boundary_conditions/{}/faces", pointer_token(first)),
                    format!(
                        "{} is claimed by more than one boundary condition: {}. Each side of the \
                         box can have only one.",
                        capitalise(side_name(face)),
                        claimants
                            .iter()
                            .map(|name| format!("\"{}\"", label(name)))
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ),
                )
                .with_hint(format!("Remove {face:?} from all but one of them.")),
            ),
        }
    }
}

/// The user's words for the setting a pointer into the resolved study names, the same words the
/// diagnostics use. A pointer this does not know is returned unchanged.
pub fn setting_label(pointer: &str) -> String {
    let unescape = |token: &str| token.replace("~1", "/").replace("~0", "~");
    let tokens: Vec<String> = pointer
        .trim_start_matches('/')
        .split('/')
        .map(unescape)
        .collect();
    let parts: Vec<&str> = tokens.iter().map(String::as_str).collect();
    match parts.as_slice() {
        ["domain", axis @ ("length_x" | "length_y" | "length_z")] => {
            format!("domain length in {}", &axis[7..])
        }
        ["domain", "open"] => "open-domain flag".into(),
        ["fluid", "kinematic_viscosity"] => "kinematic viscosity".into(),
        ["mesh", axis @ ("cells_x" | "cells_y" | "cells_z")] => {
            format!("cell count in {}", &axis[6..])
        }
        ["solver", "end_time"] => "end time".into(),
        ["solver", "write_interval"] => "write interval".into(),
        ["solver", "convergence", "p"] => "convergence limit for pressure".into(),
        ["solver", "convergence", "U"] => "convergence limit for velocity".into(),
        ["monitors", "centreline_u_sample_count"] => "centreline sample count".into(),
        ["monitors", "settle", member] => format!("settle {}", member.replace('_', " ")),
        [
            "monitors",
            kind @ ("probes" | "lines" | "surfaces"),
            name,
            member,
        ] => {
            let noun = kind.trim_end_matches('s');
            format!("{noun} {} {}", label(name), member.replace('_', " "))
        }
        ["boundary_conditions", name, "velocity"] => format!("{} velocity", label(name)),
        ["boundary_conditions", name, "kind"] => format!("{} boundary kind", label(name)),
        ["boundary_conditions", name, "faces"] => format!("{} faces", label(name)),
        _ => pointer.to_owned(),
    }
}

/// A boundary label fit to print: labels that break the naming rule are not echoed.
fn label(name: &str) -> &str {
    if valid_boundary_name(name) {
        name
    } else {
        "(invalid name)"
    }
}

/// `[A-Za-z][A-Za-z0-9_-]{0,31}`: a label can never carry slashes, tildes or dictionary syntax.
pub(crate) fn valid_boundary_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

pub fn has_errors(found: &[Diagnostic]) -> bool {
    found.iter().any(|d| d.severity == Severity::Error)
}

/// Control characters in study-derived text (a hostile label) never reach a terminal raw.
fn printable(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

/// `error CFD-BC-001: <message> [<setting>; <pointer>]`, plus an indented hint line.
pub fn render_text(found: &[Diagnostic]) -> String {
    let mut out = String::new();
    for d in found {
        let level = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let mut place = vec![printable(&d.setting)];
        if !d.pointer.is_empty() {
            place.push(printable(&d.pointer));
        }
        if let Some(location) = &d.location {
            let mut at = format!("{}:{}", printable(&location.file), location.line);
            if let Some(entry) = &location.entry {
                at.push_str(&format!(" ({})", printable(entry)));
            }
            place.push(at);
        }
        out.push_str(&format!(
            "{level} {}: {} [{}]\n",
            d.code,
            printable(&d.message),
            place.join("; ")
        ));
        if let Some(hint) = &d.hint {
            out.push_str(&format!("  hint: {}\n", printable(hint)));
        }
    }
    out
}
