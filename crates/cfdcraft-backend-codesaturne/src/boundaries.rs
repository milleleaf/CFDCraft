//! The boundary zones of the setup: one zone per mesh group, named by the part of the boundary it
//! stands on, so a label the author chose never reaches the case.
//!
//! A study face becomes a wall (fixed or moving), a symmetry zone, an inlet or an outlet. The
//! obstacle of a cylinder channel and the start-up section of a step are zones of their own. The
//! inlet carries a constant speed along the inward normal and, with a turbulence model, constant
//! turbulence values: a velocity profile would need a formula, which is never written.

use crate::{
    markers::{ZONE_NAME_ATTRIBUTE, ZONE_NUMBER_ATTRIBUTE},
    models::{InletValues, inlet_values, model_of},
    setup::{Writer, Z_FACES_GROUP, invalid, number, unsupported},
};
use cfdcraft_backend::CompileFailure;
use cfdcraft_model::{
    BoundaryKind, Face, InletProfile, ResolvedBoundary, ResolvedStudy, TurbulenceModel,
    velocity_vector,
};
use cfdcraft_shapes::{Layout, Side};

/// The group of the obstacle surface.
pub const OBSTACLE_GROUP: &str = "obstacle";

/// The group of the symmetry sections of a start-up block.
pub const STARTUP_GROUP: &str = "startup";

/// The parts of the boundary in the order their groups and zones are numbered.
pub const SIDE_ORDER: [Side; 6] = [
    Side::XMin,
    Side::XMax,
    Side::YMin,
    Side::YMax,
    Side::Obstacle,
    Side::Startup,
];

/// The mesh group, and the zone, of a part of the boundary; the z faces have their own.
pub fn side_group(side: Side) -> Option<&'static str> {
    match side {
        Side::XMin => Some("x_min"),
        Side::XMax => Some("x_max"),
        Side::YMin => Some("y_min"),
        Side::YMax => Some("y_max"),
        Side::Obstacle => Some(OBSTACLE_GROUP),
        Side::Startup => Some(STARTUP_GROUP),
        Side::FrontBack => None,
    }
}

/// The study face that stands on a part of the boundary; the start-up section has none.
fn side_face(side: Side) -> Option<Face> {
    match side {
        Side::XMin => Some(Face::XMin),
        Side::XMax => Some(Face::XMax),
        Side::YMin => Some(Face::YMin),
        Side::YMax => Some(Face::YMax),
        Side::Obstacle => Some(Face::Obstacle),
        Side::Startup | Side::FrontBack => None,
    }
}

/// What a zone is, with the constants its detail node holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    FixedWall,
    MovingWall([f64; 3]),
    Symmetry,
    Inlet {
        speed: f64,
        turbulence: Option<InletValues>,
    },
    Outlet,
}

/// One boundary zone of the setup.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub label: &'static str,
    pub detail: Detail,
}

impl Zone {
    /// The nature attribute of the zone.
    pub fn nature(&self) -> &'static str {
        match self.detail {
            Detail::FixedWall | Detail::MovingWall(_) => "wall",
            Detail::Symmetry => "symmetry",
            Detail::Inlet { .. } => "inlet",
            Detail::Outlet => "outlet",
        }
    }
}

fn present(layout: &Layout, side: Side) -> bool {
    layout.quads.iter().any(|q| q.side == side)
}

fn detail_of(
    study: &ResolvedStudy,
    boundary: &ResolvedBoundary,
    face: Face,
) -> Result<Detail, CompileFailure> {
    if boundary.profile == Some(InletProfile::Parabolic) {
        return Err(unsupported(
            "inlet profile",
            "a parabolic profile needs a formula, which is never written",
        ));
    }
    if boundary.thermal.is_some() {
        return Err(unsupported(
            "thermal condition",
            "no thermal model is written for this solver",
        ));
    }
    let driven = || {
        boundary
            .velocity
            .filter(|speed| speed.is_finite() && *speed != 0.0)
    };
    if face == Face::Obstacle && boundary.kind != BoundaryKind::NoSlipWall {
        return Err(unsupported(
            "boundary kind",
            "the obstacle must be a no-slip wall for this solver",
        ));
    }
    match boundary.kind {
        BoundaryKind::NoSlipWall => Ok(Detail::FixedWall),
        BoundaryKind::MovingWall => {
            let speed =
                driven().ok_or_else(|| invalid("a moving wall needs a finite nonzero velocity"))?;
            Ok(Detail::MovingWall(velocity_vector(
                boundary.kind,
                face,
                speed,
            )))
        }
        BoundaryKind::Symmetry => Ok(Detail::Symmetry),
        BoundaryKind::PressureOutlet => Ok(Detail::Outlet),
        BoundaryKind::VelocityInlet => {
            let speed = driven()
                .filter(|speed| *speed > 0.0)
                .ok_or_else(|| invalid("an inlet needs a finite speed above zero"))?;
            let turbulence = if model_of(study) == TurbulenceModel::Laminar {
                None
            } else {
                Some(inlet_values(study, speed)?)
            };
            Ok(Detail::Inlet { speed, turbulence })
        }
        BoundaryKind::SlipWall => Err(unsupported(
            "boundary kind",
            "a slip wall is not written for this solver; use a symmetry boundary",
        )),
    }
}

/// The zones of the setup, in the order of their code numbers: the parts of the boundary the
/// layout has, then the two faces along z.
pub fn zones(study: &ResolvedStudy, layout: &Layout) -> Result<Vec<Zone>, CompileFailure> {
    for boundary in study.boundary_conditions.values() {
        for face in &boundary.faces {
            let has = SIDE_ORDER
                .iter()
                .any(|side| side_face(*side) == Some(*face) && present(layout, *side));
            if !has {
                return Err(invalid("a boundary names a face the domain does not have"));
            }
        }
    }
    let mut zones = Vec::with_capacity(SIDE_ORDER.len() + 1);
    for side in SIDE_ORDER {
        let Some(label) = side_group(side) else {
            continue;
        };
        if !present(layout, side) {
            continue;
        }
        let detail = match side_face(side) {
            None => Detail::Symmetry,
            Some(face) => {
                let boundary = study
                    .boundary_conditions
                    .values()
                    .find(|b| b.faces.contains(&face))
                    .ok_or_else(|| invalid("every part of the boundary needs a condition"))?;
                detail_of(study, boundary, face)?
            }
        };
        zones.push(Zone { label, detail });
    }
    zones.push(Zone {
        label: Z_FACES_GROUP,
        detail: Detail::Symmetry,
    });
    Ok(zones)
}

/// The zone list, then one detail node per zone.
pub(crate) fn write_zones(w: &mut Writer, zones: &[Zone]) -> Result<(), CompileFailure> {
    w.open("boundary_conditions", &[]);
    for (index, zone) in zones.iter().enumerate() {
        let code = (index + 1).to_string();
        w.leaf(
            "boundary",
            &[
                (ZONE_NAME_ATTRIBUTE, zone.label),
                (ZONE_NUMBER_ATTRIBUTE, code.as_str()),
                ("nature", zone.nature()),
            ],
            zone.label,
        );
    }
    for zone in zones {
        let attributes = [("field_id", "none"), (ZONE_NAME_ATTRIBUTE, zone.label)];
        match &zone.detail {
            Detail::MovingWall(vector) => {
                w.open("wall", &attributes);
                w.open("velocity_pressure", &[("choice", "on")]);
                for (component, value) in vector.iter().enumerate() {
                    let component = component.to_string();
                    w.leaf(
                        "dirichlet",
                        &[("component", component.as_str()), ("name", "velocity")],
                        &number(*value)?,
                    );
                }
                w.close("velocity_pressure");
                w.close("wall");
            }
            Detail::FixedWall => {
                w.open("wall", &attributes);
                w.empty("velocity_pressure", &[("choice", "off")]);
                w.close("wall");
            }
            Detail::Inlet { speed, turbulence } => {
                w.open("inlet", &attributes);
                w.open(
                    "velocity_pressure",
                    &[("choice", "norm"), ("direction", "normal")],
                );
                w.leaf("norm", &[], &number(*speed)?);
                w.close("velocity_pressure");
                if let Some(values) = turbulence {
                    w.open("turbulence", &[("choice", "turbulent_intensity")]);
                    w.leaf(
                        "hydraulic_diameter",
                        &[],
                        &number(values.hydraulic_diameter)?,
                    );
                    w.leaf(
                        "turbulent_intensity",
                        &[],
                        &number(values.intensity_percent)?,
                    );
                    w.close("turbulence");
                }
                w.close("inlet");
            }
            Detail::Symmetry | Detail::Outlet => w.empty(zone.nature(), &attributes),
        }
    }
    w.close("boundary_conditions");
    Ok(())
}
