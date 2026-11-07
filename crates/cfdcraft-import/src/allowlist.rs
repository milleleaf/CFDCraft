//! The reviewed lists of `etc` includes and function names an imported case may use, one per
//! image. A case written for a tutorial tree uses `#includeEtc` and `#includeFunc`, which the
//! guard refuses; an entry here makes exactly that path or name acceptable for the image that
//! ships it. The lists ship empty: an entry is added after the probe output of the image has been
//! reviewed, and never by prefix.

use cfdcraft_backend_esi::dialect;
use cfdcraft_backend_esi::guard::{Allowlist, AllowlistError};
use thiserror::Error;

/// One image and its list.
#[derive(Debug, Clone, Copy)]
pub struct TableRow {
    /// The backend id.
    pub id: &'static str,
    /// The image digest the list was reviewed against, or `none` while the image is not built.
    pub image: &'static str,
    /// The list text: `etc <path>` and `func <name>` lines, `#` starts a comment.
    pub text: &'static str,
}

/// The lists, one row per OpenFOAM image.
pub const TABLE: [TableRow; 3] = [
    TableRow {
        id: "esi-v2512",
        image: dialect::IMAGE,
        text: include_str!("allowlists/esi-v2512.txt"),
    },
    TableRow {
        id: "esi-v2606",
        image: dialect::IMAGE_V2606,
        text: include_str!("allowlists/esi-v2606.txt"),
    },
    TableRow {
        id: "of14-20261008",
        image: "none",
        text: include_str!("allowlists/of14-20261008.txt"),
    },
];

/// A list text that cannot be read.
#[derive(Debug, Error)]
pub enum TableError {
    #[error("line {line} is neither an `etc` entry, a `func` entry nor a comment")]
    Line { line: usize },
    #[error("line {line}: {source}")]
    Entry { line: usize, source: AllowlistError },
}

/// Reads a list text. Every entry is validated by [`Allowlist::new`].
pub fn parse_table(text: &str) -> Result<Allowlist, TableError> {
    let (mut etc, mut functions) = (Vec::new(), Vec::new());
    for (n, raw) in text.lines().enumerate() {
        let (line, row) = (n + 1, raw.trim());
        if row.is_empty() || row.starts_with('#') {
            continue;
        }
        match row.split_once(' ') {
            Some(("etc", path)) => etc.push((line, path.trim())),
            Some(("func", name)) => functions.push((line, name.trim())),
            _ => return Err(TableError::Line { line }),
        }
    }
    // Each entry is checked on its own so the error names its line.
    for (line, entry) in etc.iter().chain(functions.iter()) {
        Allowlist::new([*entry], Vec::<&str>::new()).map_err(|source| TableError::Entry {
            line: *line,
            source,
        })?;
    }
    Ok(Allowlist::new(
        etc.iter().map(|(_, e)| *e),
        functions.iter().map(|(_, e)| *e),
    )
    // Every entry was checked above.
    .unwrap_or_default())
}

/// The list of one backend id, or `None` for an id without a list. A table that does not load is
/// treated as no list, so the scan stays strict.
pub fn allowlist_for(id: &str) -> Option<Allowlist> {
    TABLE
        .iter()
        .find(|row| row.id == id)
        .and_then(|row| parse_table(row.text).ok())
}

/// The backend id whose list covers a detected dialect, if any.
pub fn image_id_for_dialect(dialect: &str) -> Option<&'static str> {
    match dialect {
        "foundation-14" => Some("of14-20261008"),
        "esi" => Some("esi-v2512"),
        _ => None,
    }
}
