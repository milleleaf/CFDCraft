//! Reading and writing the ZIP container.
//!
//! Writing is deterministic: the members come in a fixed order, every entry carries the same
//! fixed time and permissions, and the same document always gives the same bytes.
//!
//! Reading treats the file as untrusted. Only the four known member names are ever looked up;
//! every other entry is ignored, and no entry name is ever turned into a path. Each member is
//! read through a reader capped at its limit plus one byte, so an entry whose header understates
//! its size cannot exhaust memory.

use std::io::{self, Cursor, Read, Seek, Write};

use cfdcraft_migrate::{MigrateError, Parts, check_readable};
use cfdcraft_model::Study;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use zip::{
    CompressionMethod, DateTime, ZipArchive, ZipWriter, result::ZipError, write::SimpleFileOptions,
};

use crate::model::{
    ARTIFACTS_LIMIT, ARTIFACTS_MEMBER, ArtifactIndex, Document, FORMAT_ID, MANIFEST_LIMIT,
    MANIFEST_MEMBER, Manifest, Opened, STUDY_LIMIT, STUDY_MEMBER, THUMBNAIL_LIMIT,
    THUMBNAIL_MEMBER,
};

/// Why a study file could not be written or read.
#[derive(Debug, Error)]
pub enum DocError {
    /// The file is not a CFDCraft study file at all.
    #[error("this is not a CFDCraft study file: {0}")]
    NotAStudyFile(String),
    /// A required member is absent.
    #[error("the study file has no {0} entry")]
    MissingMember(String),
    /// The ZIP lists one entry name more than once, so it is unclear which one counts.
    #[error("the study file lists the same entry name twice")]
    DuplicateMember,
    /// A member is bigger than the largest size that is read.
    #[error("{name} in the study file is larger than the {limit} byte limit")]
    TooLarge {
        /// The member.
        name: String,
        /// The limit in bytes.
        limit: u64,
    },
    /// A member is not the JSON it should be.
    #[error("{member} in the study file is not valid: {reason}")]
    BadJson {
        /// The member.
        member: String,
        /// What is wrong with it.
        reason: String,
    },
    /// The file was written by a newer version.
    #[error(
        "this study file uses schema {found}, but this version only understands up to schema {supported}; open it with a newer version"
    )]
    NewerSchema {
        /// The schema number found.
        found: u32,
        /// The newest schema number this build reads.
        supported: u32,
    },
    /// Bringing an older file forward failed.
    #[error("bringing the study from schema {from} to schema {to} failed: {reason}")]
    Migrate {
        /// The schema number the step starts from.
        from: u32,
        /// The schema number the step reaches.
        to: u32,
        /// What went wrong.
        reason: String,
    },
    /// The disk refused a read or a write.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The ZIP layer reported a problem.
    #[error("the study file could not be read or written: {0}")]
    Zip(String),
}

fn zip_error(error: ZipError) -> DocError {
    match error {
        ZipError::Io(error) => DocError::Io(error),
        other => DocError::Zip(other.to_string()),
    }
}

fn pretty<T: Serialize>(member: &str, value: &T) -> Result<Vec<u8>, DocError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| DocError::BadJson {
        member: member.to_owned(),
        reason: e.to_string(),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn put<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    name: &str,
    options: SimpleFileOptions,
    bytes: &[u8],
) -> Result<(), DocError> {
    zip.start_file(name, options).map_err(zip_error)?;
    zip.write_all(bytes)?;
    Ok(())
}

/// Write the members of `doc` to `zip`: manifest first, so a reader can judge the file before
/// reading anything else, then the study, the artifact index and the thumbnail when there is one.
///
/// JSON is ordinary pretty text with a trailing newline. Every entry has the same fixed time and
/// permissions. The caller may add more entries afterwards, and finishes the archive.
pub fn write_members<W: Write + Seek>(
    doc: &Document,
    zip: &mut ZipWriter<W>,
) -> Result<(), DocError> {
    let fixed = SimpleFileOptions::default()
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    let text = fixed
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(1));
    let picture = fixed.compression_method(CompressionMethod::Stored);
    put(
        zip,
        MANIFEST_MEMBER,
        text,
        &pretty(MANIFEST_MEMBER, &Manifest::current())?,
    )?;
    put(zip, STUDY_MEMBER, text, &pretty(STUDY_MEMBER, &doc.study)?)?;
    put(
        zip,
        ARTIFACTS_MEMBER,
        text,
        &pretty(ARTIFACTS_MEMBER, &doc.artifacts)?,
    )?;
    if let Some(thumbnail) = &doc.thumbnail {
        put(zip, THUMBNAIL_MEMBER, picture, thumbnail)?;
    }
    Ok(())
}

/// The whole file for `doc`, as bytes.
pub fn to_bytes(doc: &Document) -> Result<Vec<u8>, DocError> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    write_members(doc, &mut zip)?;
    Ok(zip.finish().map_err(zip_error)?.into_inner())
}

/// Read the member `name`, at most `limit` bytes of it. `None` when the file has no such member.
fn read_member<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, DocError> {
    let file = match zip.by_name(name) {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(other) => return Err(zip_error(other)),
    };
    let too_large = || DocError::TooLarge {
        name: name.to_owned(),
        limit,
    };
    // The declared size comes from the file and may lie; it only saves work. The capped read
    // below is the real guard.
    if file.size() > limit {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| match e.kind() {
            io::ErrorKind::InvalidData => DocError::Zip(format!("{name}: {e}")),
            _ => DocError::Io(e),
        })?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(Some(bytes))
}

fn required(bytes: Option<Vec<u8>>, name: &str) -> Result<Vec<u8>, DocError> {
    bytes.ok_or_else(|| DocError::MissingMember(name.to_owned()))
}

fn bad_json(member: &str, error: serde_json::Error) -> DocError {
    DocError::BadJson {
        member: member.to_owned(),
        reason: error.to_string(),
    }
}

fn parse<T: DeserializeOwned>(member: &str, bytes: &[u8]) -> Result<T, DocError> {
    serde_json::from_slice(bytes).map_err(|e| bad_json(member, e))
}

fn from_migrate(error: MigrateError) -> DocError {
    match error {
        MigrateError::NewerSchema { found, supported } => {
            DocError::NewerSchema { found, supported }
        }
        MigrateError::Step { from, to, reason } => DocError::Migrate { from, to, reason },
    }
}

/// The schema number of a manifest read as loose JSON, after checking it is a study manifest.
fn manifest_version(manifest: &Value) -> Result<u32, DocError> {
    let refuse = |reason: &str| DocError::NotAStudyFile(reason.to_owned());
    if manifest.get("format").and_then(Value::as_str) != Some(FORMAT_ID) {
        return Err(refuse(
            "the manifest does not name the CFDCraft study format",
        ));
    }
    manifest
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| {
            refuse("the manifest's schema_version is not a whole number from 0 to 4294967295")
        })
}

/// Read a study file from `reader`.
///
/// The order is the contract. The manifest alone is read first and decides whether the file can be
/// read at all: a file from a newer version is refused before any other member is touched, so a
/// damaged or unfamiliar study member cannot change that answer. Only then are the study and the
/// artifact index read, brought forward to the current schema one step at a time, and checked
/// against the current types.
pub fn read_document<R: Read + Seek>(reader: R) -> Result<Opened, DocError> {
    let mut zip = ZipArchive::new(reader).map_err(|e| match e {
        ZipError::Io(e) if !matches!(e.kind(), io::ErrorKind::UnexpectedEof) => DocError::Io(e),
        other => DocError::NotAStudyFile(other.to_string()),
    })?;
    // The ZIP reader keeps only the last of two equal names, which is how a second copy hides
    // the first, so a file with a repeated name is refused outright.
    if zip.has_duplicate_names() {
        return Err(DocError::DuplicateMember);
    }

    let manifest_bytes = required(
        read_member(&mut zip, MANIFEST_MEMBER, MANIFEST_LIMIT)?,
        MANIFEST_MEMBER,
    )?;
    let loose_manifest: Value = parse(MANIFEST_MEMBER, &manifest_bytes)?;
    let found = manifest_version(&loose_manifest)?;
    check_readable(found).map_err(from_migrate)?;
    // Only a file that can be read is held to the exact manifest shape.
    serde_json::from_value::<Manifest>(loose_manifest).map_err(|e| bad_json(MANIFEST_MEMBER, e))?;

    let study_bytes = required(
        read_member(&mut zip, STUDY_MEMBER, STUDY_LIMIT)?,
        STUDY_MEMBER,
    )?;
    let study: Value = parse(STUDY_MEMBER, &study_bytes)?;
    let artifacts: Option<Value> = match read_member(&mut zip, ARTIFACTS_MEMBER, ARTIFACTS_LIMIT)? {
        Some(bytes) => Some(parse(ARTIFACTS_MEMBER, &bytes)?),
        None => None,
    };

    let (parts, notes) =
        cfdcraft_migrate::migrate(Parts { study, artifacts }, found).map_err(from_migrate)?;
    let study: Study =
        serde_json::from_value(parts.study).map_err(|e| bad_json(STUDY_MEMBER, e))?;
    let artifacts = match parts.artifacts {
        Some(value) => serde_json::from_value::<ArtifactIndex>(value)
            .map_err(|e| bad_json(ARTIFACTS_MEMBER, e))?,
        None => ArtifactIndex::default(),
    };
    let thumbnail = read_member(&mut zip, THUMBNAIL_MEMBER, THUMBNAIL_LIMIT)?;

    Ok(Opened {
        document: Document {
            study,
            artifacts,
            thumbnail,
        },
        schema_version: found,
        notes,
    })
}

/// Read a study file held in memory.
pub fn from_bytes(bytes: &[u8]) -> Result<Opened, DocError> {
    read_document(Cursor::new(bytes))
}
