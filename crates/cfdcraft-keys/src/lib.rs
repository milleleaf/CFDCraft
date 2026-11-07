//! Stage keys: what made a mesh and what made a result, as data, and the key of each.
//!
//! A key is the BLAKE3 hash of a recipe written as RFC 8785 JSON. The recipe is built from a
//! study already resolved to SI units, so equal studies written in other units give equal keys.
//! The result recipe names its mesh by key and not by the mesh's parameters, which is how a
//! mesh change reaches the result.
//!
//! The canonical text is for hashing only; a recipe is stored as ordinary JSON.
//!
//! The crate is pure: it reads and writes no files.

use std::fmt;
use std::str::FromStr;

use cfdcraft_store::{ContentHash, StoreError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod diff;
pub mod recipe;

pub use diff::{
    Axis, Change, Residual, ToolPart, diff_mesh, diff_result, line_text, probe_text, raw_text,
    surface_text,
};
pub use recipe::{
    BoundaryRecipe, CellsRecipe, FluidRecipe, GeometryRecipe, LineRecipe, MeshRecipe,
    MonitorsRecipe, PatchRecipe, ProbeRecipe, ResultRecipe, SolverRecipe, StageKeys, StageRecipes,
    SurfaceRecipe,
};

/// Everything about the tools that can change an output without the study changing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolIdentity {
    /// The part of the image reference after `@`, for example `sha256:...`.
    pub image_digest: String,
    /// The solver dialect the case is written in, for example `esi-v2512`.
    pub dialect: String,
    /// The hash of the geometry sidecar's lock, or `null` when no sidecar was used. An absent
    /// lock is written as `null`, never skipped, so absent and present keys differ.
    pub sidecar_lock: Option<String>,
    /// The version of this program that wrote the case.
    pub writer_version: String,
}

impl ToolIdentity {
    pub fn new(
        image_digest: impl Into<String>,
        dialect: impl Into<String>,
        sidecar_lock: Option<String>,
        writer_version: impl Into<String>,
    ) -> Self {
        Self {
            image_digest: image_digest.into(),
            dialect: dialect.into(),
            sidecar_lock,
            writer_version: writer_version.into(),
        }
    }
}

/// The two kinds of keyed output. Name it as `Stage::Result`; a glob import would shadow
/// `Result`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Mesh,
    Result,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mesh => "mesh",
            Self::Result => "result",
        }
    }
}

/// The key of one stage output: 64 lower-case hexadecimal characters.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct StageKey(ContentHash);

impl StageKey {
    /// Accepts exactly what [`ContentHash::parse`] accepts.
    pub fn parse(text: &str) -> Result<Self, StoreError> {
        ContentHash::parse(text).map(Self)
    }

    pub fn as_hash(&self) -> ContentHash {
        self.0
    }

    pub(crate) fn of_text(text: &str) -> Self {
        Self(ContentHash::of(text.as_bytes()))
    }
}

impl fmt::Display for StageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for StageKey {
    type Err = StoreError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}
