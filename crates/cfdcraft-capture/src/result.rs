use std::fs;
use std::io;
use std::path::Path;

use crate::bundle::{Bundle, read_all, walk_dirs};
use crate::monitors::monitor_names;
use crate::{CaptureError, FILE_CAP};

/// The folder below `base` (relative to `results`) whose name, read as a number, is the end time.
/// `5000` and `5000.0` are the same time. Only real folders count; when several match, the
/// smallest name wins.
fn time_folder_in(
    results: &Path,
    base: &[&str],
    end_time: f64,
    what: &str,
) -> Result<String, CaptureError> {
    let missing = || CaptureError::Missing(what.to_owned());
    let listing = match fs::read_dir(walk_dirs(results, base)?) {
        Ok(listing) => listing,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(missing()),
        Err(e) => return Err(e.into()),
    };
    let mut found = Vec::new();
    for entry in listing {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name
            .parse::<f64>()
            .is_ok_and(|t| (t - end_time).abs() < 1e-6)
        {
            found.push(name);
        }
    }
    found.into_iter().min().ok_or_else(missing)
}

/// The name of the end-time folder of a results folder, found by value.
pub fn find_time_dir(results: &Path, end_time: f64) -> Result<String, CaptureError> {
    time_folder_in(results, &[], end_time, "the end-time folder")
}

/// The name of the end-time folder inside `postProcessing/centreline`; it may spell the time
/// differently from the field folder.
pub(crate) fn centreline_folder(results: &Path, end_time: f64) -> Result<String, CaptureError> {
    time_folder_in(
        results,
        &["postProcessing", "centreline"],
        end_time,
        "the end-time folder in postProcessing/centreline",
    )
}

/// The final fields, centreline samples and logs of a run.
pub fn result_bundle(results: &Path, end_time: f64) -> Result<Bundle, CaptureError> {
    result_bundle_limited(results, end_time, FILE_CAP)
}

/// [`result_bundle`] with a smaller cap per file (for tests).
pub fn result_bundle_limited(
    results: &Path,
    end_time: f64,
    file_cap: u64,
) -> Result<Bundle, CaptureError> {
    let end = find_time_dir(results, end_time)?;
    let centre = centreline_folder(results, end_time)?;
    let mut names = vec![
        format!("{end}/U"),
        format!("{end}/p"),
        "log.blockMesh".to_owned(),
        "log.checkMesh".to_owned(),
        "log.simpleFoam".to_owned(),
        format!("postProcessing/centreline/{centre}/vertical_U.xy"),
        format!("postProcessing/centreline/{centre}/horizontal_U.xy"),
    ];
    push_if_present(results, format!("{end}/phi"), &mut names)?;
    names.extend(monitor_names(results)?);
    read_all(results, names, file_cap)
}

/// Adds `rel` to `names` when something is there; whatever is there must then be a plain file,
/// which the read that follows checks.
pub(crate) fn push_if_present(
    results: &Path,
    rel: String,
    names: &mut Vec<String>,
) -> Result<(), CaptureError> {
    match fs::symlink_metadata(results.join(&rel)) {
        Ok(_) => names.push(rel),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
