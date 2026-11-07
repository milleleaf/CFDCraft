//! The backward-facing step, in two forms: the turbulent step of the Driver and Seegmiller
//! experiment (the NASA turbulence modelling set-up) and the laminar step of the Armaly
//! experiment.

use cfdcraft_model::{Study, TurbulenceModel};
use cfdcraft_shapes::{DomainSpec, ShapeSpec, reference_totals};
use serde_json::{Map, Value, json};

use crate::{
    Recipe, Run, TemplateError, TemplateOptions, TimeChoice, apply, finish, mesh_json, q, refuse,
};

/// Step height of the turbulent step, in tenths of a millimetre (12.7 mm).
const HEIGHT_TENTHS: u64 = 127;
/// Distance from the inlet to the step of the turbulent step, in tenths of a millimetre.
const UPSTREAM_TENTHS: u64 = 16_510;
/// Samples along the floor line and along each profile line.
const FLOOR_SAMPLES: u64 = 1000;
const PROFILE_SAMPLES: u64 = 400;

/// Step heights downstream of the step where the velocity profiles are read.
const PROFILE_STATIONS: [u64; 4] = [1, 4, 6, 10];

/// Armaly experiment: the inlet channel is 5.2 mm high, the step 4.9 mm, and the Reynolds number
/// is taken on the hydraulic diameter of the inlet channel, twice its height.
const INLET_HEIGHT_MM: f64 = 5.2;
const LAMINAR_VISCOSITY: f64 = 1.5e-5;
const LAMINAR_REYNOLDS: f64 = 200.0;
/// The Reynolds numbers where the flow stays two-dimensional and laminar.
const LAMINAR_LOWEST: f64 = 100.0;
const LAMINAR_HIGHEST: f64 = 400.0;

/// A millimetre length given in tenths.
fn tenths_mm(tenths: u64) -> f64 {
    tenths as f64 / 10.0
}

/// The cell counts along the two reference lines, from the shape itself.
fn totals(shape: ShapeSpec) -> Result<(u64, u64), TemplateError> {
    let domain = DomainSpec {
        length_x: 1.0,
        length_y: 1.0,
        length_z: 1.0,
    };
    reference_totals(&shape, &domain).ok_or_else(|| TemplateError::Internal {
        detail: "the step shape has no reference cell counts".to_string(),
    })
}

/// A line of velocity samples, its ends in millimetres.
fn line_mm(x0: f64, y0: f64, x1: f64, y1: f64, samples: u64) -> Value {
    json!({
        "x0": q(x0, "mm"),
        "y0": q(y0, "mm"),
        "x1": q(x1, "mm"),
        "y1": q(y1, "mm"),
        "samples": samples,
        "field": "velocity"
    })
}

/// The turbulent step: Reynolds number 36 000 on the step height, k-omega SST, steady.
pub(crate) fn turbulent(options: &TemplateOptions) -> Result<Study, TemplateError> {
    refuse(options.rayleigh, "rayleigh")?;
    refuse(options.reynolds, "reynolds")?;
    let (cells_x, cells_y) = totals(ShapeSpec::BackwardStep {
        step_height: 0.0127,
        upstream_length: 1.651,
        startup_length: 0.254,
    })?;
    let step_x = tenths_mm(UPSTREAM_TENTHS);
    let mut lines = Map::new();
    // Just above the downstream floor: where the streamwise velocity changes sign is the
    // reattachment point (0.01 step heights up).
    lines.insert(
        "floor".to_string(),
        line_mm(step_x, 0.127, 2286.0, 0.127, FLOOR_SAMPLES),
    );
    for k in PROFILE_STATIONS {
        let x = tenths_mm(UPSTREAM_TENTHS + k * HEIGHT_TENTHS);
        lines.insert(
            format!("profile_x{k}"),
            line_mm(x, 0.0, x, 114.3, PROFILE_SAMPLES),
        );
    }
    let mut study = json!({
        "schema_version": 1,
        "domain": {
            "length_x": q(2286.0, "mm"),
            "length_y": q(114.3, "mm"),
            "length_z": q(12.7, "mm"),
            "open": true,
            "shape": {
                "kind": "backward_step",
                "step_height": q(12.7, "mm"),
                "upstream_length": q(1651.0, "mm"),
                "startup_length": q(254.0, "mm")
            }
        },
        "fluid": {"kinematic_viscosity": q(1.56e-5, "m2/s")},
        "boundary_conditions": {
            "inlet": {"faces": ["XMin"], "kind": "VelocityInlet", "velocity": q(44.2, "m/s")},
            "outlet": {"faces": ["XMax"], "kind": "PressureOutlet"},
            "floor_walls": {"faces": ["YMin"], "kind": "NoSlipWall"},
            "top_wall": {"faces": ["YMax"], "kind": "NoSlipWall"}
        },
        "mesh": mesh_json(cells_x, cells_y, options.mesh),
        "solver": {"convergence": {"p": 1e-4, "U": 1e-6}},
        "monitors": {"centreline_u_sample_count": 64, "lines": lines}
    });
    // The intensity and the viscosity ratio reproduce the inflow values of the reference set-up
    // (k 1.09e-3 m2/s2 and omega 181 728 1/s). The set-up's own documentation gives a different
    // ratio in words, which would not reproduce its numbers.
    let recipe = Recipe {
        model: TurbulenceModel::KOmegaSst,
        intensity: 0.00061,
        viscosity_ratio: 3.846e-4,
        reference_velocity: None,
        time: TimeChoice::Steady,
        steady: Run {
            end: 2000.0,
            interval: 100.0,
        },
        transient: Run {
            end: 0.5,
            interval: 0.05,
        },
        delta_t: 1e-5,
        buoyancy: None,
        potential: true,
        plain_solver: false,
    };
    apply(&mut study, &recipe, options)?;
    finish(&study)
}

/// The laminar step: a parabolic inflow, Reynolds number 200 by default, laminar, steady.
pub(crate) fn laminar(options: &TemplateOptions) -> Result<Study, TemplateError> {
    refuse(options.rayleigh, "rayleigh")?;
    let reynolds = options.reynolds.unwrap_or(LAMINAR_REYNOLDS);
    if !(LAMINAR_LOWEST..=LAMINAR_HIGHEST).contains(&reynolds) {
        return Err(TemplateError::BadOption { option: "reynolds" });
    }
    // The mean inflow speed for U (2 h) / nu = Re.
    let speed = reynolds * LAMINAR_VISCOSITY / (2.0 * INLET_HEIGHT_MM / 1000.0);
    let (cells_x, cells_y) = totals(ShapeSpec::BackwardStep {
        step_height: 0.0049,
        upstream_length: 0.026,
        startup_length: 0.0,
    })?;
    let mut lines = Map::new();
    // 0.01 step heights above the downstream floor, from the step to the outlet.
    lines.insert(
        "floor".to_string(),
        line_mm(26.0, 0.049, 173.0, 0.049, FLOOR_SAMPLES),
    );
    let mut study = json!({
        "schema_version": 1,
        "domain": {
            "length_x": q(173.0, "mm"),
            "length_y": q(10.1, "mm"),
            "length_z": q(4.9, "mm"),
            "open": true,
            "shape": {
                "kind": "backward_step",
                "step_height": q(4.9, "mm"),
                "upstream_length": q(26.0, "mm"),
                "startup_length": q(0.0, "mm")
            }
        },
        "fluid": {"kinematic_viscosity": q(LAMINAR_VISCOSITY, "m2/s")},
        "boundary_conditions": {
            "inlet": {
                "faces": ["XMin"],
                "kind": "VelocityInlet",
                "velocity": q(speed, "m/s"),
                "profile": "parabolic"
            },
            "outlet": {"faces": ["XMax"], "kind": "PressureOutlet"},
            "floor_walls": {"faces": ["YMin"], "kind": "NoSlipWall"},
            "top_wall": {"faces": ["YMax"], "kind": "NoSlipWall"}
        },
        "mesh": mesh_json(cells_x, cells_y, options.mesh),
        "solver": {"convergence": {"p": 1e-4, "U": 1e-6}},
        "monitors": {"centreline_u_sample_count": 64, "lines": lines}
    });
    let recipe = Recipe {
        model: TurbulenceModel::Laminar,
        intensity: 0.05,
        viscosity_ratio: 10.0,
        reference_velocity: None,
        time: TimeChoice::Steady,
        steady: Run {
            end: 5000.0,
            interval: 500.0,
        },
        transient: Run {
            end: 5.0,
            interval: 0.5,
        },
        delta_t: 1e-4,
        buoyancy: None,
        potential: true,
        plain_solver: false,
    };
    apply(&mut study, &recipe, options)?;
    finish(&study)
}
