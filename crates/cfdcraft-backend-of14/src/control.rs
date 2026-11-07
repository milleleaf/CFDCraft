//! `system/controlDict`: the solver module, the run times and the function objects that watch a
//! run. The function objects themselves are written by the monitors module.
//!
//! Only numbers and fixed words are written. A steady run takes one step per iteration; a
//! transient run takes the fixed time step of the study and writes on the clock.

use crate::{
    markers::{FLUID_MODULE, SOLVER_MODULE},
    monitors::{SetLine, functions_block, sets_object},
};
use cfdcraft_backend_esi::{dictionary, scalar};
use cfdcraft_model::ResolvedStudy;

/// The solver module a study runs: the heat transfer module for a study with buoyancy, the plain
/// incompressible module otherwise.
pub fn solver_module(s: &ResolvedStudy) -> &'static str {
    if s.physics.as_ref().is_some_and(|p| p.buoyancy.is_some()) {
        FLUID_MODULE
    } else {
        SOLVER_MODULE
    }
}

/// The time lines of the dictionary: one step per iteration for a steady run, the study's fixed
/// time step and a write on the clock for a transient one.
fn time_lines(s: &ResolvedStudy) -> String {
    match s.effective_delta_t() {
        Some(step) => format!(
            "deltaT {};\nadjustTimeStep no;\nwriteControl runTime;\n",
            scalar(step)
        ),
        None => "deltaT 1;\nwriteControl timeStep;\n".to_owned(),
    }
}

/// The control dictionary: the solver module (`solver incompressibleFluid;` or `solver fluid;`,
/// run by `foamRun`, with no application entry), the time lines, and the two centreline profiles
/// every study samples.
pub fn control_dict(s: &ResolvedStudy) -> String {
    let (x, y) = (scalar(s.domain.length_x), scalar(s.domain.length_y));
    let cx = scalar(s.domain.length_x / 2.0);
    let cy = scalar(s.domain.length_y / 2.0);
    let cz = scalar(s.domain.length_z / 2.0);
    let end = scalar(s.solver.end_time);
    let module = solver_module(s);
    let timing = time_lines(s);
    let interval = scalar(s.solver.write_interval);
    let centreline = sets_object(
        "centreline",
        "U",
        &[
            SetLine {
                name: "vertical",
                axis: "y",
                start: [cx.clone(), "0".into(), cz.clone()],
                end: [cx, y, cz.clone()],
            },
            SetLine {
                name: "horizontal",
                axis: "x",
                start: ["0".into(), cy.clone(), cz.clone()],
                end: [x, cy, cz],
            },
        ],
        s.monitors.centreline_u_sample_count,
    );
    let monitors = functions_block(s);
    dictionary(
        "controlDict",
        "dictionary",
        &format!(
            "solver {module};\nstartFrom startTime;\nstartTime 0;\nstopAt endTime;\nendTime {end};\n{timing}writeInterval {interval};\npurgeWrite 2;\nwriteFormat ascii;\nwritePrecision 12;\nwriteCompression off;\ntimeFormat general;\ntimePrecision 12;\nrunTimeModifiable true;\n\nfunctions\n{{\n{centreline}{monitors}}}\n"
        ),
    )
}
