//! The inlet profile file of a parabolic inlet.
//!
//! The file lists, for every node of the inlet marker, the position, the temperature, the speed and
//! the flow direction (and the two turbulence values when the model has them). The speed at a node
//! is the plain polynomial the study states: zero at both walls and one and a half times the mean
//! speed in the middle. Nothing in the file is a formula the solver evaluates.
//!
//! Format, read in the solver's profile reader: `NMARK=`, then per marker `MARKER_TAG=`, `NROW=`,
//! `NCOL=` (the coordinates and the data columns), an optional header line that starts with
//! `# COORD`, and one line per node.

use crate::{
    config::{inflow_turbulence, initial_temperature, num, speed_and_direction},
    mesh::{Marker, coordinate, invalid, marker_nodes},
};
use cfdcraft_backend::CompileFailure;
use cfdcraft_blockmesh::Mesh2d;
use cfdcraft_model::{BoundaryKind, InletProfile, ResolvedShape, ResolvedStudy};
use cfdcraft_shapes::Side;
use std::fmt::Write;

/// The columns of the profile file, in order: the coordinates, then the data.
pub const COLUMNS: &str = "# COORD-X COORD-Y TEMPERATURE VELOCITY NORMAL-X NORMAL-Y";
/// The two extra columns of a model with turbulent kinetic energy and dissipation.
pub const COLUMNS_TURBULENCE: &str = " TKE DISSIPATION";

/// Whether any velocity inlet of the study has a parabolic profile.
pub fn parabolic_requested(study: &ResolvedStudy) -> bool {
    study.boundary_conditions.values().any(|b| {
        b.kind == BoundaryKind::VelocityInlet && b.profile == Some(InletProfile::Parabolic)
    })
}

/// Where the inlet starts on y and how tall it is. The inlet of a step is the narrow channel above
/// the step; any other inlet spans the whole height.
pub fn inlet_extent(study: &ResolvedStudy) -> (f64, f64) {
    match study.domain.shape {
        Some(ResolvedShape::BackwardStep { step_height, .. }) => {
            (step_height, study.domain.length_y - step_height)
        }
        _ => (0.0, study.domain.length_y),
    }
}

/// The speed at height `y` of a parabola with mean speed `mean` over `[origin, origin + height]`.
pub fn profile_speed(mean: f64, origin: f64, height: f64, y: f64) -> f64 {
    let s = y - origin;
    6.0 * mean * s / height - 6.0 * mean * s * s / (height * height)
}

/// The profile file text, or `None` when no inlet is parabolic.
pub fn profile_text(
    study: &ResolvedStudy,
    mesh: &Mesh2d,
    markers: &[Marker],
) -> Result<Option<String>, CompileFailure> {
    if !parabolic_requested(study) {
        return Ok(None);
    }
    let marker = markers
        .iter()
        .find(|m| m.kind == BoundaryKind::VelocityInlet)
        .ok_or_else(|| invalid("a parabolic inlet needs a velocity inlet"))?;
    if !marker
        .sides
        .iter()
        .all(|s| matches!(s, Side::XMin | Side::XMax))
    {
        return Err(invalid(
            "a parabolic inlet is supported on the left or the right face only",
        ));
    }
    let (mean, d) = speed_and_direction(marker.velocity.unwrap_or([0.0; 3]));
    let (origin, height) = inlet_extent(study);
    let temperature = num(initial_temperature(study));
    let turbulence = inflow_turbulence(study);
    let columns = if turbulence.is_some() { 8 } else { 6 };
    let nodes = marker_nodes(mesh, marker)?;
    let mut text = String::new();
    let _ = writeln!(text, "NMARK= 1");
    let _ = writeln!(text, "MARKER_TAG= {}", marker.name);
    let _ = writeln!(text, "NROW= {}", nodes.len());
    let _ = writeln!(text, "NCOL= {columns}");
    let _ = write!(text, "{COLUMNS}");
    if turbulence.is_some() {
        let _ = write!(text, "{COLUMNS_TURBULENCE}");
    }
    let _ = writeln!(text);
    for n in nodes {
        let [x, y] = mesh
            .nodes
            .get(n as usize)
            .copied()
            .ok_or_else(|| invalid("a boundary edge names a node that does not exist"))?;
        // The speed is taken at the coordinate as written, so the file agrees with the mesh text.
        let (x_text, y_text) = (coordinate(x), coordinate(y));
        let at = y_text.parse().unwrap_or(y);
        let speed = profile_speed(mean, origin, height, at).max(0.0);
        let _ = write!(
            text,
            "{x_text} {y_text} {temperature} {} {} {}",
            num(speed),
            num(d[0]),
            num(d[1]),
        );
        if let Some((k, omega)) = turbulence {
            let _ = write!(text, " {} {}", num(k), num(omega));
        }
        let _ = writeln!(text);
    }
    Ok(Some(text))
}
