//! The shape of `import.json`, the record written last into an imported case folder.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The value of the `format` member.
pub const FORMAT: &str = "cfdcraft-case-import-1";

/// The guard policy of an import that allow-listed nothing: the scan was used as it is. An import
/// that used a list records `allowlist:<image id>:<number of entries>`.
pub const GUARD_POLICY: &str = "strict";

/// One stored file of `original/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileEntry {
    /// The path below `original/`, `/` separated.
    pub rel_path: String,
    pub bytes: u64,
    /// The BLAKE3 hash of the file, as lower-case hex.
    pub blake3: String,
}

/// One entry the guard refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImportRefusal {
    /// The guard rule code, for example `CFD-GUARD-001`.
    pub code: String,
    pub file: String,
    pub line: u32,
    pub entry: String,
    pub message: String,
}

/// The time settings of `system/controlDict`, each as written; a missing entry is `null`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimeSettings {
    pub start_from: Option<String>,
    pub start_time: Option<String>,
    pub stop_at: Option<String>,
    pub end_time: Option<String>,
    pub delta_t: Option<String>,
    pub write_control: Option<String>,
    pub write_interval: Option<String>,
}

/// What the case's dictionaries say about it. Lists are sorted and without repeats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Inventory {
    /// `foundation-14`, `foundation-legacy`, `esi` or `unknown`.
    pub dialect: String,
    /// The `solver` entry of the control dictionary, else its `application` entry.
    pub application_or_solver: Option<String>,
    /// The simulation type of the turbulence dictionary (`laminar`, `RAS`, `LES`).
    pub turbulence_model: Option<String>,
    /// The model named under `RAS` or `LES`.
    pub turbulence_detail: Option<String>,
    /// `snappyHexMesh`, `blockMesh`, `polyMesh` or `unknown`.
    pub mesh_kind: String,
    /// The `type` words of the boundary conditions found in the field files.
    pub boundary_condition_kinds: Vec<String>,
    pub time_settings: TimeSettings,
    /// The names of the function objects of the control dictionary.
    pub function_objects: Vec<String>,
    /// Dictionaries that could not be parsed or were too large to parse.
    pub unparsed: Vec<String>,
}

/// The whole record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Record {
    pub format: String,
    /// The last component of the source folder's path; the full path is never stored.
    pub source_name: String,
    /// Every file of `original/`, sorted by path.
    pub files: Vec<FileEntry>,
    /// Empty for an import that happened; a refused case writes no record.
    pub refusals: Vec<ImportRefusal>,
    /// How many files were left out, by kind: `time`, `processor`, `post_processing`, `log`.
    pub skipped: BTreeMap<String, u64>,
    /// The scripts of `original/`, kept inert and left out of `work/`.
    pub scripts: Vec<String>,
    #[serde(flatten)]
    pub inventory: Inventory,
    pub guard_policy: String,
}
