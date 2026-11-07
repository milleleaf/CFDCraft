//! Synthetic result files of a heated-cavity run that reproduces Table V, for tests of this crate
//! and of the code that uses it. Nothing here is solver output.
//!
//! The heat-flux files hold the gradient integral that gives the published Nusselt number, and
//! the two mid-plane lines are parabolas that peak at the published value and position.

use std::{fs, io, path::Path};

use cfdcraft_model::ResolvedStudy;

use crate::data::Table;
use crate::metrics::Setup;

/// Samples on each mid-plane line.
const SAMPLES: usize = 41;

/// How far a synthetic run is moved from the published numbers: factors for the Nusselt number,
/// the two maxima and the cold-wall flow, and shifts (in cavity sides) for the two positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tweak {
    pub nu: f64,
    pub u_max: f64,
    pub w_max: f64,
    pub cold_flux: f64,
    pub u_max_shift: f64,
    pub w_max_shift: f64,
}

impl Default for Tweak {
    fn default() -> Self {
        Self {
            nu: 1.0,
            u_max: 1.0,
            w_max: 1.0,
            cold_flux: 1.0,
            u_max_shift: 0.0,
            w_max_shift: 0.0,
        }
    }
}

fn surface_text(boundary: &str, area: f64, time: f64, integral: f64) -> String {
    format!(
        "# Region type : patch {boundary}\n# Faces : 40\n# Area : {area:.12}\n# Time areaIntegrate(gradT)\n1 ({half:.12} 0 0)\n{time} ({integral:.12} 0 0)\n",
        half = integral / 2.0
    )
}

/// Six columns, x y z Ux Uy Uz, for a line along `axis` (0 for x, 1 for y) in a cavity of side
/// `length` and depth `depth`. The component `component` follows a parabola that peaks at
/// `at` (in cavity sides) with the value `top` (in m/s).
fn line_text(
    length: f64,
    depth: f64,
    axis: usize,
    component: usize,
    (at, top): (f64, f64),
) -> String {
    let mut out = String::from("# x y z Ux Uy Uz\n");
    for i in 0..SAMPLES {
        let t = i as f64 / (SAMPLES - 1) as f64;
        let along = t * length;
        let value = top * (1.0 - 2.0 * (t - at) * (t - at));
        let (x, y) = if axis == 0 {
            (along, length / 2.0)
        } else {
            (length / 2.0, along)
        };
        let mut u = [0.0; 3];
        u[component] = value;
        out.push_str(&format!(
            "{x:.12} {y:.12} {z:.12} {:.12} {:.12} {:.12}\n",
            u[0],
            u[1],
            u[2],
            z = depth / 2.0
        ));
    }
    out
}

/// The files of a passing run of the study: `(path below the results folder, text)` pairs. The
/// study must be a heated cavity at a tabulated Rayleigh number.
pub fn good_files(r: &ResolvedStudy, tweak: &Tweak) -> Result<Vec<(String, String)>, String> {
    let setup = Setup::from_study(r).map_err(|found| {
        let list: Vec<String> = found.iter().map(ToString::to_string).collect();
        list.join("; ")
    })?;
    let table = Table::load().map_err(|e| e.to_string())?;
    let row = table
        .row_for_ra(setup.rayleigh)
        .ok_or_else(|| format!("Rayleigh number {} is not tabulated", setup.rayleigh))?;
    let folder = r.solver.end_time.to_string();
    let time = r.solver.end_time;
    let length = setup.length;
    let area = length * r.domain.length_z;
    let hot = -row.nu_avg * tweak.nu * area * setup.delta_t / length;
    let cold = hot * tweak.cold_flux;
    let scale = setup.velocity_scale();
    let u_top = row.u_max * tweak.u_max / scale;
    let w_top = row.w_max * tweak.w_max / scale;
    let surface = |fo: &str, boundary: &str, integral: f64| {
        (
            format!("postProcessing/{fo}/{folder}/surfaceFieldValue.dat"),
            surface_text(boundary, area, time, integral),
        )
    };
    let line = |fo: &str, axis: usize, component: usize, peak: (f64, f64)| {
        (
            format!("postProcessing/{fo}/{folder}/line_U.xy"),
            line_text(length, r.domain.length_z, axis, component, peak),
        )
    };
    Ok(vec![
        surface(&setup.hot_fo, &setup.hot_boundary, hot),
        surface(&setup.cold_fo, "cold", cold),
        line(
            &setup.vertical_fo,
            1,
            0,
            (row.u_max_z + tweak.u_max_shift, u_top),
        ),
        line(
            &setup.horizontal_fo,
            0,
            1,
            (row.w_max_x + tweak.w_max_shift, w_top),
        ),
    ])
}

/// Write the files of a passing run below `root`.
pub fn write_good_results(root: &Path, r: &ResolvedStudy) -> io::Result<()> {
    for (rel, text) in good_files(r, &Tweak::default()).map_err(io::Error::other)? {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    Ok(())
}
