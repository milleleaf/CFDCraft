//! The structured difference between two recipes: which setting changed, from what to what.
//!
//! A key says a stage is out of date; the diff says why. A change is plain data, never a
//! sentence, so a caller can turn it into text in any language or read it as JSON. Every field
//! of both recipes has a change that names it, so the diff is empty exactly when the recipes
//! are equal.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ToolIdentity;
use crate::recipe::{BoundaryRecipe, MeshRecipe, ResultRecipe};

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
    /// The patch layout of the mesh differs (a boundary's kind or faces).
    Layout,
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
/// counts, patch layout. Empty exactly when the recipes are equal.
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
    if old.patches != new.patches {
        out.push(Change::Layout);
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
}

/// The changes between two result recipes, in a fixed order: tool parts, the mesh, viscosity,
/// boundaries by label, end time, write interval, the two convergence limits, sample count.
/// Empty exactly when the recipes are equal.
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
    out
}
