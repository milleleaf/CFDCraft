//! Air in a differentially heated square cavity: a vertical wall held 0.5 K above and the other
//! 0.5 K below 300 K, the floor and roof insulated, buoyancy by the Boussinesq model. The
//! Rayleigh number is the knob; the viscosity follows from it.

use cfdcraft_model::{Study, TurbulenceModel};
use serde_json::{Map, Value, json};

use crate::{
    Recipe, Run, TemplateError, TemplateOptions, TimeChoice, apply, finish, mesh_json, q, refuse,
};

/// Gravity in m/s2.
const GRAVITY: f64 = 9.81;
/// Thermal expansion coefficient in 1/K.
const BETA: f64 = 3e-3;
/// Temperature difference between the hot and the cold wall in K.
const DELTA_T: f64 = 1.0;
/// Side of the cavity in m.
const SIDE: f64 = 1.0;
/// Prandtl number of air.
const PRANDTL: f64 = 0.71;
/// Turbulent Prandtl number.
const TURBULENT_PRANDTL: f64 = 0.85;
/// The Rayleigh number when the user does not choose one.
const DEFAULT_RAYLEIGH: f64 = 1e5;
/// The Rayleigh numbers a template accepts.
const LOWEST_RAYLEIGH: f64 = 1e2;
const HIGHEST_RAYLEIGH: f64 = 1e8;
/// Cells along each side at level 0.
const BASE_CELLS: u64 = 40;
/// Growth of the cells from each wall toward the middle, per cell.
const WALL_GRADING: f64 = 1.088;
/// Samples along each mid-plane line.
const LINE_SAMPLES: u64 = 201;

/// The kinematic viscosity for which the cavity has Rayleigh number `ra`:
/// nu = sqrt(g beta dT L^3 Pr / Ra).
pub fn viscosity_for_rayleigh(ra: f64) -> f64 {
    (GRAVITY * BETA * DELTA_T * SIDE.powi(3) * PRANDTL / ra).sqrt()
}

/// A line of velocity samples, its ends in metres.
fn line_m(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!({
        "x0": q(x0, "m"),
        "y0": q(y0, "m"),
        "x1": q(x1, "m"),
        "y1": q(y1, "m"),
        "samples": LINE_SAMPLES,
        "field": "velocity"
    })
}

pub(crate) fn build(options: &TemplateOptions) -> Result<Study, TemplateError> {
    refuse(options.reynolds, "reynolds")?;
    let rayleigh = options.rayleigh.unwrap_or(DEFAULT_RAYLEIGH);
    if !(LOWEST_RAYLEIGH..=HIGHEST_RAYLEIGH).contains(&rayleigh) {
        return Err(TemplateError::BadOption { option: "rayleigh" });
    }
    let mut lines = Map::new();
    lines.insert(
        "mid_vertical".to_string(),
        line_m(SIDE / 2.0, 0.0, SIDE / 2.0, SIDE),
    );
    lines.insert(
        "mid_horizontal".to_string(),
        line_m(0.0, SIDE / 2.0, SIDE, SIDE / 2.0),
    );
    let mut mesh = mesh_json(BASE_CELLS, BASE_CELLS, options.mesh);
    mesh["grading"] = json!({"x": WALL_GRADING, "y": WALL_GRADING});
    let mut study = json!({
        "schema_version": 1,
        "domain": {
            "length_x": q(SIDE, "m"),
            "length_y": q(SIDE, "m"),
            "length_z": q(0.1, "m"),
            "open": false
        },
        "fluid": {"kinematic_viscosity": q(viscosity_for_rayleigh(rayleigh), "m2/s")},
        "boundary_conditions": {
            "hot": {
                "faces": ["XMin"],
                "kind": "NoSlipWall",
                "thermal": {"kind": "fixed_temperature", "value": q(300.5, "K")}
            },
            "cold": {
                "faces": ["XMax"],
                "kind": "NoSlipWall",
                "thermal": {"kind": "fixed_temperature", "value": q(299.5, "K")}
            },
            "adiabatic": {
                "faces": ["YMin", "YMax"],
                "kind": "NoSlipWall",
                "thermal": {"kind": "adiabatic"}
            }
        },
        "mesh": mesh,
        "solver": {"convergence": {"p": 1e-5, "U": 1e-5}},
        "monitors": {
            "centreline_u_sample_count": 64,
            "lines": lines,
            "surfaces": {
                "hot_flux": {"boundary": "hot", "quantity": "heat_flux"},
                "cold_flux": {"boundary": "cold", "quantity": "heat_flux"}
            }
        }
    });
    let recipe = Recipe {
        model: TurbulenceModel::Laminar,
        intensity: 0.05,
        viscosity_ratio: 10.0,
        // A closed cavity has no inlet or moving wall, so the buoyancy velocity scale is the
        // speed the inflow turbulence values are built from.
        reference_velocity: Some((GRAVITY * BETA * DELTA_T * SIDE).sqrt()),
        time: TimeChoice::Steady,
        steady: Run {
            end: 20000.0,
            interval: 1000.0,
        },
        transient: Run {
            end: 3000.0,
            interval: 300.0,
        },
        delta_t: 0.1,
        buoyancy: Some(json!({
            "gravity": GRAVITY,
            "beta": BETA,
            "reference_temperature": q(300.0, "K"),
            "prandtl": PRANDTL,
            "turbulent_prandtl": TURBULENT_PRANDTL
        })),
        // A closed cavity has no flow to start from.
        potential: false,
        plain_solver: false,
    };
    apply(&mut study, &recipe, options)?;
    finish(&study)
}
