//! Finding and reading the monitor files of a finished heated-cavity run.
//!
//! A monitor writes `postProcessing/<function object>/<time folder>/<file>`. The newest time
//! folder is the one whose name is the largest number, so a run that was continued is read from
//! its last stretch.

use cfdcraft_gate::ResultFiles;
use cfdcraft_monitors::{LineFile, SurfaceFile, parse_surface, parse_xy};
use thiserror::Error;

/// Why a run cannot be judged. Every text starts with the code of a missing or unusable result.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum Refusal {
    #[error("CFD-BENCH-002 the results lack {file}")]
    Missing { file: String },
    #[error("CFD-BENCH-002 {file} cannot be read: {reason}")]
    Unreadable { file: String, reason: String },
    #[error("CFD-BENCH-002 {what}")]
    Unusable { what: String },
}

/// The name of a surface file written by a surface integral function object.
const SURFACE_FILE: &str = "surfaceFieldValue.dat";
/// The name of a sampled line of velocity: the set is called `line` and the field is `U`.
const LINE_FILE: &str = "line_U.xy";

/// The newest time folder of a function object.
fn latest_folder(files: &dyn ResultFiles, fo: &str) -> Result<String, Refusal> {
    let dir = format!("postProcessing/{fo}");
    files
        .list(&dir)
        .into_iter()
        .filter_map(|name| {
            let time: f64 = name.parse().ok()?;
            time.is_finite().then_some((name, time))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(name, _)| name)
        .ok_or(Refusal::Missing { file: dir })
}

fn read_text(files: &dyn ResultFiles, rel: &str) -> Result<String, Refusal> {
    files.read(rel).ok_or_else(|| Refusal::Missing {
        file: rel.to_owned(),
    })
}

/// The newest surface file of a function object, with its path.
pub fn read_surface(files: &dyn ResultFiles, fo: &str) -> Result<(SurfaceFile, String), Refusal> {
    let folder = latest_folder(files, fo)?;
    let rel = format!("postProcessing/{fo}/{folder}/{SURFACE_FILE}");
    let parsed = parse_surface(&read_text(files, &rel)?).map_err(|e| Refusal::Unreadable {
        file: rel.clone(),
        reason: e.to_string(),
    })?;
    Ok((parsed, rel))
}

/// The newest sampled line of a function object, with its path. The velocity file is the one
/// called `line_U.xy`; when a run names it differently, the first `.xy` file is taken.
pub fn read_line(files: &dyn ResultFiles, fo: &str) -> Result<(LineFile, String), Refusal> {
    let folder = latest_folder(files, fo)?;
    let dir = format!("postProcessing/{fo}/{folder}");
    let names = files.list(&dir);
    let name = if names.iter().any(|n| n == LINE_FILE) {
        LINE_FILE.to_owned()
    } else {
        names
            .into_iter()
            .find(|n| n.ends_with(".xy"))
            .unwrap_or_else(|| LINE_FILE.to_owned())
    };
    let rel = format!("{dir}/{name}");
    let parsed = parse_xy(&read_text(files, &rel)?).map_err(|e| Refusal::Unreadable {
        file: rel.clone(),
        reason: e.to_string(),
    })?;
    Ok((parsed, rel))
}
