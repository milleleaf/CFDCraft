//! Synthetic result trees for tests: a lift that shed at 3 Hz, a drag that oscillates at twice
//! that, and two pressure probe series, sampled every millisecond. Every number is written with
//! twelve decimals, so the text is the same on every platform. No solver ran.
//!
//! The study must list the force monitor `cylinder_forces` and the pressure probes `front` and
//! `rear`; the files are put under the function-object names the study's monitor plan gives.

use std::f64::consts::PI;
use std::fmt::Write as _;
use std::io;
use std::ops::RangeInclusive;
use std::path::Path;

use cfdcraft_model::ResolvedStudy;

use crate::files::names;

/// The sampling interval, in seconds.
pub const STEP: f64 = 0.001;
/// The lift frequency, in hertz; the Strouhal number of the tree is this times 0.1 m over 1 m/s.
pub const FREQUENCY: f64 = 3.0;

/// How a synthetic run looks.
#[derive(Debug, Clone)]
pub struct Run {
    /// The last step recorded; the series cover `0..=steps` steps of [`STEP`].
    pub steps: u32,
    /// The peak lift.
    pub lift_amplitude: f64,
    /// The lift amplitude grows by this fraction per second (0 for a steady shedding).
    pub growth: f64,
    /// The lift direction line of the coefficient file, for example `(0 1 0)`; `None` leaves the
    /// line out.
    pub lift_dir: Option<&'static str>,
}

impl Run {
    /// Eight seconds of steady shedding with the published numbers: peak drag 3.24, peak lift 1.0,
    /// and a pressure difference of 2.48 half a period after the lift maximum.
    pub fn good() -> Self {
        Self {
            steps: 8000,
            lift_amplitude: 1.0,
            growth: 0.0,
            lift_dir: Some("(0 1 0)"),
        }
    }
}

fn time(step: u32) -> f64 {
    f64::from(step) * STEP
}

/// A force coefficient file for the given steps.
pub fn coefficient_file(run: &Run, steps: RangeInclusive<u32>) -> String {
    let mut text = String::from("# Force coefficients\n# dragDir      : (1 0 0)\n");
    if let Some(direction) = run.lift_dir {
        let _ = writeln!(text, "# liftDir      : {direction}");
    }
    text.push_str("# Time\tCd\tCl\n");
    for step in steps {
        let t = time(step);
        let cd = 3.23 + 0.01 * (2.0 * PI * 2.0 * FREQUENCY * t).sin();
        let cl = run.lift_amplitude * (1.0 + run.growth * t) * (2.0 * PI * FREQUENCY * t).sin();
        let _ = writeln!(text, "{t:.6}\t{cd:.12}\t{cl:.12}");
    }
    text
}

/// A pressure probe file for the front (`true`) or rear probe. The front pressure is
/// `3.18 - 0.05 cos(2 pi f t)` and the rear one is 0.7, so the difference is 2.48 half a period
/// after every lift maximum.
pub fn pressure_file(run: &Run, front: bool) -> String {
    let x = if front { 0.15 } else { 0.25 };
    let mut text = format!(
        "# Probe 0 ({x:.12} 0.200000000000 0.005000000000)\n#       Probe             0\n#        Time\n"
    );
    for step in 0..=run.steps {
        let t = time(step);
        let p = if front {
            3.18 - 0.05 * (2.0 * PI * FREQUENCY * t).cos()
        } else {
            0.7
        };
        let _ = writeln!(text, "{t:.6}   {p:.12}");
    }
    text
}

/// The files of a run: `(path below the results root, text)`. Empty when the study lacks one of
/// the three monitors.
pub fn tree(study: &ResolvedStudy, run: &Run) -> Vec<(String, String)> {
    let Ok(fos) = names(study) else {
        return Vec::new();
    };
    vec![
        (
            format!("postProcessing/{}/0/coefficient.dat", fos.force),
            coefficient_file(run, 0..=run.steps),
        ),
        (
            format!("postProcessing/{}/0/p", fos.front),
            pressure_file(run, true),
        ),
        (
            format!("postProcessing/{}/0/p", fos.rear),
            pressure_file(run, false),
        ),
    ]
}

fn write(root: &Path, files: Vec<(String, String)>) -> io::Result<()> {
    for (rel, text) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    Ok(())
}

/// Write the tree of [`Run::good`] below `root`.
pub fn write_good_results(root: &Path, study: &ResolvedStudy) -> io::Result<()> {
    write(root, tree(study, &Run::good()))
}

/// Write the same tree with the lift direction `(0 0 1)`, the default of the solver.
pub fn write_wrong_axis_results(root: &Path, study: &ResolvedStudy) -> io::Result<()> {
    let run = Run {
        lift_dir: Some("(0 0 1)"),
        ..Run::good()
    };
    write(root, tree(study, &run))
}
