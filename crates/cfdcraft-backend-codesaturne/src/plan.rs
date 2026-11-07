//! How a case is run: one command from the case folder, with every path relative to it.

use crate::{
    markers::{
        PROBES_PRESSURE_FILE, PROBES_VELOCITY_FILE, RESIDUALS_FILE, RUN_CASE_FLAG, RUN_ID,
        RUN_ID_FLAG, RUN_LOG, RUN_PARAM_FLAG, RUN_PROCESSES_FLAG, RUN_SUBCOMMAND,
    },
    setup::SETUP_FILE,
};
use cfdcraft_backend::{RunPlan, RunStep};
use cfdcraft_model::{ResolvedStudy, ResolvedTime};

/// The program every step runs.
pub const SOLVER: &str = "code_saturne";

/// The run plan of a case: one command whatever the study, with the residual history and the
/// monitoring histories as the files a finished run must hold. Only the length of the run and
/// whether it marches in time come from the study.
pub fn run_plan(study: &ResolvedStudy, np: u32) -> RunPlan {
    let transient = study
        .physics
        .as_ref()
        .is_some_and(|p| matches!(p.time, ResolvedTime::Transient { .. }));
    let mut command = format!(
        "{SOLVER} {RUN_SUBCOMMAND} {RUN_CASE_FLAG} . {RUN_PARAM_FLAG} {SETUP_FILE} {RUN_ID_FLAG} {RUN_ID}"
    );
    if np > 1 {
        command.push_str(&format!(" {RUN_PROCESSES_FLAG} {np}"));
    }
    RunPlan {
        solver: SOLVER.to_owned(),
        steps: vec![RunStep {
            command,
            log: RUN_LOG.to_owned(),
        }],
        solver_log: RUN_LOG.to_owned(),
        judged_logs: vec![RUN_LOG.to_owned()],
        final_fields: vec![
            RESIDUALS_FILE.to_owned(),
            PROBES_VELOCITY_FILE.to_owned(),
            PROBES_PRESSURE_FILE.to_owned(),
        ],
        transient,
        buoyant: false,
        energy_field: "T".to_owned(),
        pressure_field: String::new(),
        residual_checks: Vec::new(),
        potential: false,
    }
}
