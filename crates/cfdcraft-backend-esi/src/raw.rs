//! Raw dictionary overrides applied to the files the writer has built.
//!
//! An override names a case file, an entry path and a value. It is applied to the text of the file
//! in the list of files the writer made, with the dictionary editor, so nothing is read from disk
//! and every other byte of the case stays as it was. An override that cannot be applied is an
//! error, never skipped: the case is the answer to what the study asked for.

use crate::case::{CaseFile, CompileError};
use cfdcraft_dictedit::set_entry;
use cfdcraft_model::{RawBackend, RawOverride};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn refuse(label: &str, reason: impl Into<String>) -> CompileError {
    CompileError::Raw {
        label: label.to_owned(),
        reason: reason.into(),
    }
}

/// A relative path as `/`-joined text, the form a study names a file in.
fn name_of(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Apply every override, in label order, to the file it names.
///
/// Refused, naming the override: a backend tag other than `esi`, a file the case does not have,
/// an entry that another override already set, a file that is not text, and any refusal of the
/// dictionary editor (a block that is not in the file, a value it will not write).
pub(crate) fn apply(
    files: &mut [CaseFile],
    overrides: &BTreeMap<String, RawOverride>,
) -> Result<(), CompileError> {
    let mut set: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (label, item) in overrides {
        if item.backend != RawBackend::Esi {
            return Err(refuse(
                label,
                format!(
                    "it is written for the {} backend and this case is written for esi",
                    item.backend
                ),
            ));
        }
        let Some(file) = files.iter_mut().find(|f| name_of(&f.rel_path) == item.file) else {
            return Err(refuse(label, format!("the case has no file {}", item.file)));
        };
        if !set.insert((item.file.as_str(), item.entry.as_str())) {
            return Err(refuse(
                label,
                format!(
                    "another override already sets {} in {}",
                    item.entry, item.file
                ),
            ));
        }
        let text = std::str::from_utf8(&file.bytes)
            .map_err(|_| refuse(label, format!("{} is not text", item.file)))?;
        let edited = set_entry(text, &item.entry, &item.value)
            .map_err(|e| refuse(label, format!("{} in {}: {e}", item.entry, item.file)))?;
        file.bytes = edited.into_bytes();
    }
    Ok(())
}
