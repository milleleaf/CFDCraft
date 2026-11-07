//! Says why a job failed. Whether the log of a run that ended shows a failure is asked of
//! [`classify`]; [`finish_failed`] ends a job as failed with the sentence for a person.

use cfdcraft_logscan::{
    Divergence, DivergenceCode, ErrorId, Limits, Scanner, divergence, explain, redact, tail_lines,
};
use cfdcraft_queue::Job;
use cfdcraft_remote::{ContainerState, Host};
use cfdcraft_text::Line;

use crate::DrainError;
use crate::finalize;
use crate::tick::{Drain, FETCH_CAP, run_id_of};

/// How many lines of the end of a log an unknown error shows.
const TAIL_LINES: usize = 20;
/// The logs of a case, newest first: the one with text that comes first names the failure.
fn logs_newest_first(solver_log: &str) -> [String; 3] {
    [
        solver_log.to_owned(),
        "log.checkMesh".to_owned(),
        "log.blockMesh".to_owned(),
    ]
}
/// The code of a run that the time limit of the compute host ended.
pub const TIME_LIMIT: &str = "CFD-RUN-009";
/// The exit statuses of a run ended by `timeout`: 124 when the solver stopped at the limit, 137
/// when it had to be killed after the grace period.
const TIME_LIMIT_STATUSES: [i32; 2] = [124, 137];
/// Seconds a container that is still running is given to stop before its job is ended.
const GRACE_SECONDS: u32 = 30;

/// What the mirrored log of a job that ended shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Classified {
    /// Nothing in the log says the run failed; the files are judged next.
    Clean,
    /// The run failed; `code` is the stable code and `line` the sentence for a person.
    Failed { code: String, line: Line },
}

/// The sentence for a divergence. A rule with no field or limit of its own is named in words.
fn diverged_line(found: &Divergence) -> Line {
    let (field, value, limit) = match found.code {
        DivergenceCode::Trap => (
            "the solution".to_owned(),
            "an invalid number".to_owned(),
            "what a number can hold".to_owned(),
        ),
        DivergenceCode::ResidualBlowup => (
            found
                .field
                .clone()
                .unwrap_or_else(|| "a residual".to_owned()),
            found.value.to_string(),
            found.limit.to_string(),
        ),
        DivergenceCode::Courant => (
            "the Courant number".to_owned(),
            found.value.to_string(),
            found.limit.to_string(),
        ),
        DivergenceCode::ContinuityGrowing => (
            "the continuity error".to_owned(),
            found.value.to_string(),
            found.limit.to_string(),
        ),
    };
    Line::RunDiverged {
        code: found.code.code().to_owned(),
        field,
        value,
        limit,
    }
}

/// The sentence for a known error. Only an unknown one carries text from the log, and that text
/// is redacted here, before it reaches a [`Line`].
fn error_line(id: ErrorId, tail: &str) -> Line {
    let code = id.code().to_owned();
    match id {
        ErrorId::MissingFile => Line::SolverMissingFile { code },
        ErrorId::NumericalBlowup => Line::SolverNumericalBlowup { code },
        ErrorId::UnknownType => Line::SolverUnknownType { code },
        ErrorId::MissingKeyword => Line::SolverMissingKeyword { code },
        ErrorId::LinearSolverNotConverged => Line::SolverLinearNotConverged { code },
        ErrorId::PoorMesh => Line::SolverPoorMesh { code },
        ErrorId::OutOfMemory => Line::SolverOutOfMemory { code },
        ErrorId::DiskFull => Line::SolverDiskFull { code },
        ErrorId::DecompositionMismatch => Line::SolverDecomposition { code },
        ErrorId::NotEnoughSlots => Line::SolverNotEnoughSlots { code },
        ErrorId::SegmentationFault => Line::SolverSegfault { code },
        // A sentence has no line breaks, so the lines of the tail are joined with a visible one.
        ErrorId::Unknown => Line::SolverUnknown {
            tail: redact(tail).replace('\n', " | "),
        },
    }
}

/// How the container ended: its exit status and whether it was killed for lack of memory. What the
/// last inspection saw is used; after a restart the host is asked, as the container is only
/// removed when the job ends.
fn container_end<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
) -> Result<(Option<i32>, bool), DrainError> {
    if ctx.scratch.exit_code.is_some() {
        return Ok((ctx.scratch.exit_code, ctx.scratch.oom_killed));
    }
    let run_id = run_id_of(job)?.to_owned();
    Ok(match ctx.host.inspect(&run_id)? {
        ContainerState::Exited { code, oom_killed } => {
            (Some(code), oom_killed || ctx.scratch.oom_killed)
        }
        _ => (None, ctx.scratch.oom_killed),
    })
}

/// Reads the mirrored log of a job that ended. A run that diverged says which rule it broke; a
/// log that ends with `End` and shows no fatal line is clean; anything else is a known solver
/// error or an unknown one with the redacted last lines. A container killed for lack of memory
/// that left no recognised message is an out-of-memory failure.
pub fn classify<H: Host>(ctx: &mut Drain<H>, job: &Job) -> Result<Classified, DrainError> {
    let mirror = ctx.mirror(job)?;
    let solver_log = ctx.solver_log(job)?;
    let mut solver = mirror.read_to_string(&solver_log)?;
    let mut scanner = Scanner::new();
    scanner.feed(&solver);
    // A full live view stops growing while the solver goes on, so the mirror of a long run (a fine
    // mesh, a transient with a line of force coefficients every step) can stop before the end of
    // the log. The host still holds the rest: read on from where the mirror stops, so that the run
    // is not judged by a log that was cut.
    if !scanner.state().ended {
        let run_id = run_id_of(job)?.to_owned();
        let mut at = mirror.len(&solver_log);
        while let Ok(piece) = ctx.host.tail(&run_id, &solver_log, at, FETCH_CAP) {
            if piece.is_empty() {
                break;
            }
            at += piece.len() as u64;
            let text = String::from_utf8_lossy(&piece);
            scanner.feed(&text);
            solver.push_str(&text);
        }
    }
    let state = scanner.state();
    if let Some(found) = divergence(state, &Limits::default(), false) {
        return Ok(Classified::Failed {
            code: found.code.code().to_owned(),
            line: diverged_line(&found),
        });
    }
    if state.ended && state.fatal.is_none() {
        return Ok(Classified::Clean);
    }
    let mut log = solver;
    for rel in logs_newest_first(&solver_log) {
        if !log.trim().is_empty() {
            break;
        }
        log = mirror.read_to_string(&rel)?;
    }
    let tail = tail_lines(&log, TAIL_LINES);
    let mut id = explain(tail);
    if id == ErrorId::Unknown {
        let (status, oom_killed) = container_end(ctx, job)?;
        if oom_killed {
            id = ErrorId::OutOfMemory;
        } else if let Some(status) = status.filter(|s| TIME_LIMIT_STATUSES.contains(s)) {
            return Ok(Classified::Failed {
                code: TIME_LIMIT.to_owned(),
                line: Line::JobFailed {
                    job: job.id.clone(),
                    reason: format!(
                        "the run ended with exit status {status}, which is how the time limit of the compute host ends a run. The fields it had written were not kept, so it cannot be continued. Run a smaller case, or ask for a higher limit"
                    ),
                },
            });
        }
    }
    Ok(Classified::Failed {
        code: id.code().to_owned(),
        line: error_line(id, tail),
    })
}

/// Ends a job as failed with `code` and the sentence for `line`, gives the lock back if it is
/// ours and removes the container, stopping it first if it is somehow still running. Nothing is
/// downloaded: the mirror holds the logs.
pub fn finish_failed<H: Host>(
    ctx: &mut Drain<H>,
    job: &Job,
    code: &str,
    line: Line,
) -> Result<(), DrainError> {
    let sentence = ctx.text.render(&line);
    let run_id = run_id_of(job)?.to_owned();
    if ctx.host.inspect(&run_id)? == ContainerState::Running {
        ctx.host.stop_container(&run_id, GRACE_SECONDS)?;
    }
    let lines = std::slice::from_ref(&sentence);
    // An error nothing recognises keeps its folder on the host: a person may need all of its output.
    if code == UNKNOWN_ERROR_CODE {
        finalize::fail(ctx, job, code, lines, true)
    } else {
        finalize::fail_explained(ctx, job, code, lines)
    }
}

/// The code of an error the table does not know (`ErrorId::Unknown`).
const UNKNOWN_ERROR_CODE: &str = "CFD-SOLVER-099";
