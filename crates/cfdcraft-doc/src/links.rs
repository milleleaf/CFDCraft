//! Linked files: files a study refers to but does not contain.
//!
//! A link is stored as a path relative to the folder of the study file, always written with
//! forward slashes, together with the file name and the content hash the file had when it was
//! linked. Moving the whole project folder therefore changes nothing in the study file.
//!
//! A link may point anywhere the user can read, including outside the project folder: a study can
//! use a geometry library shared between projects. The stored text is untrusted. It is turned into
//! a path only by the shared Craft document-path crate, which refuses web addresses, files on
//! other computers, device names and paths that climb above the root. A refused form is never
//! followed and nothing is read from it.
//!
//! Opening a document never reads a linked file. [`resolve`] and [`check_links`] only look whether
//! the file exists. A file is read, and only to compare its hash, when [`verify`] is called; the
//! content of a linked file is never displayed.
//!
//! The relative path is computed from the paths as given, without asking the file system: symbolic
//! links are not followed, so a link made through a symbolic link keeps that spelling.

use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};

use cfdcraft_store::ContentHash;
use thiserror::Error;

use crate::model::{ArtifactIndex, Document, Link};

/// Longest link id, in characters.
const ID_MAX: usize = 40;

/// Why a link could not be made, followed or checked. Every message names the linked file.
#[derive(Debug, Error)]
pub enum LinkError {
    /// The linked file is not where the link says it should be.
    #[error("the linked file {name} is missing; it was expected at {}", .expected.display())]
    Missing {
        /// The file name recorded in the link.
        name: String,
        /// The full path where the file was looked for.
        expected: PathBuf,
    },
    /// The file is there, but its content is not what it was when it was linked.
    #[error("the linked file {name} has changed since it was linked")]
    Changed {
        /// The file name recorded in the link.
        name: String,
    },
    /// The stored link is not a path to a local file, so it is not followed.
    #[error("the link to {name} cannot be followed: {reason}")]
    Refused {
        /// The file name recorded in the link.
        name: String,
        /// Why the stored path was refused.
        reason: String,
    },
    /// No relative path written as text leads from the study file to the file.
    #[error(
        "the file {name} cannot be linked: no relative path leads to it from the study file (it may be on another drive)"
    )]
    NoRelativePath {
        /// The file name.
        name: String,
    },
    /// The link id is empty, too long or uses characters other than lower-case letters, digits and hyphens.
    #[error(
        "{0:?} is not a valid link id: use up to 40 lower-case letters, digits or hyphens, starting with a letter or digit"
    )]
    BadId(String),
    /// Another link already has this id.
    #[error("a link named {0} already exists")]
    DuplicateId(String),
    /// The path to link is not an existing file.
    #[error("{} is not a file that exists", .path.display())]
    NotAFile {
        /// The path that was given.
        path: PathBuf,
    },
    /// Reading the file failed.
    #[error("the linked file {name} could not be read: {source}")]
    Io {
        /// The file name recorded in the link.
        name: String,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
}

/// What looking at one link found.
#[derive(Debug)]
pub struct LinkCheck {
    /// The link id.
    pub id: String,
    /// The file name recorded in the link.
    pub name: String,
    /// The path of the file, or why it could not be reached.
    pub result: Result<PathBuf, LinkError>,
}

/// The id a link gets when none is given: the whole file name, lower-case, every character outside
/// a to z and 0 to 9 turned into a hyphen, leading hyphens dropped, cut to the longest id allowed.
/// The result is empty when the name has nothing to build an id from; it is then not a valid id.
pub fn default_link_id(target: &Path) -> String {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let hyphenated: String = name
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    hyphenated
        .trim_start_matches('-')
        .chars()
        .take(ID_MAX)
        .collect()
}

fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let Some((first, rest)) = bytes.split_first() else {
        return false;
    };
    bytes.len() <= ID_MAX
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && rest
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// The components of `path` with `.` removed and `..` folded in; `None` when a `..` would climb
/// above the root.
fn fold(path: &Path) -> Option<Vec<Component<'_>>> {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                _ => return None,
            },
            other => out.push(other),
        }
    }
    Some(out)
}

/// The path that leads from the folder `from_dir` to `to`, written with forward slashes: `..` for
/// every folder to climb out of, then the way down; `.` when both are the same.
///
/// Both paths must be absolute, otherwise the answer is `None`. It is also `None` when no such
/// path exists (different drives or shares on Windows), when a `..` climbs above the root, or when
/// a name is not valid text. The comparison is lexical: the file system is not asked and symbolic
/// links are not followed.
pub fn relative_path(from_dir: &Path, to: &Path) -> Option<String> {
    if !from_dir.is_absolute() || !to.is_absolute() {
        return None;
    }
    let from = fold(from_dir)?;
    let to = fold(to)?;
    let roots = from
        .iter()
        .take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
        .count();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common < roots {
        return None;
    }
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    for component in &to[common..] {
        match component {
            Component::Normal(name) => parts.push(name.to_str()?),
            _ => return None,
        }
    }
    Some(if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    })
}

/// The BLAKE3 hash of the file at `path`, read in a stream so a large file takes constant memory.
fn hash_file(path: &Path) -> io::Result<ContentHash> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    io::copy(&mut file, &mut hasher)?;
    ContentHash::parse(hasher.finalize().to_hex().as_str())
        .map_err(|e| io::Error::other(e.to_string()))
}

fn absolute(path: &Path, name: &str) -> Result<PathBuf, LinkError> {
    std::path::absolute(path).map_err(|source| LinkError::Io {
        name: name.to_owned(),
        source,
    })
}

fn refused(name: &str, reason: craft_docpath::Refused) -> LinkError {
    LinkError::Refused {
        name: name.to_owned(),
        reason: reason.to_string(),
    }
}

/// Why the links of a document that was read are not usable: an id that `add_link` would refuse,
/// or one that two links share. A file from disk is untrusted, and later code finds links by id.
pub(crate) fn check_ids(index: &ArtifactIndex) -> Result<(), String> {
    let shown = |id: &str| format!("{:?}", id.chars().take(ID_MAX).collect::<String>());
    let mut seen = std::collections::HashSet::new();
    for link in &index.links {
        if !valid_id(&link.id) {
            return Err(format!(
                "the link id {} is not a valid link id",
                shown(&link.id)
            ));
        }
        if !seen.insert(link.id.as_str()) {
            return Err(format!(
                "the link id {} is used by two links",
                shown(&link.id)
            ));
        }
    }
    Ok(())
}

impl Link {
    /// Make a link named `id` to the existing file `target`, for the study file at `doc_path`.
    ///
    /// Neither path has to be absolute. The file is read once, to record its hash.
    pub fn to_file(doc_path: &Path, target: &Path, id: &str) -> Result<Self, LinkError> {
        if !valid_id(id) {
            return Err(LinkError::BadId(id.to_owned()));
        }
        let shown = target.to_string_lossy().into_owned();
        let target = absolute(target, &shown)?;
        if !target.is_file() {
            return Err(LinkError::NotAFile { path: target });
        }
        let doc = absolute(doc_path, &shown)?;
        let name = target.file_name().and_then(|n| n.to_str());
        let (Some(name), Some(dir)) = (name, doc.parent()) else {
            let lossy = target
                .file_name()
                .map_or(shown, |n| n.to_string_lossy().into_owned());
            return Err(LinkError::NoRelativePath { name: lossy });
        };
        let no_path = || LinkError::NoRelativePath {
            name: name.to_owned(),
        };
        let path = relative_path(dir, &target).ok_or_else(no_path)?;
        // A backslash inside a name would be read back as a separator.
        if path.contains('\\') {
            return Err(no_path());
        }
        craft_docpath::resolve_absolute(&path, Some(&doc)).map_err(|r| refused(name, r))?;
        let hash = hash_file(&target).map_err(|source| LinkError::Io {
            name: name.to_owned(),
            source,
        })?;
        Ok(Self {
            id: id.to_owned(),
            path,
            name: name.to_owned(),
            hash,
        })
    }
}

impl Document {
    /// Add `link`. A link whose id is not valid or is already used is refused and the document
    /// stays as it was.
    pub fn add_link(&mut self, link: Link) -> Result<(), LinkError> {
        if !valid_id(&link.id) {
            return Err(LinkError::BadId(link.id));
        }
        if self.artifacts.links.iter().any(|l| l.id == link.id) {
            return Err(LinkError::DuplicateId(link.id));
        }
        self.artifacts.links.push(link);
        Ok(())
    }
}

/// The absolute path of the file `link` refers to, for the study file at `doc_path`.
///
/// Fails with [`LinkError::Refused`] when the stored text is not a path to a local file, and with
/// [`LinkError::Missing`] when there is no file at the resolved path. The file is not read.
pub fn resolve(doc_path: &Path, link: &Link) -> Result<PathBuf, LinkError> {
    resolve_in(doc_path, link, None)
}

/// [`resolve`] for a study file whose author is not trusted to name only their own files, as on a
/// server: a link that leads outside the folder `root`, by `..` or through a symbolic link, is
/// refused ([`LinkError::Refused`]) before the file system is asked about it, and the refusal does
/// not say where the link led.
pub fn resolve_within(doc_path: &Path, link: &Link, root: &Path) -> Result<PathBuf, LinkError> {
    resolve_in(doc_path, link, Some(root))
}

fn resolve_in(doc_path: &Path, link: &Link, root: Option<&Path>) -> Result<PathBuf, LinkError> {
    let doc = absolute(doc_path, &link.name)?;
    let expected = craft_docpath::resolve_absolute(&link.path, Some(&doc))
        .map_err(|r| refused(&link.name, r))?;
    if root.is_some_and(|root| !inside(&expected, root)) {
        return Err(LinkError::Refused {
            name: link.name.clone(),
            reason: "the path leads outside the folder this server works in".to_owned(),
        });
    }
    if expected.is_file() {
        Ok(expected)
    } else {
        Err(LinkError::Missing {
            name: link.name.clone(),
            expected,
        })
    }
}

/// Whether `path` is under `root`, as written and again after symbolic links are followed.
fn inside(path: &Path, root: &Path) -> bool {
    if !path.starts_with(root) {
        return false;
    }
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(real), Ok(real_root)) => real.starts_with(real_root),
        // Nothing there to follow, so nothing there to escape through.
        _ => true,
    }
}

/// Resolve `link` and compare the content of the file with the hash recorded when it was linked.
///
/// Fails with [`LinkError::Changed`] when they differ, and with the errors of [`resolve`].
pub fn verify(doc_path: &Path, link: &Link) -> Result<(), LinkError> {
    let path = resolve(doc_path, link)?;
    let actual = hash_file(&path).map_err(|source| LinkError::Io {
        name: link.name.clone(),
        source,
    })?;
    if actual == link.hash {
        Ok(())
    } else {
        Err(LinkError::Changed {
            name: link.name.clone(),
        })
    }
}

/// Resolve every link of `index`, in order. Files are not read, so this stays cheap; call
/// [`verify`] to compare content.
pub fn check_links(doc_path: &Path, index: &ArtifactIndex) -> Vec<LinkCheck> {
    check_in(doc_path, index, None)
}

/// [`check_links`] with every link confined to the folder `root`, as [`resolve_within`] does.
pub fn check_links_within(doc_path: &Path, index: &ArtifactIndex, root: &Path) -> Vec<LinkCheck> {
    check_in(doc_path, index, Some(root))
}

fn check_in(doc_path: &Path, index: &ArtifactIndex, root: Option<&Path>) -> Vec<LinkCheck> {
    index
        .links
        .iter()
        .map(|link| LinkCheck {
            id: link.id.clone(),
            name: link.name.clone(),
            result: resolve_in(doc_path, link, root),
        })
        .collect()
}
