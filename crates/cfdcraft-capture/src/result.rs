use std::fs;
use std::io;
use std::path::Path;

use crate::bundle::{Bundle, read_all, walk_dirs};
use crate::monitors::monitor_names;
use crate::{CaptureError, CapturePlan, FILE_CAP};

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

/// The final fields, centreline samples and logs of a cavity run.
pub fn result_bundle(results: &Path, end_time: f64) -> Result<Bundle, CaptureError> {
    result_bundle_with(results, end_time, &CapturePlan::cavity())
}

/// [`result_bundle`] with a smaller cap per file (for tests).
pub fn result_bundle_limited(
    results: &Path,
    end_time: f64,
    file_cap: u64,
) -> Result<Bundle, CaptureError> {
    result_bundle_limited_with(results, end_time, file_cap, &CapturePlan::cavity())
}

/// The final fields, logs and monitor files of a run, named by `plan`.
pub fn result_bundle_with(
    results: &Path,
    end_time: f64,
    plan: &CapturePlan,
) -> Result<Bundle, CaptureError> {
    result_bundle_limited_with(results, end_time, FILE_CAP, plan)
}

/// [`result_bundle_with`] with a smaller cap per file (for tests).
pub fn result_bundle_limited_with(
    results: &Path,
    end_time: f64,
    file_cap: u64,
    plan: &CapturePlan,
) -> Result<Bundle, CaptureError> {
    let end = find_time_dir(results, end_time)?;
    let mut names: Vec<String> = plan.fields.iter().map(|f| format!("{end}/{f}")).collect();
    names.extend([
        "log.blockMesh".to_owned(),
        "log.checkMesh".to_owned(),
        plan.log.clone(),
    ]);
    if plan.centreline {
        let centre = centreline_folder(results, end_time)?;
        names.push(format!("postProcessing/centreline/{centre}/vertical_U.xy"));
        names.push(format!(
            "postProcessing/centreline/{centre}/horizontal_U.xy"
        ));
    }
    push_if_present(results, format!("{end}/phi"), &mut names)?;
    push_if_present(results, "log.potentialFoam".to_owned(), &mut names)?;
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
