//! How a case is run: one step that pipes the command stream through the solver's headless
//! server. The server answers each line with a reply, and the replies are the only results that
//! come back, because the solver writes no result file of its own that a fluid run could be read
//! from. The answers are matched to the requests that asked for them, so a stored result keeps the
//! commands beside the replies.

use cfdcraft_backend::{RunPlan, RunStep};
use cfdcraft_model::{ResolvedStudy, ResolvedTime};

/// The folder the case is mounted at inside the image.
pub const READ_ROOT: &str = "/w";

/// The only folder the server may write to; it is made before the server starts.
pub const WRITE_ROOT: &str = "/w/out";

pub const COMMANDS_FILE: &str = "commands.jsonl";
pub const REPLIES_FILE: &str = "replies.jsonl";
pub const LOG_FILE: &str = "log.fieldcraft";

const HEADER: &str = "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n";

/// The one command of a run. It names the CPU server only: the processor is chosen by the
/// commands, and the guard refuses any other.
pub fn step_command() -> String {
    format!(
        "mkdir -p out && fieldcraft-cli mcp --headless --automation-read-root {READ_ROOT} \
         --automation-write-root {WRITE_ROOT} < {COMMANDS_FILE} > {REPLIES_FILE} 2> {LOG_FILE}"
    )
}

/// The run script. It does not depend on the study, so a scan can compare a script found in a
/// case with this text and refuse any difference.
pub fn allrun_text() -> String {
    format!("{HEADER}{} || exit 1\n", step_command())
}

pub fn run_plan(study: &ResolvedStudy) -> RunPlan {
    let physics = study.physics.as_ref();
    RunPlan {
        solver: "fieldcraft-cli".to_owned(),
        steps: vec![RunStep {
            command: step_command(),
            log: LOG_FILE.to_owned(),
        }],
        solver_log: LOG_FILE.to_owned(),
        judged_logs: vec![LOG_FILE.to_owned()],
        final_fields: vec![REPLIES_FILE.to_owned(), COMMANDS_FILE.to_owned()],
        transient: physics.is_some_and(|p| matches!(p.time, ResolvedTime::Transient { .. })),
        buoyant: physics.is_some_and(|p| p.buoyancy.is_some()),
        energy_field: "T".to_owned(),
        pressure_field: String::new(),
        residual_checks: Vec::new(),
        potential: false,
    }
}
