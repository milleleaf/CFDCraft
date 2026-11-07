//! The data a study file holds. Plain records only: reading and writing live in the container.

use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::{MigrationNote, Study};
use cfdcraft_runs::RunRecord;
use cfdcraft_store::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The format name written into every manifest.
pub const FORMAT_ID: &str = "cfdcraft-study";

/// Member name of the manifest, the first entry of every file.
pub const MANIFEST_MEMBER: &str = "manifest.json";
/// Member name of the study.
pub const STUDY_MEMBER: &str = "study.json";
/// Member name of the artifact index.
pub const ARTIFACTS_MEMBER: &str = "artifacts.json";
/// Member name of the thumbnail picture.
pub const THUMBNAIL_MEMBER: &str = "thumbnail.png";

/// Largest manifest that is read, in bytes.
pub const MANIFEST_LIMIT: u64 = 64 * 1024;
/// Largest study that is read, in bytes.
pub const STUDY_LIMIT: u64 = 32 * 1024 * 1024;
/// Largest artifact index that is read, in bytes.
pub const ARTIFACTS_LIMIT: u64 = 32 * 1024 * 1024;
/// Largest thumbnail that is read, in bytes.
pub const THUMBNAIL_LIMIT: u64 = 8 * 1024 * 1024;

/// The picture stored until a real view of the study exists: a 64 by 64 grey image.
pub const PLACEHOLDER_THUMBNAIL: &[u8] = include_bytes!("../assets/placeholder-thumbnail.png");

/// Which program wrote the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Writer {
    /// Program name.
    pub name: String,
    /// Program version.
    pub version: String,
}

impl Writer {
    /// This program.
    pub fn current() -> Self {
        Self {
            name: "cfdcraft".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

/// The first member of a file: what it is, which schema it follows and who wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Always [`FORMAT_ID`].
    pub format: String,
    /// The one schema number that covers every member of the file.
    pub schema_version: u32,
    /// The program that wrote the file.
    pub writer: Writer,
}

impl Manifest {
    /// The manifest this build writes.
    pub fn current() -> Self {
        Self {
            format: FORMAT_ID.to_owned(),
            schema_version: CURRENT_SCHEMA,
            writer: Writer::current(),
        }
    }
}

/// What kind of result a stored blob is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BlobKind {
    /// Prepared geometry.
    Surface,
    /// A generated mesh.
    Mesh,
    /// A solver result.
    Result,
}

/// A result kept in the content-addressed store beside the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlobRef {
    /// BLAKE3 hash of the content.
    pub hash: ContentHash,
    /// Size of the content in bytes.
    pub size: u64,
    /// What the content is.
    pub kind: BlobKind,
}

/// A file the study refers to but does not contain: where it was, and what it held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// Name the study uses for this link: 1 to 40 lower-case letters, digits or hyphens, starting with a letter or digit.
    #[schemars(regex(pattern = r"^[a-z0-9][a-z0-9-]{0,39}$"))]
    pub id: String,
    /// Path of the file, relative to the folder of the study file.
    pub path: String,
    /// File name as recorded when the link was made.
    pub name: String,
    /// BLAKE3 hash of the file content as recorded when the link was made.
    pub hash: ContentHash,
}

/// Everything the file refers to outside itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIndex {
    /// Stored results.
    #[serde(default)]
    pub blobs: Vec<BlobRef>,
    /// Linked files.
    #[serde(default)]
    pub links: Vec<Link>,
    /// The runs recorded for this study, oldest first; the list only grows.
    #[serde(default)]
    pub runs: Vec<RunRecord>,
}

/// A study file held in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The study.
    pub study: Study,
    /// Stored results and linked files.
    pub artifacts: ArtifactIndex,
    /// The picture shown for the file, if any.
    pub thumbnail: Option<Vec<u8>>,
}

impl Document {
    /// A document for `study` with no results, no links and the placeholder thumbnail.
    pub fn new(study: Study) -> Self {
        Self {
            study,
            artifacts: ArtifactIndex::default(),
            thumbnail: Some(PLACEHOLDER_THUMBNAIL.to_vec()),
        }
    }
}

/// A file that was read.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    /// The document, at the current schema.
    pub document: Document,
    /// The schema number the file was written with.
    pub schema_version: u32,
    /// What changed when an older file was brought forward; empty for a current file.
    pub notes: Vec<MigrationNote>,
    /// How many entries of the file hold stored results (`blobs/...`, the mark of a pack). Opening
    /// ignores them, so saving the document back to the same file would drop them.
    pub packed_entries: usize,
}
