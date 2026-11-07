//! Importing an existing OpenFOAM case.
//!
//! An import is a raw copy with an inventory, not a conversion: the case folder is scanned by the
//! same guard that scans every case CFDCraft writes, and a case holding any refused entry is
//! refused as a whole and nothing is written. A clean case becomes a `<name>.cfdcase` folder
//! holding `original/` (every file read-only), `work/` (a copy to edit, without the case's
//! scripts) and `import.json` (an inventory and a hash of every file), written last.
//!
//! Nothing here runs a process, follows a link or opens a file the walk did not list. The case's
//! own scripts are copied as inert files and never run. Result folders (time folders other than
//! `0`, `processor*`, `postProcessing` and `log.*` files) are counted and left out; the guard does
//! not read them either (a field file there may be large or compressed), and still refuses links,
//! special files, libraries and unsafe names inside them.
//!
//! The scan is strict unless a reviewed list covers the case's dialect (see `allowlist_for`): then
//! exactly the listed `#includeEtc` paths and `#includeFunc` names are accepted and every other
//! directive stays refused. The lists ship empty.

mod allowlist;
mod builder;
mod copy;
mod inventory;
mod record;
mod verify;

pub use allowlist::{
    TABLE, TableError, TableRow, allowlist_for, image_id_for_dialect, parse_table,
};
pub use builder::{Candidate, Excluded, build_candidate};
pub use cfdcraft_backend_esi::guard::{Allowlist, AllowlistError};
pub use record::{FORMAT, FileEntry, GUARD_POLICY, ImportRefusal, Inventory, Record, TimeSettings};
pub use verify::{Finding, FindingKind, verify_import};

use cfdcraft_backend_esi::guard;
use std::fs;
use std::io;
use std::path::Path;
use thiserror::Error;

/// The caps of an import. The defaults are the ones the scan itself uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOptions {
    /// Most files copied.
    pub max_files: usize,
    /// Largest single file, in bytes.
    pub max_file_bytes: u64,
    /// Largest total of all copied files, in bytes.
    pub max_total_bytes: u64,
    /// Deepest folder nesting below the source.
    pub max_depth: usize,
    /// A list to use instead of the table's list for the detected dialect. `None` takes the
    /// table's list; a dialect without a list is scanned strictly.
    pub allowlist: Option<Allowlist>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_file_bytes: 16 * 1024 * 1024,
            max_total_bytes: 1024 * 1024 * 1024,
            max_depth: 16,
            allowlist: None,
        }
    }
}

/// Why an import could not start or finish. `code()` is the catalogue code, when there is one.
#[derive(Debug, Error)]
pub enum ImportError {
    /// `CFD-IMPORT-001`.
    #[error("the source folder cannot be read: {reason}")]
    Source { reason: String },
    /// `CFD-IMPORT-003`.
    #[error("the target folder already exists")]
    TargetExists,
    /// `CFD-IMPORT-004`.
    #[error(
        "the folder is not an OpenFOAM case: it has no system/controlDict and no dictionary with a FoamFile header"
    )]
    NotACase,
    /// `CFD-IMPORT-006`. `what` is `files`, `file_bytes`, `total_bytes` or `depth`.
    #[error("the case reached the cap on {what} ({limit}) at {at}")]
    Cap {
        what: &'static str,
        limit: u64,
        at: String,
    },
    /// A write failed; the target holds what was written so far and no record.
    #[error("writing the import failed: {reason}")]
    Write { reason: String },
}

impl ImportError {
    pub fn code(&self) -> Option<&'static str> {
        match self {
            ImportError::Source { .. } => Some("CFD-IMPORT-001"),
            ImportError::TargetExists => Some("CFD-IMPORT-003"),
            ImportError::NotACase => Some("CFD-IMPORT-004"),
            ImportError::Cap { .. } => Some("CFD-IMPORT-006"),
            ImportError::Write { .. } => None,
        }
    }
}

pub(crate) fn source_err(e: io::Error) -> ImportError {
    ImportError::Source {
        reason: e.to_string(),
    }
}

/// What an import did.
// The record is the whole result and the refused outcome carries the inventory by value, so the
// two differ by less than a boxed pointer is worth.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// The case was stored and `import.json` was written.
    Imported(Record),
    /// The case holds refused entries; nothing was written.
    Refused {
        /// Every refusal of the guard, unchanged and sorted by file and line.
        refusals: Vec<ImportRefusal>,
        /// What could be read of the case.
        inventory: Inventory,
    },
}

/// The list for a detected dialect and the policy text the record carries: `strict` when the list
/// is empty, otherwise `allowlist:<image id>:<entries>`.
fn choose_allowlist(dialect: &str, options: &ImportOptions) -> (Allowlist, String) {
    let id = allowlist::image_id_for_dialect(dialect);
    let list = match (&options.allowlist, id) {
        (Some(list), _) => list.clone(),
        (None, Some(id)) => allowlist::allowlist_for(id).unwrap_or_default(),
        (None, None) => Allowlist::empty(),
    };
    let policy = if list.is_empty() {
        GUARD_POLICY.to_string()
    } else {
        format!("allowlist:{}:{}", id.unwrap_or("custom"), list.len())
    };
    (list, policy)
}

/// Imports the case folder `source` into the new folder `target`.
pub fn import_case(
    source: &Path,
    target: &Path,
    options: &ImportOptions,
) -> Result<ImportOutcome, ImportError> {
    import_case_hooked(source, target, options, &mut |_| Ok(()))
}

/// [`import_case`] with a hook that is called with the path of every file just before it is
/// written, in write order; an error from the hook stops the import there. For tests of the
/// write order and of an import that fails half way.
#[doc(hidden)]
pub fn import_case_hooked(
    source: &Path,
    target: &Path,
    options: &ImportOptions,
    hook: &mut dyn FnMut(&Path) -> io::Result<()>,
) -> Result<ImportOutcome, ImportError> {
    if !fs::metadata(source).map_err(source_err)?.is_dir() {
        return Err(ImportError::Source {
            reason: "it is not a folder".to_string(),
        });
    }
    if fs::symlink_metadata(target).is_ok() {
        return Err(ImportError::TargetExists);
    }
    let mut found = inventory::walk(source, options)?;

    // The dialect decides which reviewed list applies, so the dictionaries are read first.
    let (cache, inventory) = inventory::load(&found)?;
    let (allow, policy) = choose_allowlist(&inventory.dialect, options);
    let scan =
        guard::scan_dir_leaving_out(source, &allow, &inventory::is_left_out).map_err(source_err)?;
    let mut refusals = std::mem::take(&mut found.refusals);
    refusals.extend(scan.refusals.iter().map(|r| ImportRefusal {
        code: r.rule.code().to_string(),
        file: r.file.clone(),
        line: u32::try_from(r.line).unwrap_or(u32::MAX),
        entry: r.entry.clone(),
        message: r.message.clone(),
    }));
    refusals.sort_by_key(|r| (r.file.clone(), r.line));

    // A folder that is not a case but holds refused entries (a link where the control dictionary
    // should be) is reported with its refusals; only a clean folder is called not a case.
    if refusals.is_empty() && !inventory::is_case(&found) {
        return Err(ImportError::NotACase);
    }

    if !refusals.is_empty() {
        return Ok(ImportOutcome::Refused {
            refusals,
            inventory,
        });
    }

    let copied = copy::copy_all(&found, &cache, target, options, hook)?;
    let record = Record {
        format: FORMAT.to_string(),
        source_name: source
            .file_name()
            .map_or_else(|| "case".to_string(), |n| n.to_string_lossy().into_owned()),
        files: copied.files,
        refusals: Vec::new(),
        skipped: found.skipped,
        scripts: copied.scripts,
        inventory,
        guard_policy: policy,
    };
    let mut text = serde_json::to_string_pretty(&record).map_err(|e| ImportError::Write {
        reason: e.to_string(),
    })?;
    text.push('\n');
    copy::put(&target.join("import.json"), text.as_bytes(), hook)?;
    Ok(ImportOutcome::Imported(record))
}
