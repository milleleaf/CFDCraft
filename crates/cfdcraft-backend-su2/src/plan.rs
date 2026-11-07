//! How an SU2 case is run: one solver command, one log, one history file.

use crate::markers::{HISTORY_FILE, SOLVER, SOLVER_LOG, THREAD_FLAG, VOLUME_FILE};
use cfdcraft_backend::{RunPlan, RunStep};
use cfdcraft_model::{ResolvedStudy, ResolvedTime};

/// The run plan of a study for `np` threads. The solver writes no fields in folders by time: its
/// final results are the history file and then the volume file, which the result view and the
/// judge both read and which a capture keeps only when the plan names them.
pub fn run_plan(study: &ResolvedStudy, np: u32) -> RunPlan {
    let physics = study.physics.as_ref();
    RunPlan {
        solver: SOLVER.to_owned(),
        steps: vec![RunStep {
            command: format!(
                "{SOLVER} {THREAD_FLAG} {} {}",
                np.max(1),
                crate::markers::CONFIG_FILE
            ),
            log: SOLVER_LOG.to_owned(),
        }],
        solver_log: SOLVER_LOG.to_owned(),
        judged_logs: vec![SOLVER_LOG.to_owned()],
        final_fields: vec![HISTORY_FILE.to_owned(), VOLUME_FILE.to_owned()],
        transient: physics.is_some_and(|p| matches!(p.time, ResolvedTime::Transient { .. })),
        buoyant: physics.is_some_and(|p| p.buoyancy.is_some()),
        energy_field: "T".to_owned(),
        pressure_field: String::new(),
        residual_checks: Vec::new(),
        potential: false,
    }
}
