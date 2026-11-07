//! What the backend can run today, and the same limits checked again at compile time.
//!
//! The limits state what the writer writes from nodes the solver's own sources show. Three things
//! are not offered, each for a reason found in those sources:
//! - a parabolic inlet profile, because the solver's plain inlet takes a constant, a flow rate or
//!   a formula, and a formula becomes compiled code;
//! - buoyancy, because the pinned release has no setup node for the thermal expansion coefficient
//!   of its Boussinesq model: the value can only be given in user code or a formula;
//! - a potential-flow start, because the solver has no such initialisation.

use cfdcraft_backend::CompileFailure;
use cfdcraft_model::{BoundaryKind, Capabilities, InitMode, ResolvedStudy, TurbulenceModel};

/// The limits of the backend. A study that asks for more gets a finding for the setting that asks.
pub fn capabilities() -> Capabilities {
    Capabilities {
        boundary_kinds: vec![
            BoundaryKind::VelocityInlet,
            BoundaryKind::PressureOutlet,
            BoundaryKind::NoSlipWall,
            BoundaryKind::MovingWall,
            BoundaryKind::Symmetry,
        ],
        requires_single_cell_z: true,
        turbulence_models: vec![TurbulenceModel::Laminar, TurbulenceModel::KOmegaSst],
        transient: true,
        potential_init: false,
        buoyancy: false,
        accepts_raw: false,
        parabolic_inlet: false,
        ..Capabilities::unrestricted()
    }
}

fn unsupported(setting: &str, reason: &str) -> CompileFailure {
    CompileFailure::NotSupported {
        setting: setting.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Refuse every setting the writer would otherwise have to drop. The checks that a finding for the
/// same limit already covers repeat here, so a study that skipped the findings still cannot lose a
/// setting silently.
pub fn check_supported(study: &ResolvedStudy) -> Result<(), CompileFailure> {
    if let Some(label) = study.raw_overrides.keys().next() {
        return Err(CompileFailure::Raw {
            label: label.clone(),
            reason: "this backend does not accept raw overrides".to_owned(),
        });
    }
    if let Some(physics) = &study.physics {
        if !capabilities()
            .turbulence_models
            .contains(&physics.turbulence.model)
        {
            return Err(unsupported(
                "turbulence model",
                "only laminar flow and the k-omega SST model are written for this solver",
            ));
        }
        if physics.buoyancy.is_some() {
            return Err(unsupported(
                "buoyancy",
                "the thermal expansion coefficient of the buoyancy model has no setup node in this release",
            ));
        }
        if physics.init != InitMode::Uniform {
            return Err(unsupported(
                "start of the flow",
                "this solver cannot start from a potential-flow solution",
            ));
        }
    }
    let monitors = &study.monitors;
    if !monitors.surfaces.is_empty() || !monitors.forces.is_empty() {
        return Err(unsupported(
            "monitors",
            "surfaces and force coefficients are not written for this solver",
        ));
    }
    Ok(())
}
