//! What the SU2 writer offers, stated once. Every limit is a named finding when a study asks for
//! more, so nothing is dropped silently.

use cfdcraft_model::{BoundaryKind, Capabilities, TurbulenceModel};

/// SU2 is a two-dimensional solver here (one cell through z), runs the laminar and the k-omega SST
/// models, steady and in time, starts from a uniform field only, supports buoyancy and a parabolic
/// inlet (through a profile file), has no raw overrides and one velocity per boundary kind.
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
        raw_backend: None,
        turbulence_models: vec![TurbulenceModel::Laminar, TurbulenceModel::KOmegaSst],
        transient: true,
        potential_init: false,
        buoyancy: true,
        uniform_square_cells: false,
        accepts_raw: false,
        parabolic_inlet: true,
    }
}
