//! The lid-driven cavity at a Reynolds number of 100: a 0.1 m square, the lid moving at 0.1 m/s.

use cfdcraft_model::{Study, TurbulenceModel};
use serde_json::json;

use crate::{
    Recipe, Run, TemplateError, TemplateOptions, TimeChoice, apply, finish, mesh_json, q, refuse,
};

/// Cells along each side at level 0; level 2 gives the 128 by 128 mesh.
const BASE_CELLS: u64 = 32;

fn recipe() -> Recipe {
    Recipe {
        model: TurbulenceModel::Laminar,
        intensity: 0.05,
        viscosity_ratio: 10.0,
        // The lid speed is the reference velocity.
        reference_velocity: None,
        time: TimeChoice::Steady,
        steady: Run {
            end: 5000.0,
            interval: 500.0,
        },
        transient: Run {
            end: 20.0,
            interval: 5.0,
        },
        delta_t: 0.005,
        buoyancy: None,
        potential: true,
        plain_solver: true,
    }
}

pub(crate) fn build(options: &TemplateOptions) -> Result<Study, TemplateError> {
    refuse(options.rayleigh, "rayleigh")?;
    refuse(options.reynolds, "reynolds")?;
    let mut study = json!({
        "schema_version": 1,
        "domain": {
            "length_x": q(0.1, "m"),
            "length_y": q(0.1, "m"),
            "length_z": q(0.05, "m"),
            "open": false
        },
        "fluid": {"kinematic_viscosity": q(1e-4, "m2/s")},
        "boundary_conditions": {
            "lid": {"faces": ["YMax"], "kind": "MovingWall", "velocity": q(0.1, "m/s")},
            "walls": {"faces": ["XMin", "XMax", "YMin"], "kind": "NoSlipWall"}
        },
        "mesh": mesh_json(BASE_CELLS, BASE_CELLS, options.mesh),
        "solver": {"convergence": {"p": 1e-6, "U": 1e-6}},
        "monitors": {"centreline_u_sample_count": 64}
    });
    apply(&mut study, &recipe(), options)?;
    finish(&study)
}
