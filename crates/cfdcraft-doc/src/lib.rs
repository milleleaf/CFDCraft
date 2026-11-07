//! The CFDCraft study file.
//!
//! A study file is a ZIP archive with these members, always in this order:
//!
//! | Member | Content |
//! |---|---|
//! | `manifest.json` | the format name, the schema number and which program wrote the file |
//! | `study.json` | the study, as ordinary pretty JSON |
//! | `artifacts.json` | the index of stored results and linked files |
//! | `thumbnail.png` | a small picture of the study, stored without compression |
//!
//! One integer, the schema number in the manifest, covers the whole file. Older files are brought
//! forward on open and the caller is told what changed; a file from a newer version is refused
//! after reading the manifest alone.
//!
//! Unknown entries are ignored, never extracted and never used as paths, so a packed copy of a
//! study can carry extra entries. Each member is read through a size limit (manifest 64 KiB, study
//! and artifact index 32 MiB each, thumbnail 8 MiB), and a file that lists the same entry name
//! twice is refused.
//!
//! Linked files are stored as paths relative to the folder of the study file, so they keep
//! working when the project folder is moved; see [`resolve`], [`verify`] and [`check_links`].
//!
//! Saving builds the whole file in memory and replaces the old one through the shared crash-safe
//! save, so a crash or a failure leaves the previous file as it was. Nothing in this crate writes a
//! file any other way.

mod container;
mod links;
mod model;
mod save;

pub use container::{DocError, from_bytes, read_document, to_bytes, write_members};
pub use links::{LinkCheck, LinkError, check_links, relative_path, resolve, verify};
pub use model::{
    ARTIFACTS_LIMIT, ARTIFACTS_MEMBER, ArtifactIndex, BlobKind, BlobRef, Document, FORMAT_ID, Link,
    MANIFEST_LIMIT, MANIFEST_MEMBER, Manifest, Opened, PLACEHOLDER_THUMBNAIL, STUDY_LIMIT,
    STUDY_MEMBER, THUMBNAIL_LIMIT, THUMBNAIL_MEMBER, Writer,
};
pub use save::{open, save, save_with, store_dir, sweep_stale_staging};
