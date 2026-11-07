//! The cylinder benchmark: whether a study is the Schafer and Turek 2D-2 set-up, and the verdict
//! for the files of a finished run.

use cfdcraft_gate::{Benchmark, GateReport, Mismatch, ResultFiles};
use cfdcraft_model::{
    BoundaryKind, Face, InletProfile, MonitorField, ResolvedShape, ResolvedStudy, ResolvedTime,
};

use crate::data::{ID, LoadError, Reference, Tolerances, judge_all};
use crate::files::{FORCE, FRONT, REAR};
use crate::metrics::measure;

/// The channel, in metres.
const LENGTH_X: f64 = 2.2;
const LENGTH_Y: f64 = 0.41;
const DIAMETER: f64 = 0.1;
const CENTRE: [f64; 2] = [0.2, 0.2];
/// The kinematic viscosity in m2/s and the mean inflow speed in m/s.
const VISCOSITY: f64 = 1e-3;
const MEAN_SPEED: f64 = 1.0;
/// Lengths and speeds must agree to this fraction; probe points to this many metres.
const RELATIVE: f64 = 0.01;
const POINT: f64 = 1e-6;

fn miss(what: &str, found: impl Into<String>, expected: impl Into<String>) -> Mismatch {
    Mismatch {
        what: what.to_owned(),
        found: found.into(),
        expected: expected.into(),
    }
}

/// Record a mismatch when `found` is not within `RELATIVE` of `expected`.
fn near(out: &mut Vec<Mismatch>, what: &str, found: f64, expected: f64, unit: &str) {
    if (found - expected).abs() > RELATIVE * expected.abs() {
        out.push(miss(
            what,
            format!("{found} {unit}"),
            format!("{expected} {unit}"),
        ));
    }
}

fn on_cylinder(study: &ResolvedStudy, boundary: &str) -> bool {
    study
        .boundary_conditions
        .get(boundary)
        .is_some_and(|b| b.faces.contains(&Face::Obstacle))
}

/// The cylinder benchmark with the allowances it gates by.
#[derive(Debug, Clone)]
pub struct CylinderBenchmark {
    reference: Reference,
    tolerances: Tolerances,
}

impl CylinderBenchmark {
    /// The benchmark with the given allowances, for example from an override.
    pub fn new(tolerances: Tolerances) -> Result<Self, LoadError> {
        Ok(Self {
            reference: Reference::embedded()?,
            tolerances,
        })
    }

    /// The benchmark with the shipped allowances.
    pub fn shipped() -> Result<Self, LoadError> {
        Self::new(Tolerances::default_embedded()?)
    }

    fn check_flow(study: &ResolvedStudy, out: &mut Vec<Mismatch>) {
        near(
            out,
            "kinematic viscosity",
            study.fluid.kinematic_viscosity,
            VISCOSITY,
            "m2/s",
        );
        let inlet = study
            .boundary_conditions
            .values()
            .find(|b| b.kind == BoundaryKind::VelocityInlet && b.faces.contains(&Face::XMin));
        match inlet {
            None => out.push(miss(
                "inlet",
                "no velocity inlet on the XMin face",
                "a velocity inlet on the XMin face",
            )),
            Some(b) => {
                if b.profile != Some(InletProfile::Parabolic) {
                    out.push(miss("inlet profile", "uniform", "parabolic"));
                }
                match b.velocity {
                    Some(speed) => near(out, "mean inflow speed", speed, MEAN_SPEED, "m/s"),
                    None => out.push(miss("mean inflow speed", "none", "1 m/s")),
                }
            }
        }
        let transient = matches!(
            study.physics.as_ref().map(|p| p.time),
            Some(ResolvedTime::Transient { .. })
        );
        if !transient {
            out.push(miss("time", "steady", "transient"));
        }
    }

    fn check_geometry(study: &ResolvedStudy, out: &mut Vec<Mismatch>) {
        near(out, "channel length", study.domain.length_x, LENGTH_X, "m");
        near(out, "channel height", study.domain.length_y, LENGTH_Y, "m");
        match study.domain.shape {
            Some(ResolvedShape::CylinderChannel {
                diameter,
                centre_x,
                centre_y,
            }) => {
                near(out, "cylinder diameter", diameter, DIAMETER, "m");
                near(out, "cylinder centre x", centre_x, CENTRE[0], "m");
                near(out, "cylinder centre y", centre_y, CENTRE[1], "m");
            }
            Some(ResolvedShape::BackwardStep { .. }) => {
                out.push(miss("domain shape", "backward step", "cylinder channel"));
            }
            None => out.push(miss("domain shape", "box", "cylinder channel")),
        }
    }

    fn check_monitors(&self, study: &ResolvedStudy, out: &mut Vec<Mismatch>) {
        let wanted = "present";
        match study.monitors.forces.iter().find(|f| f.label == FORCE) {
            None => out.push(miss("force monitor", "missing", wanted)),
            Some(f) => {
                if !on_cylinder(study, &f.boundary) {
                    out.push(miss(
                        "force monitor boundary",
                        f.boundary.clone(),
                        "the cylinder surface",
                    ));
                }
                near(
                    out,
                    "force reference speed",
                    f.reference_velocity,
                    1.0,
                    "m/s",
                );
                near(
                    out,
                    "force reference length",
                    f.reference_length,
                    DIAMETER,
                    "m",
                );
            }
        }
        let definition = &self.reference.definition;
        for (label, point) in [
            (FRONT, definition.front_point),
            (REAR, definition.rear_point),
        ] {
            let what = format!("pressure probe {label}");
            let place = format!(
                "a pressure probe at ({}, {}) on the cylinder",
                point[0], point[1]
            );
            let Some(p) = study.monitors.probes.iter().find(|p| p.label == label) else {
                out.push(miss(&what, "missing", place));
                continue;
            };
            let on_surface = p.boundary.as_deref().is_some_and(|b| on_cylinder(study, b));
            let there = (p.x - point[0]).abs() <= POINT && (p.y - point[1]).abs() <= POINT;
            if p.field != MonitorField::Pressure || !on_surface || !there {
                out.push(miss(&what, format!("at ({}, {})", p.x, p.y), place));
            }
        }
    }
}

impl Benchmark for CylinderBenchmark {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Flow around a cylinder, Re 100, 2D-2"
    }

    fn reference(&self) -> &'static str {
        "Schafer and Turek (1996), benchmark 2D-2"
    }

    fn matches(&self, study: &ResolvedStudy) -> Result<(), Vec<Mismatch>> {
        let mut out = Vec::new();
        Self::check_geometry(study, &mut out);
        Self::check_flow(study, &mut out);
        self.check_monitors(study, &mut out);
        if out.is_empty() { Ok(()) } else { Err(out) }
    }

    fn compare(&self, files: &dyn ResultFiles, study: &ResolvedStudy) -> GateReport {
        match measure(files, study, &self.tolerances) {
            Ok(m) => judge_all(
                &self.reference,
                &self.tolerances,
                m.cd_max,
                m.cl_max,
                m.st,
                m.delta_p,
            ),
            Err(refusal) => GateReport::failed_named(ID, refusal.to_string()),
        }
    }

    fn quantities(&self, files: &dyn ResultFiles, study: &ResolvedStudy) -> Vec<(String, f64)> {
        let Ok(m) = measure(files, study, &self.tolerances) else {
            return Vec::new();
        };
        vec![
            ("bench.cylinder.st".to_owned(), m.st),
            ("bench.cylinder.cd_max".to_owned(), m.cd_max),
            ("bench.cylinder.cl_max".to_owned(), m.cl_max),
            ("bench.cylinder.delta_p".to_owned(), m.delta_p),
            ("bench.cylinder.cycles".to_owned(), m.cycles as f64),
        ]
    }
}
