use cfdcraft_model::{BoundaryKind, Capabilities, RawBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    EsiV2512,
}

impl Dialect {
    /// The stable identifier of this dialect, as recorded in a stage key.
    pub fn id(self) -> &'static str {
        match self {
            Self::EsiV2512 => "esi-v2512",
        }
    }

    /// The image reference this dialect runs in, pinned by digest.
    pub fn image(self) -> &'static str {
        match self {
            Self::EsiV2512 => IMAGE,
        }
    }

    /// What this dialect's writer can run: every boundary kind, one cell through z (the front
    /// and back patches are empty), one velocity per boundary kind (patch names come from a
    /// fixed table), and raw overrides tagged `esi`.
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
                raw_backend: Some(RawBackend::Esi),
            },
        }
    }
}

pub const IMAGE: &str = "opencfd/openfoam-default@sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319";
pub const IMAGE_ENV: &str = "bash -lc";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialect_states_its_identifier_and_image() {
        assert_eq!(Dialect::EsiV2512.id(), "esi-v2512");
        assert_eq!(Dialect::EsiV2512.image(), IMAGE);
        assert!(Dialect::EsiV2512.image().contains("@sha256:"));
    }

    #[test]
    fn the_dialect_runs_overrides_tagged_for_its_backend() {
        assert_eq!(
            Dialect::EsiV2512.capabilities().raw_backend,
            Some(RawBackend::Esi)
        );
    }
}
