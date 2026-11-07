use cfdcraft_model::{BoundaryKind, Capabilities};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    EsiV2512,
}

impl Dialect {
    /// What this dialect's writer can run: every boundary kind, one cell through z (the front
    /// and back patches are empty), and one velocity per boundary kind (patch names come from a
    /// fixed table).
    pub fn capabilities(self) -> Capabilities {
        match self {
            Self::EsiV2512 => Capabilities {
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
            },
        }
    }
}

pub const IMAGE: &str = "opencfd/openfoam-default@sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319";
pub const IMAGE_ENV: &str = "bash -lc";
