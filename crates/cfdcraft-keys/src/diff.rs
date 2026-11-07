//! The structured difference between two recipes: which setting changed, from what to what.
//!
//! A key says a stage is out of date; the diff says why. A change is plain data, never a
//! sentence, so a caller can turn it into text in any language or read it as JSON. Every field
//! of both recipes has a change that names it, so the diff is empty exactly when the recipes
//! are equal.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cfdcraft_model::{BoundaryKind, InitMode, MonitorField, RawOverride, SurfaceQuantity};

use crate::ToolIdentity;
use crate::recipe::{
    BoundaryRecipe, BuoyancyRecipe, ForceRecipe, LineRecipe, MeshRecipe, PhysicsRecipe,
    ProbeRecipe, ResultRecipe, ShapeRecipe, SurfaceRecipe, ThermalRecipe, TimeRecipe,
};

/// One of the three domain axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}

/// One part of the tool identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolPart {
    Image,
    Dialect,
    Sidecar,
    Writer,
}

/// Which residual limit a convergence change is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Residual {
    Pressure,
    Velocity,
}

/// One part of the physics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhysicsPart {
    /// Steady or transient.
    Time,
    /// The time step of mesh level 0.
    DeltaT,
    /// The turbulence model.
    Turbulence,
    Intensity,
    ViscosityRatio,
    ReferenceVelocity,
    /// The whole buoyancy record, or `none` when it is off.
    Buoyancy,
    Init,
}

/// One difference between an old recipe and a new one. Old values come first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A part of the tool identity. An absent sidecar lock is written as `none`.
    Tool {
        part: ToolPart,
        from: String,
        to: String,
    },
    Length {
        axis: Axis,
        from: f64,
        to: f64,
    },
    DomainOpen {
        from: bool,
        to: bool,
    },
    Cells {
        axis: Axis,
        from: u64,
        to: u64,
    },
    /// The refinement level of the mesh. The cell counts it implies are named by `Cells`.
    MeshLevel {
        from: u32,
        to: u32,
    },
    /// The wall grading ratio along an axis; 1 means an even mesh.
    Grading {
        axis: Axis,
        from: f64,
        to: f64,
    },
    /// The shape of the domain, as a one-line description (see [`shape_text`]), or `None` for a
    /// plain box.
    Shape {
        from: Option<String>,
        to: Option<String>,
    },
    /// The patch layout of the mesh differs (a boundary's kind, faces or wall temperature place).
    Layout,
    /// A boundary's inlet profile, `uniform` or `parabolic`, or `None` when it states none.
    InletProfile {
        boundary: String,
        from: Option<String>,
        to: Option<String>,
    },
    /// One part of the physics, as a word or a number in the study's words (`laminar`,
    /// `k_omega_sst`, `0.001`), `none` where there is no value (see [`buoyancy_text`]).
    Physics {
        what: PhysicsPart,
        from: String,
        to: String,
    },
    /// A wall's thermal condition (see [`thermal_text`]), or `None` when it states none.
    Thermal {
        boundary: String,
        from: Option<String>,
        to: Option<String>,
    },
    Viscosity {
        from: f64,
        to: f64,
    },
    /// A boundary's velocity, including one that appears or disappears.
    BoundaryVelocity {
        boundary: String,
        from: Option<f64>,
        to: Option<f64>,
    },
    /// The variant names of the two kinds.
    BoundaryKind {
        boundary: String,
        from: String,
        to: String,
    },
    /// The face variant names, joined by a comma and a space.
    BoundaryFaces {
        boundary: String,
        from: String,
        to: String,
    },
    BoundaryAdded {
        boundary: String,
    },
    BoundaryRemoved {
        boundary: String,
    },
    EndTime {
        from: f64,
        to: f64,
    },
    WriteInterval {
        from: f64,
        to: f64,
    },
    Convergence {
        which: Residual,
        from: f64,
        to: f64,
    },
    CentrelineSamples {
        from: u64,
        to: u64,
    },
    /// The mesh the result stands on is a different one.
    MeshChanged,
    /// A raw override, named by its label, that was added, changed or removed. A side is
    /// `<backend> <file> <entry> = <value>` (see [`raw_text`]), or `None` when the override is
    /// absent there; the ops text says `none` for it.
    RawOverride {
        id: String,
        from: Option<String>,
        to: Option<String>,
    },
    /// A raw override in a mesh dictionary differs, so the mesh differs.
    RawMesh,
    /// A monitor, named by its place in its list (`probe_2`, `line_1`, `surface_3`, `force_1`),
    /// that was added, moved, changed or removed. A side is the monitor's one-line description,
    /// with positions in metres (see [`probe_text`], [`line_text`], [`surface_text`] and
    /// [`force_text`]), or `None` when the monitor is absent there; the ops text says `none` for
    /// it.
    Monitor {
        id: String,
        from: Option<String>,
        to: Option<String>,
    },
}

/// A raw override as one line of text: `<backend> <file> <entry> = <value>`.
pub fn raw_text(raw: &RawOverride) -> String {
    format!(
        "{} {} {} = {}",
        raw.backend.as_str(),
        raw.file,
        raw.entry,
        raw.value
    )
}

/// A shape as one line, with sizes in metres: `backward step, step 0.0127 m, upstream 1.651 m,
/// start-up 0.254 m` or `cylinder, diameter 0.1 m at (0.2, 0.2) m`.
pub fn shape_text(shape: &ShapeRecipe) -> String {
    match shape {
        ShapeRecipe::BackwardStep {
            step_height,
            upstream_length,
            startup_length,
        } => format!(
            "backward step, step {step_height} m, upstream {upstream_length} m, start-up \
             {startup_length} m"
        ),
        ShapeRecipe::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        } => format!("cylinder, diameter {diameter} m at ({centre_x}, {centre_y}) m"),
    }
}

fn field_word(field: MonitorField) -> &'static str {
    match field {
        MonitorField::Velocity => "velocity",
        MonitorField::Pressure => "pressure",
    }
}

/// A probe as one line: `x 0.05 m, y 0.05 m, velocity`, with `, on <boundary>` for a reading at
/// a wall.
pub fn probe_text(p: &ProbeRecipe) -> String {
    let base = format!("x {} m, y {} m, {}", p.x, p.y, field_word(p.field));
    match &p.on {
        Some(boundary) => format!("{base}, on {boundary}"),
        None => base,
    }
}

/// A force monitor as one line: `forces on cylinder, speed 1 m/s, length 0.1 m`.
pub fn force_text(f: &ForceRecipe) -> String {
    format!(
        "forces on {}, speed {} m/s, length {} m",
        f.boundary, f.reference_velocity, f.reference_length
    )
}

/// A thermal condition as one line: `fixed 307.8 K` or `adiabatic`.
pub fn thermal_text(t: &ThermalRecipe) -> String {
    match t {
        ThermalRecipe::FixedTemperature { value } => format!("fixed {value} K"),
        ThermalRecipe::Adiabatic => "adiabatic".to_owned(),
    }
}

/// The buoyancy record as one line, with SI units.
pub fn buoyancy_text(b: &BuoyancyRecipe) -> String {
    format!(
        "gravity {} m/s2, beta {} 1/K, reference {} K, Prandtl {}, turbulent Prandtl {}",
        b.gravity, b.beta, b.reference_temperature, b.prandtl, b.turbulent_prandtl
    )
}

/// A line as one line: `from (0, 0.05) m to (0.1, 0.05) m, 64 samples, velocity`.
pub fn line_text(l: &LineRecipe) -> String {
    format!(
        "from ({}, {}) m to ({}, {}) m, {} samples, {}",
        l.x0,
        l.y0,
        l.x1,
        l.y1,
        l.samples,
        field_word(l.field)
    )
}

/// A surface monitor as one line: `flux on movingWall`. The patch word is the name the written
/// case gives a boundary of that kind.
pub fn surface_text(s: &SurfaceRecipe) -> String {
    let quantity = match s.quantity {
        SurfaceQuantity::Flux => "flux",
        SurfaceQuantity::PressureAverage => "pressure average",
        SurfaceQuantity::PressureIntegral => "pressure integral",
        SurfaceQuantity::HeatFlux => "heat flux",
    };
    if let Some(boundary) = &s.boundary {
        return format!("{quantity} on {boundary}");
    }
    let patch = match s.kind {
        BoundaryKind::VelocityInlet => "inlet",
        BoundaryKind::PressureOutlet => "outlet",
        BoundaryKind::MovingWall => "movingWall",
        BoundaryKind::NoSlipWall => "fixedWalls",
        BoundaryKind::SlipWall => "slipWalls",
        BoundaryKind::Symmetry => "symmetry",
    };
    format!("{quantity} on {patch}")
}

/// One change per place in the list where the two lists differ or one is missing.
fn monitor_list<T: PartialEq>(
    prefix: &str,
    old: &[T],
    new: &[T],
    text: fn(&T) -> String,
    out: &mut Vec<Change>,
) {
    for i in 0..old.len().max(new.len()) {
        let (from, to) = (old.get(i), new.get(i));
        if from != to {
            out.push(Change::Monitor {
                id: format!("{prefix}_{}", i + 1),
                from: from.map(text),
                to: to.map(text),
            });
        }
    }
}

fn tool_changes(old: &ToolIdentity, new: &ToolIdentity, out: &mut Vec<Change>) {
    let lock = |l: &Option<String>| l.as_deref().unwrap_or("none").to_owned();
    let parts = [
        (
            ToolPart::Image,
            old.image_digest.clone(),
            new.image_digest.clone(),
        ),
        (ToolPart::Dialect, old.dialect.clone(), new.dialect.clone()),
        (
            ToolPart::Sidecar,
            lock(&old.sidecar_lock),
            lock(&new.sidecar_lock),
        ),
        (
            ToolPart::Writer,
            old.writer_version.clone(),
            new.writer_version.clone(),
        ),
    ];
    for (part, from, to) in parts {
        if from != to || (part == ToolPart::Sidecar && old.sidecar_lock != new.sidecar_lock) {
            out.push(Change::Tool { part, from, to });
        }
    }
}

/// The changes between two mesh recipes, in a fixed order: tool parts, lengths, openness, cell
/// counts, level, grading, shape, patch layout, raw mesh overrides. Empty exactly when the recipes are equal.
pub fn diff_mesh(old: &MeshRecipe, new: &MeshRecipe) -> Vec<Change> {
    let mut out = Vec::new();
    tool_changes(&old.tool, &new.tool, &mut out);
    let (og, ng) = (&old.geometry, &new.geometry);
    for (axis, from, to) in [
        (Axis::X, og.length_x, ng.length_x),
        (Axis::Y, og.length_y, ng.length_y),
        (Axis::Z, og.length_z, ng.length_z),
    ] {
        if from != to {
            out.push(Change::Length { axis, from, to });
        }
    }
    if og.open != ng.open {
        out.push(Change::DomainOpen {
            from: og.open,
            to: ng.open,
        });
    }
    let (oc, nc) = (&old.cells, &new.cells);
    for (axis, from, to) in [
        (Axis::X, oc.x, nc.x),
        (Axis::Y, oc.y, nc.y),
        (Axis::Z, oc.z, nc.z),
    ] {
        if from != to {
            out.push(Change::Cells { axis, from, to });
        }
    }
    if old.level != new.level {
        out.push(Change::MeshLevel {
            from: old.level,
            to: new.level,
        });
    }
    let ratio = |g: Option<crate::recipe::GradingRecipe>, axis: Axis| {
        g.and_then(|g| if axis == Axis::X { g.x } else { g.y })
            .unwrap_or(1.0)
    };
    for axis in [Axis::X, Axis::Y] {
        let (from, to) = (ratio(old.grading, axis), ratio(new.grading, axis));
        if from != to {
            out.push(Change::Grading { axis, from, to });
        }
    }
    if old.shape != new.shape {
        out.push(Change::Shape {
            from: old.shape.as_ref().map(shape_text),
            to: new.shape.as_ref().map(shape_text),
        });
    }
    if old.patches != new.patches {
        out.push(Change::Layout);
    }
    if old.raw != new.raw {
        out.push(Change::RawMesh);
    }
    out
}

fn faces_text(b: &BoundaryRecipe) -> String {
    let names: Vec<String> = b.faces.iter().map(|f| format!("{f:?}")).collect();
    names.join(", ")
}

fn boundary_changes(
    label: &str,
    old: &BoundaryRecipe,
    new: &BoundaryRecipe,
    out: &mut Vec<Change>,
) {
    let boundary = || label.to_owned();
    if old.kind != new.kind {
        out.push(Change::BoundaryKind {
            boundary: boundary(),
            from: format!("{:?}", old.kind),
            to: format!("{:?}", new.kind),
        });
    }
    if old.faces != new.faces {
        out.push(Change::BoundaryFaces {
            boundary: boundary(),
            from: faces_text(old),
            to: faces_text(new),
        });
    }
    if old.velocity != new.velocity {
        out.push(Change::BoundaryVelocity {
            boundary: boundary(),
            from: old.velocity,
            to: new.velocity,
        });
    }
    if old.profile != new.profile {
        let word = |p: Option<cfdcraft_model::InletProfile>| {
            p.map(|p| match p {
                cfdcraft_model::InletProfile::Uniform => "uniform".to_owned(),
                cfdcraft_model::InletProfile::Parabolic => "parabolic".to_owned(),
            })
        };
        out.push(Change::InletProfile {
            boundary: boundary(),
            from: word(old.profile),
            to: word(new.profile),
        });
    }
    if old.thermal != new.thermal {
        out.push(Change::Thermal {
            boundary: boundary(),
            from: old.thermal.as_ref().map(thermal_text),
            to: new.thermal.as_ref().map(thermal_text),
        });
    }
}

/// A number, or `none` where the study states none.
fn optional(value: Option<f64>) -> String {
    value.map_or_else(|| "none".to_owned(), |v| v.to_string())
}

/// The changes between two physics records; an absent record is the plain steady laminar run.
fn physics_changes(
    old: Option<&PhysicsRecipe>,
    new: Option<&PhysicsRecipe>,
    out: &mut Vec<Change>,
) {
    let plain = PhysicsRecipe::default();
    let (o, n) = (old.unwrap_or(&plain), new.unwrap_or(&plain));
    let step = |t: TimeRecipe| match t {
        TimeRecipe::Steady => None,
        TimeRecipe::Transient { delta_t } => Some(delta_t),
    };
    let init = |i: InitMode| match i {
        InitMode::Uniform => "uniform",
        InitMode::Potential => "potential",
    };
    let mode = |t: TimeRecipe| match t {
        TimeRecipe::Steady => "steady",
        TimeRecipe::Transient { .. } => "transient",
    };
    let parts = [
        (
            PhysicsPart::Time,
            mode(o.time).to_owned(),
            mode(n.time).to_owned(),
        ),
        (
            PhysicsPart::DeltaT,
            optional(step(o.time)),
            optional(step(n.time)),
        ),
        (
            PhysicsPart::Turbulence,
            o.turbulence.model.word().to_owned(),
            n.turbulence.model.word().to_owned(),
        ),
        (
            PhysicsPart::Intensity,
            optional(o.turbulence.intensity),
            optional(n.turbulence.intensity),
        ),
        (
            PhysicsPart::ViscosityRatio,
            optional(o.turbulence.viscosity_ratio),
            optional(n.turbulence.viscosity_ratio),
        ),
        (
            PhysicsPart::ReferenceVelocity,
            optional(o.turbulence.reference_velocity),
            optional(n.turbulence.reference_velocity),
        ),
        (
            PhysicsPart::Buoyancy,
            o.buoyancy
                .as_ref()
                .map_or_else(|| "none".to_owned(), buoyancy_text),
            n.buoyancy
                .as_ref()
                .map_or_else(|| "none".to_owned(), buoyancy_text),
        ),
        (
            PhysicsPart::Init,
            init(o.init).to_owned(),
            init(n.init).to_owned(),
        ),
    ];
    for (what, from, to) in parts {
        if from != to {
            out.push(Change::Physics { what, from, to });
        }
    }
}

/// The changes between two result recipes, in a fixed order: tool parts, the mesh, viscosity,
/// boundaries by label, end time, write interval, the two convergence limits, sample count,
/// raw overrides by label, the physics, then the monitors (probes, lines, surfaces, forces) by
/// place. Empty exactly when the recipes are equal.
pub fn diff_result(old: &ResultRecipe, new: &ResultRecipe) -> Vec<Change> {
    let mut out = Vec::new();
    tool_changes(&old.tool, &new.tool, &mut out);
    if old.mesh_key != new.mesh_key {
        out.push(Change::MeshChanged);
    }
    if old.fluid.kinematic_viscosity != new.fluid.kinematic_viscosity {
        out.push(Change::Viscosity {
            from: old.fluid.kinematic_viscosity,
            to: new.fluid.kinematic_viscosity,
        });
    }
    let labels: BTreeSet<&String> = old.boundaries.keys().chain(new.boundaries.keys()).collect();
    for label in labels {
        match (old.boundaries.get(label), new.boundaries.get(label)) {
            (Some(a), Some(b)) => boundary_changes(label, a, b, &mut out),
            (Some(_), None) => out.push(Change::BoundaryRemoved {
                boundary: label.clone(),
            }),
            (None, Some(_)) => out.push(Change::BoundaryAdded {
                boundary: label.clone(),
            }),
            (None, None) => {}
        }
    }
    let (os, ns) = (&old.solver, &new.solver);
    if os.end_time != ns.end_time {
        out.push(Change::EndTime {
            from: os.end_time,
            to: ns.end_time,
        });
    }
    if os.write_interval != ns.write_interval {
        out.push(Change::WriteInterval {
            from: os.write_interval,
            to: ns.write_interval,
        });
    }
    for (which, from, to) in [
        (Residual::Pressure, os.convergence_p, ns.convergence_p),
        (Residual::Velocity, os.convergence_u, ns.convergence_u),
    ] {
        if from != to {
            out.push(Change::Convergence { which, from, to });
        }
    }
    let (om, nm) = (
        old.monitors.centreline_u_sample_count,
        new.monitors.centreline_u_sample_count,
    );
    if om != nm {
        out.push(Change::CentrelineSamples { from: om, to: nm });
    }
    let ids: BTreeSet<&String> = old.raw.keys().chain(new.raw.keys()).collect();
    for id in ids {
        let (from, to) = (old.raw.get(id), new.raw.get(id));
        if from != to {
            out.push(Change::RawOverride {
                id: id.clone(),
                from: from.map(raw_text),
                to: to.map(raw_text),
            });
        }
    }
    physics_changes(old.physics.as_ref(), new.physics.as_ref(), &mut out);
    let (om, nm) = (&old.monitors, &new.monitors);
    monitor_list("probe", &om.probes, &nm.probes, probe_text, &mut out);
    monitor_list("line", &om.lines, &nm.lines, line_text, &mut out);
    monitor_list(
        "surface",
        &om.surfaces,
        &nm.surfaces,
        surface_text,
        &mut out,
    );
    monitor_list("force", &om.forces, &nm.forces, force_text, &mut out);
    out
}
