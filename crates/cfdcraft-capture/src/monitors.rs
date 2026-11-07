//! The monitor files a run leaves, the latest time folder, and the bundle of a stopped run.
//!
//! The folder came from a compute host, so every step uses the file type of the entry itself and
//! never follows a link: a link where a folder or a file belongs is refused by name.

use std::fs;
use std::io;
use std::path::Path;

use crate::bundle::{Bundle, read_all};
use crate::result::push_if_present;
use crate::{CaptureError, CapturePlan, FILE_CAP};

/// The function-object folders under `postProcessing` that hold monitor files: probes, lines,
/// surfaces, balances and force coefficients.
const PREFIXES: [&str; 5] = ["probe_", "line_", "surface_", "balance_", "force_"];
/// The most monitor files one result may carry. A line monitor writes one folder per write time,
/// so the number grows with the lines and the write times; the size of the bundle is bounded
/// separately, so this only stops a folder that holds an absurd number of files.
pub const MAX_MONITOR_FILES: usize = 20_000;

type Listing = Vec<(String, fs::FileType)>;

/// The entries of a folder as (name, type), sorted by name. A name that is not text is skipped.
fn list(dir: &Path) -> io::Result<Listing> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if let Ok(name) = entry.file_name().into_string() {
            found.push((name, entry.file_type()?));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

/// [`list`] for a folder below the results, which must be a real folder (a link is refused). A
/// folder that does not exist gives `None`.
fn entries(dir: &Path, shown: &str) -> Result<Option<Listing>, CaptureError> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_dir() => Ok(Some(list(dir)?)),
        Ok(_) => Err(CaptureError::NotAFile(shown.to_owned())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The monitor files below `postProcessing`, as `postProcessing/<function object>/<folder>/<file>`,
/// sorted. A results folder with no `postProcessing` has none. More than [`MAX_MONITOR_FILES`] files
/// are refused.
pub fn monitor_names(results: &Path) -> Result<Vec<String>, CaptureError> {
    monitor_names_limited(results, MAX_MONITOR_FILES)
}

/// [`monitor_names`] with a smaller limit on the number of files (for tests).
pub fn monitor_names_limited(results: &Path, max: usize) -> Result<Vec<String>, CaptureError> {
    let mut names = Vec::new();
    let Some(objects) = entries(&results.join("postProcessing"), "postProcessing")? else {
        return Ok(names);
    };
    for (object, kind) in objects {
        if !PREFIXES.iter().any(|p| object.starts_with(p)) {
            continue;
        }
        let object_shown = format!("postProcessing/{object}");
        if !kind.is_dir() {
            return Err(CaptureError::NotAFile(object_shown));
        }
        let Some(folders) = entries(&results.join(&object_shown), &object_shown)? else {
            continue;
        };
        for (folder, kind) in folders {
            let folder_shown = format!("{object_shown}/{folder}");
            if kind.is_symlink() {
                return Err(CaptureError::NotAFile(folder_shown));
            }
            if !kind.is_dir() {
                continue;
            }
            let Some(files) = entries(&results.join(&folder_shown), &folder_shown)? else {
                continue;
            };
            for (file, kind) in files {
                let file_shown = format!("{folder_shown}/{file}");
                if kind.is_symlink() {
                    return Err(CaptureError::NotAFile(file_shown));
                }
                if kind.is_file() {
                    if names.len() == max {
                        return Err(CaptureError::Invalid(format!(
                            "more than {max} monitor files"
                        )));
                    }
                    names.push(file_shown);
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

/// The real folders of `results` whose name is a finite number above zero, with that number.
fn time_dirs(results: &Path) -> Result<Vec<(String, f64)>, CaptureError> {
    // The results folder itself is the caller's choice, so it is listed as given.
    let listing = match list(results) {
        Ok(listing) => listing,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    Ok(listing
        .into_iter()
        .filter(|(_, kind)| kind.is_dir())
        .filter_map(|(name, _)| {
            let time = name
                .parse::<f64>()
                .ok()
                .filter(|t| t.is_finite() && *t > 0.0)?;
            Some((name, time))
        })
        .collect())
}

/// The name of the time folder with the largest number above zero; on a tie the smaller text.
pub fn latest_time_dir(results: &Path) -> Result<String, CaptureError> {
    time_dirs(results)?
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(name, _)| name)
        .ok_or_else(|| CaptureError::Missing("a time folder".to_owned()))
}

/// What a stopped cavity run needs to continue and be judged: its fields at `time`, the three
/// logs and the monitor files. `time` must name one of the real time folders of `results`.
pub fn checkpoint_bundle(results: &Path, time: &str) -> Result<Bundle, CaptureError> {
    checkpoint_bundle_with(results, time, &CapturePlan::cavity())
}

/// [`checkpoint_bundle`] for the fields and log `plan` names.
pub fn checkpoint_bundle_with(
    results: &Path,
    time: &str,
    plan: &CapturePlan,
) -> Result<Bundle, CaptureError> {
    let folder = time_dirs(results)?
        .into_iter()
        .find(|(name, _)| name == time)
        .map(|(name, _)| name)
        .ok_or_else(|| CaptureError::Missing("the requested time folder".to_owned()))?;
    let mut names: Vec<String> = plan
        .fields
        .iter()
        .map(|f| format!("{folder}/{f}"))
        .collect();
    names.extend(plan.logs.iter().cloned());
    names.push(plan.log.clone());
    push_if_present(results, format!("{folder}/phi"), &mut names)?;
    names.extend(monitor_names(results)?);
    read_all(results, names, FILE_CAP)
}
