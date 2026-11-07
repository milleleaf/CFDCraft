//! Flow around a cylinder in a channel at a Reynolds number of 100 (mean inflow speed 1 m/s,
//! cylinder diameter 0.1 m, viscosity 0.001 m2/s): laminar vortex shedding, run in time.

use cfdcraft_model::{Study, TurbulenceModel};
use cfdcraft_shapes::{DomainSpec, ShapeSpec, reference_totals};
use serde_json::json;

use crate::{
    Recipe, Run, TemplateError, TemplateOptions, TimeChoice, apply, finish, mesh_json, q, refuse,
};

pub(crate) fn build(options: &TemplateOptions) -> Result<Study, TemplateError> {
    refuse(options.rayleigh, "rayleigh")?;
    refuse(options.reynolds, "reynolds")?;
    let domain = DomainSpec {
        length_x: 2.2,
        length_y: 0.41,
        length_z: 0.01,
    };
    let shape = ShapeSpec::CylinderChannel {
        diameter: 0.1,
        centre_x: 0.2,
        centre_y: 0.2,
    };
    let (cells_x, cells_y) =
        reference_totals(&shape, &domain).ok_or_else(|| TemplateError::Internal {
            detail: "the cylinder shape has no reference cell counts".to_string(),
        })?;
    let mut study = json!({
        "schema_version": 1,
        "domain": {
            "length_x": q(2200.0, "mm"),
            "length_y": q(410.0, "mm"),
            "length_z": q(10.0, "mm"),
            "open": true,
            "shape": {
                "kind": "cylinder_channel",
                "diameter": q(100.0, "mm"),
                "centre_x": q(200.0, "mm"),
                "centre_y": q(200.0, "mm")
            }
        },
        "fluid": {"kinematic_viscosity": q(1e-3, "m2/s")},
        "boundary_conditions": {
            "inlet": {
                "faces": ["XMin"],
                "kind": "VelocityInlet",
                "velocity": q(1.0, "m/s"),
                "profile": "parabolic"
            },
            "outlet": {"faces": ["XMax"], "kind": "PressureOutlet"},
            "channel_walls": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall"},
            "cylinder": {"faces": ["Obstacle"], "kind": "NoSlipWall"}
        },
        "mesh": mesh_json(cells_x, cells_y, options.mesh),
        "solver": {"convergence": {"p": 1e-6, "U": 1e-6}},
        "monitors": {
            "centreline_u_sample_count": 64,
            "forces": {
                "cylinder_forces": {
                    "boundary": "cylinder",
                    "reference_velocity": q(1.0, "m/s"),
                    "reference_length": q(100.0, "mm")
                }
            },
            // Pressure on the cylinder at its front and rear, 0.05 m ahead of and behind the axis.
            "probes": {
                "front": {
                    "x": q(150.0, "mm"),
                    "y": q(200.0, "mm"),
                    "field": "pressure",
                    "boundary": "cylinder"
                },
                "rear": {
                    "x": q(250.0, "mm"),
                    "y": q(200.0, "mm"),
                    "field": "pressure",
                    "boundary": "cylinder"
                }
            }
        }
    });
    let recipe = Recipe {
        model: TurbulenceModel::Laminar,
        intensity: 0.05,
        viscosity_ratio: 10.0,
        reference_velocity: None,
        time: TimeChoice::Transient,
        // A steady run counts iterations, not seconds.
        steady: Run {
            end: 2000.0,
            interval: 200.0,
        },
        // About 24 shedding periods at a Strouhal number of 0.3.
        transient: Run {
            end: 8.0,
            interval: 0.5,
        },
        delta_t: 1e-3,
        buoyancy: None,
        potential: true,
        plain_solver: false,
    };
    apply(&mut study, &recipe, options)?;
    finish(&study)
}
