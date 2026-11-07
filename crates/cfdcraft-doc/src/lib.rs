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
//! The artifact index also holds the history of runs. A run is added together with the stored
//! results it names ([`Document::add_run`]), and a file whose history disagrees with its index is
//! refused on read and on write ([`check_runs`]).
//!
//! A caller that reads a file, does long work and writes it back reads it with [`open_stamped`] and
//! writes it with [`save_if_unchanged`], so a save made by someone else in between is refused and not
//! overwritten.
//!
//! The index can also hold variants of the study, parameter sweeps and the hashes of results that
//! were removed on purpose. All three are optional and left out of a file that has none. One check
//! ties them to each other, to the run history and to the list of stored results, on read and on
//! write ([`check_scenario_records`]).
//!
//! Linked files are stored as paths relative to the folder of the study file, so they keep
//! working when the project folder is moved; see [`resolve`], [`verify`] and [`check_links`].
//!
//! Saving builds the whole file in memory and replaces the old one through the shared crash-safe
//! save, so a crash or a failure leaves the previous file as it was. [`save_new`] is the same for a
//! file that must not exist yet: it never replaces one, even when another caller creates the file
//! at the same moment. Nothing in this crate writes a file any other way.

mod container;
mod links;
mod model;
mod runlog;
mod save;
mod scenelog;

pub use container::{DocError, from_bytes, read_document, to_bytes, write_members};
pub use links::{
    LinkCheck, LinkError, check_links, check_links_within, default_link_id, relative_path, resolve,
    resolve_within, verify,
};
pub use model::{
    ARTIFACTS_LIMIT, ARTIFACTS_MEMBER, ArtifactIndex, BlobKind, BlobRef, Document, FORMAT_ID, Link,
    MANIFEST_LIMIT, MANIFEST_MEMBER, Manifest, Opened, PLACEHOLDER_THUMBNAIL, STUDY_LIMIT,
    STUDY_MEMBER, THUMBNAIL_LIMIT, THUMBNAIL_MEMBER, Writer,
};
pub use runlog::{RunLogError, check_runs};
pub use save::{
    Stamp, open, open_stamped, save, save_if_unchanged, save_new, save_with, store_dir,
    sweep_stale_staging,
};
pub use scenelog::{MAX_SWEEPS, check_scenario_records};
