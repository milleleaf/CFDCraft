//! Raw dictionary overrides: one solver dictionary entry the study does not model, written by an
//! expert and carried in the study so it is visible, diffable and part of the result's identity.
//!
//! A raw override is parsed no further than file, entry path and value. Whether one is valid is
//! decided by the study checks in [`crate::diagnostics`]; this module holds the shapes and the
//! one fact about them that other code needs: whether the file shapes the mesh.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Most raw overrides one study may carry.
pub const MAX_RAW_OVERRIDES: usize = 50;

/// The dictionary files that shape the mesh. Anything under [`MESH_DIRECTORY`] does too.
pub const MESH_FILES: [&str; 4] = [
    "system/blockMeshDict",
    "system/snappyHexMeshDict",
    "system/meshQualityDict",
    "system/surfaceFeaturesDict",
];

/// Files below this folder describe the mesh itself.
pub const MESH_DIRECTORY: &str = "constant/polyMesh/";

/// The backend a raw override is written for. The dictionary syntax differs between backends, so
/// an override only runs on the backend it names.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum RawBackend {
    Esi,
    Foundation,
}

impl RawBackend {
    /// The tag as written in a study.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Esi => "esi",
            Self::Foundation => "foundation",
        }
    }
}

impl fmt::Display for RawBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One dictionary entry to set in the written case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RawOverride {
    /// The backend this override is written for.
    pub backend: RawBackend,
    /// The case file, for example `system/fvSolution`.
    pub file: String,
    /// The entry inside the file: block names then the entry, joined by `/`, for example
    /// `relaxationFactors/equations/U`.
    pub entry: String,
    /// The text that goes after the entry name.
    pub value: String,
}

impl RawOverride {
    /// True when the file is one the mesh is built from, so a change to it needs a new mesh.
    pub fn affects_mesh(&self) -> bool {
        MESH_FILES.contains(&self.file.as_str()) || self.file.starts_with(MESH_DIRECTORY)
    }
}
