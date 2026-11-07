use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use cfdcraft_store::{ContentHash, Store};
use zip::{
    CompressionMethod, DateTime, ZipArchive, ZipWriter, result::ZipError, write::SimpleFileOptions,
};

use crate::{BUNDLE_CAP, CaptureError, FILE_CAP};

/// The files of a mesh, relative to the results folder.
const MESH_FILES: [&str; 5] = [
    "constant/polyMesh/boundary",
    "constant/polyMesh/faces",
    "constant/polyMesh/neighbour",
    "constant/polyMesh/owner",
    "constant/polyMesh/points",
];

/// Named files held together; the ZIP form is a pure function of the names and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    entries: Vec<(String, Vec<u8>)>,
}

impl Bundle {
    /// Sorts the entries by name. A repeated name or a total over [`BUNDLE_CAP`] is refused.
    pub fn from_entries(mut entries: Vec<(String, Vec<u8>)>) -> Result<Self, CaptureError> {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(pair) = entries.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(CaptureError::Invalid(format!(
                "{} appears twice in the bundle",
                pair[0].0
            )));
        }
        let total: u64 = entries.iter().map(|(_, bytes)| bytes.len() as u64).sum();
        if total > BUNDLE_CAP {
            return Err(CaptureError::TooLarge("the bundle as a whole".to_owned()));
        }
        Ok(Self { entries })
    }

    /// Entry names in sorted order.
    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// The stored ZIP: sorted entries, no compression, fixed time and permissions.
    pub fn to_bytes(&self) -> Result<Vec<u8>, CaptureError> {
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .last_modified_time(DateTime::default())
            .unix_permissions(0o644);
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in &self.entries {
            zip.start_file(name.as_str(), options).map_err(zip_error)?;
            zip.write_all(bytes)?;
        }
        Ok(zip.finish().map_err(zip_error)?.into_inner())
    }

    /// The hash of [`Bundle::to_bytes`].
    pub fn hash(&self) -> Result<ContentHash, CaptureError> {
        Ok(ContentHash::of(&self.to_bytes()?))
    }
}

fn zip_error(error: ZipError) -> CaptureError {
    match error {
        ZipError::Io(error) => CaptureError::Io(error),
        other => CaptureError::Zip(other.to_string()),
    }
}

/// The bytes of one entry of a bundle, read through a reader capped at [`FILE_CAP`].
pub fn read_entry(bytes: &[u8], name: &str) -> Result<Vec<u8>, CaptureError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    let file = match zip.by_name(name) {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Err(CaptureError::Missing(name.to_owned())),
        Err(other) => return Err(zip_error(other)),
    };
    let too_large = || CaptureError::TooLarge(name.to_owned());
    if file.size() > FILE_CAP {
        return Err(too_large());
    }
    let mut out = Vec::new();
    file.take(FILE_CAP + 1)
        .read_to_end(&mut out)
        .map_err(|e| match e.kind() {
            io::ErrorKind::InvalidData => CaptureError::Zip(format!("{name}: {e}")),
            _ => CaptureError::Io(e),
        })?;
    if out.len() as u64 > FILE_CAP {
        return Err(too_large());
    }
    Ok(out)
}

/// Every entry name of a bundle, sorted.
pub fn entry_names(bytes: &[u8]) -> Result<Vec<String>, CaptureError> {
    let zip = ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    let mut names = zip
        .file_names()
        .map(|name| name.map(Cow::into_owned).map_err(zip_error))
        .collect::<Result<Vec<String>, CaptureError>>()?;
    names.sort();
    Ok(names)
}

fn not_found_or_io(error: io::Error, shown: &str) -> CaptureError {
    if error.kind() == io::ErrorKind::NotFound {
        CaptureError::Missing(shown.to_owned())
    } else {
        CaptureError::Io(error)
    }
}

/// Walk `dirs` below `results`, requiring each to be a real folder (a link is refused).
/// With no components this is `results` itself, which the caller chose.
pub(crate) fn walk_dirs(results: &Path, dirs: &[&str]) -> Result<PathBuf, CaptureError> {
    let mut path = results.to_path_buf();
    let mut shown = String::new();
    for part in dirs {
        if !shown.is_empty() {
            shown.push('/');
        }
        shown.push_str(part);
        path.push(part);
        let meta = fs::symlink_metadata(&path).map_err(|e| not_found_or_io(e, &shown))?;
        if !meta.file_type().is_dir() {
            return Err(CaptureError::NotAFile(shown));
        }
    }
    Ok(path)
}

/// Read the regular file `rel` (names joined by `/`) below `results`. Links anywhere on the way,
/// a folder in place of the file, and a file longer than `cap` are refused.
pub(crate) fn read_regular(results: &Path, rel: &str, cap: u64) -> Result<Vec<u8>, CaptureError> {
    let parts: Vec<&str> = rel.split('/').collect();
    let Some((file, dirs)) = parts.split_last() else {
        return Err(CaptureError::Invalid("an empty file name".to_owned()));
    };
    let path = walk_dirs(results, dirs)?.join(file);
    let meta = fs::symlink_metadata(&path).map_err(|e| not_found_or_io(e, rel))?;
    if !meta.file_type().is_file() {
        return Err(CaptureError::NotAFile(rel.to_owned()));
    }
    let too_large = || CaptureError::TooLarge(rel.to_owned());
    if meta.len() > cap {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| not_found_or_io(e, rel))?
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Read every named file and gather them into a bundle, stopping when the total gets too big.
pub(crate) fn read_all(
    results: &Path,
    names: Vec<String>,
    file_cap: u64,
) -> Result<Bundle, CaptureError> {
    let mut entries = Vec::with_capacity(names.len());
    let mut total = 0u64;
    for name in names {
        let bytes = read_regular(results, &name, file_cap)?;
        total = total.saturating_add(bytes.len() as u64);
        if total > BUNDLE_CAP {
            return Err(CaptureError::TooLarge("the bundle as a whole".to_owned()));
        }
        entries.push((name, bytes));
    }
    Bundle::from_entries(entries)
}

/// The five `polyMesh` files of a results folder.
pub fn mesh_bundle(results: &Path) -> Result<Bundle, CaptureError> {
    mesh_bundle_limited(results, FILE_CAP)
}

/// [`mesh_bundle`] with a smaller cap per file (for tests).
pub fn mesh_bundle_limited(results: &Path, file_cap: u64) -> Result<Bundle, CaptureError> {
    read_all(
        results,
        MESH_FILES.iter().map(|name| (*name).to_owned()).collect(),
        file_cap,
    )
}

/// Put the bundle in the store; returns its hash and its length in bytes.
pub fn store_bundle(store: &Store, bundle: &Bundle) -> Result<(ContentHash, u64), CaptureError> {
    let bytes = bundle.to_bytes()?;
    let hash = store.put_bytes(&bytes)?;
    Ok((hash, bytes.len() as u64))
}
