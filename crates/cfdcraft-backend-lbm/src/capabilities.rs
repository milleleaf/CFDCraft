//! What the lattice-Boltzmann backend can run today: a laminar study, steady (as a run in time that
//! stops once the flow has settled) or transient, on a box of equal square cells or in a channel
//! with a step or a cylinder, with walls, a moving wall, slip or symmetry planes, a velocity inlet
//! (uniform or parabolic) and a pressure outlet, and with buoyancy and fixed wall temperatures.
//! Everything else is a named finding of the model's checks or of the writer, and none of it is
//! mapped to something narrower.

use cfdcraft_model::{BoundaryKind, Capabilities, TurbulenceModel};

pub fn capabilities() -> Capabilities {
    Capabilities {
        boundary_kinds: vec![
            BoundaryKind::NoSlipWall,
            BoundaryKind::SlipWall,
            BoundaryKind::MovingWall,
            BoundaryKind::Symmetry,
            BoundaryKind::VelocityInlet,
            BoundaryKind::PressureOutlet,
        ],
        // The study describes a two-dimensional case; the backend makes the slab itself.
        requires_single_cell_z: true,
        turbulence_models: vec![TurbulenceModel::Laminar],
        transient: true,
        potential_init: false,
        buoyancy: true,
        uniform_square_cells: true,
        accepts_raw: false,
        parabolic_inlet: true,
        ..Capabilities::unrestricted()
    }
}
