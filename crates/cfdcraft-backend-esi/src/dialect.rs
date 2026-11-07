use cfdcraft_model::{BoundaryKind, Capabilities, RawBackend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    EsiV2512,
    EsiV2606,
}

impl Dialect {
    /// Every dialect of this crate, the default one first.
    pub const ALL: [Dialect; 2] = [Dialect::EsiV2512, Dialect::EsiV2606];

    /// The stable identifier of this dialect, as recorded in a stage key.
    pub fn id(self) -> &'static str {
        match self {
            Self::EsiV2512 => "esi-v2512",
            Self::EsiV2606 => "esi-v2606",
        }
    }

    /// The image reference this dialect runs in, pinned by digest.
    pub fn image(self) -> &'static str {
        match self {
            Self::EsiV2512 => IMAGE,
            Self::EsiV2606 => IMAGE_V2606,
        }
    }

    /// What this dialect's writer can run: every boundary kind, one cell through z (the front
    /// and back patches are empty), one velocity per boundary kind (patch names come from a
    /// fixed table), and raw overrides tagged `esi`. Every other limit is permissive: the writer
    /// runs all four turbulence models, both time modes, buoyancy, potential-flow starts, graded
    /// cells and parabolic inlets.
    pub fn capabilities(self) -> Capabilities {
        match self {
            Self::EsiV2512 | Self::EsiV2606 => esi_capabilities(),
        }
    }
}

fn esi_capabilities() -> Capabilities {
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
        raw_backend: Some(RawBackend::Esi),
        ..Capabilities::unrestricted()
    }
}

pub const IMAGE: &str = "opencfd/openfoam-default@sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319";
/// The image of the v2606 dialect, pinned by the index digest (amd64 and arm64). Whether the image
/// entrypoint passes the executor's `bash -lc` command through is not yet proven on a real run;
/// the hosted probe checks it.
pub const IMAGE_V2606: &str =
    "opencfd/openfoam-run@sha256:4229997e74defb81548222d511b8e3b95b98305e5df41b8e88b031813fe47eeb";
/// What the v2606 image is, in one sentence, so a result never claims more than it knows.
pub const IMAGE_NOTE_V2606: &str = "The v2606 source is tagged final, but the packages the image was built from are labelled 2606.0~rc2-1 (rc2 packages, tagged final).";
pub const IMAGE_ENV: &str = "bash -lc";

/// A difference between what v2512 and v2606 write, named by the file it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeltaEntry {
    pub file: &'static str,
    pub reason: &'static str,
}

/// The files v2606 writes differently from v2512. Empty today: the research found no dictionary
/// difference for the four case types. The equality test reads this table, so a real difference
/// has a named place to go.
pub const V2606_DELTA: [DeltaEntry; 0] = [];

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
    fn the_dialect_keeps_every_other_limit_permissive() {
        let own = Dialect::EsiV2512.capabilities();
        let open = Capabilities::unrestricted();
        assert_eq!(own.turbulence_models, open.turbulence_models);
        assert!(own.transient && own.potential_init && own.buoyancy);
        assert!(own.accepts_raw && own.parabolic_inlet && !own.uniform_square_cells);
    }

    #[test]
    fn the_dialect_runs_overrides_tagged_for_its_backend() {
        assert_eq!(
            Dialect::EsiV2512.capabilities().raw_backend,
            Some(RawBackend::Esi)
        );
    }
}
