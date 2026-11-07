//! What the Foundation 14 writer offers, stated once. A setting it cannot write is a named finding
//! when a study asks for it, never written wrong and never dropped.

use cfdcraft_model::{BoundaryKind, Capabilities, RawBackend};

/// The writer runs two-dimensional cases (one cell through z), all six boundary kinds, a parabolic
/// inlet, every turbulence model, steady and transient runs, buoyancy (the fluid module with a
/// Boussinesq equation of state), a potential-flow start, and raw overrides tagged `foundation`.
/// Nothing the model can state is left out, so every limit that is not stated here is the
/// permissive default.
pub fn capabilities() -> Capabilities {
    Capabilities {
        boundary_kinds: vec![
            BoundaryKind::VelocityInlet,
            BoundaryKind::PressureOutlet,
            BoundaryKind::NoSlipWall,
            BoundaryKind::SlipWall,
            BoundaryKind::MovingWall,
            BoundaryKind::Symmetry,
        ],
        requires_single_cell_z: true,
        same_kind_boundaries_must_agree: true,
        raw_backend: Some(RawBackend::Foundation),
        ..Capabilities::unrestricted()
    }
}
