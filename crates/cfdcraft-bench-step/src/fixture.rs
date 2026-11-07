//! Synthetic result files of a finished step run, for the tests of code that reads reports.
//!
//! The files are made, not run: a floor line whose velocity changes sign at the published
//! reattachment length and, for the turbulent step, profile lines equal to the published ones. A
//! study that matches neither step gets nothing.

use std::{fs, io, path::Path};

use cfdcraft_gate::Benchmark;
use cfdcraft_model::ResolvedStudy;

use crate::armaly::{ArmalyBenchmark, reynolds_number};
use crate::data::{ArmalyCurve, DsProfiles, Station};
use crate::ds::StepBenchmark;
use crate::lines::{inlet, step_geometry};

/// The published reattachment length of the turbulent step, in step heights.
const DS_REATTACHMENT: f64 = 6.26;

/// The path of the sample file of a line, in the folder of the study's end time.
fn line_path(r: &ResolvedStudy, label: &str) -> Option<String> {
    let at = r.monitors.lines.iter().position(|l| l.label == label)?;
    Some(format!(
        "postProcessing/line_{}/{}/line_U.xy",
        at + 1,
        r.solver.end_time
    ))
}

/// The floor line: 101 samples along the line whose streamwise velocity grows linearly with the
/// distance and changes sign `crossing` step heights from the step.
fn floor_file(r: &ResolvedStudy, step: f64, crossing: f64) -> Option<(String, String)> {
    let line = r.monitors.lines.iter().find(|l| l.label == "floor")?;
    let length = line.x1 - line.x0;
    let text = (0..=100)
        .map(|i| {
            let d = f64::from(i) * length / 100.0;
            format!(
                "{d:.12} {:.12} 0.000000000000 0.000000000000\n",
                d / step - crossing
            )
        })
        .collect();
    Some((line_path(r, "floor")?, text))
}

/// A profile line equal to the published profile at a station, as distance from the bottom, Ux, Uy, Uz.
fn profile_file(
    r: &ResolvedStudy,
    (step, uref): (f64, f64),
    station: &Station,
) -> Option<(String, String)> {
    let text = station
        .y_over_h
        .iter()
        .zip(&station.u_over_uref)
        .map(|(y, u)| {
            format!(
                "{:.12} {:.12} 0.000000000000 0.000000000000\n",
                y * step,
                u * uref
            )
        })
        .collect();
    Some((
        line_path(r, &format!("profile_x{}", station.x_over_h))?,
        text,
    ))
}

/// The files of a finished run of `r` that the benchmark it matches judges as passed, as pairs of
/// the path below the results folder and the text. `None` when `r` matches neither step.
pub fn good_files(r: &ResolvedStudy) -> Option<Vec<(String, String)>> {
    let (step, _) = step_geometry(r)?;
    if StepBenchmark::new().ok()?.matches(r).is_ok() {
        let uref = inlet(r)?.velocity?;
        let mut files = vec![floor_file(r, step, DS_REATTACHMENT)?];
        for station in &DsProfiles::load().ok()?.stations {
            files.push(profile_file(r, (step, uref), station)?);
        }
        Some(files)
    } else if ArmalyBenchmark::new().ok()?.matches(r).is_ok() {
        let crossing = ArmalyCurve::load()
            .ok()?
            .reference_at(reynolds_number(r)?)?;
        Some(vec![floor_file(r, step, crossing)?])
    } else {
        None
    }
}

/// Write [`good_files`] below `root`.
pub fn write_good_results(root: &Path, r: &ResolvedStudy) -> io::Result<()> {
    let files = good_files(r).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the study matches neither step benchmark",
        )
    })?;
    for (rel, text) in files {
        let path = root.join(rel);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, text)?;
    }
    Ok(())
}
