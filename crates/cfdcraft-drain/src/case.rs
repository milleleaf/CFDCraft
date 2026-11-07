//! Making the case a job runs: how many processes it gets, what it starts from, and the files
//! written for upload.

use std::fs;
use std::path::Path;

use cfdcraft_backend_esi::case::np_for;
use cfdcraft_backend_esi::{CaseFile, Dialect, write_to_disk};
use cfdcraft_ops::CONTINUE_REFUSED;
use cfdcraft_ops::jobs::DocJob;
use cfdcraft_ops::prepare;
use cfdcraft_queue::{Job, JobKind};
use cfdcraft_remote::Host;
use cfdcraft_store::Store;
use cfdcraft_text::Line;

use crate::DrainError;
use crate::tick::{Drain, STUDY};

/// What the case script of a parallel run starts the solver with, up to the process count.
const MPIRUN: &str = "mpirun -np ";

/// How a job starts: the number of processes, and the files of a saved state to start from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPlan {
    pub np: u32,
    pub seed: Vec<CaseFile>,
}

/// Decides how a job starts: as many processes as the cells justify and the host allows, and for a
/// continue job the files of the saved state of the run it continues. A saved state that cannot be
/// read refuses the job before anything is sent to the host.
pub fn prepare_start<H: Host>(
    ctx: &Drain<H>,
    job: &Job,
    planned: &DocJob,
) -> Result<StartPlan, DrainError> {
    let np = np_for(planned.cell_count(), ctx.config.np_cap);
    let seed = match job.kind {
        JobKind::Solve => Vec::new(),
        JobKind::Continue { .. } => seed_of(ctx, planned)?,
    };
    Ok(StartPlan { np, seed })
}

/// The files of the saved state a continue job starts from.
fn seed_of<H: Host>(ctx: &Drain<H>, planned: &DocJob) -> Result<Vec<CaseFile>, DrainError> {
    let refuse =
        |reason: String| DrainError::refused(CONTINUE_REFUSED, Line::ContinueRefused { reason });
    let store = Store::open(cfdcraft_doc::store_dir(&ctx.config.doc))
        .map_err(|_| refuse("the store of the document cannot be opened".to_owned()))?;
    planned.continue_seed(&store).map_err(refuse)
}

/// Extra sentences for the report at the end of a job: the run it continued, and the processes it
/// ran on when there were more than one.
pub fn lines_for<H: Host>(ctx: &Drain<H>, job: &Job) -> Vec<Line> {
    let mut lines = Vec::new();
    if let JobKind::Continue { run } = &job.kind
        && let Some(time) = checkpoint_time(&ctx.config.doc, run)
    {
        lines.push(Line::ContinueFrom {
            run: run.clone(),
            time,
        });
    }
    if let Some(np) = ctx.scratch.np.filter(|n| *n > 1)
        && let Some(cells) = cells_of(ctx, job)
    {
        lines.push(Line::ParallelRun {
            np: np.to_string(),
            cells: cells.to_string(),
        });
    }
    lines
}

/// The time the stopped run `run` was saved at, as the document has it.
fn checkpoint_time(doc: &Path, run: &str) -> Option<String> {
    let opened = cfdcraft_doc::open(doc).ok()?;
    let runs = opened.document.artifacts.runs;
    let found = runs.iter().find(|r| r.id == run)?;
    found.checkpoint.as_ref().map(|c| c.time.clone())
}

/// The cells of the mesh of the study the job was started from.
fn cells_of<H: Host>(ctx: &Drain<H>, job: &Job) -> Option<u64> {
    let study = fs::read_to_string(ctx.live_dir(job).ok()?.join(STUDY)).ok()?;
    let prepared = prepare(&study, Dialect::EsiV2512).ok()?;
    let mesh = &prepared.resolved.mesh;
    mesh.cells_x
        .checked_mul(mesh.cells_y)?
        .checked_mul(mesh.cells_z)
}

/// Brings back the process count of the job in hand after a restart, from the case script that was
/// uploaded. A process that started the job already knows it.
pub fn restore_np<H: Host>(ctx: &mut Drain<H>, job: &Job) {
    if ctx.scratch.np.is_some() {
        return;
    }
    let Ok(live) = ctx.live_dir(job) else {
        return;
    };
    let script = fs::read_to_string(live.join("case").join("Allrun")).unwrap_or_default();
    ctx.scratch.np = Some(np_in(&script));
}

/// The process count a case script runs the solver with; one when it runs it alone.
fn np_in(script: &str) -> u32 {
    script
        .split_once(MPIRUN)
        .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|digits| digits.parse::<u32>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1)
}

/// Writes the case of `job` for `np` processes, started from `seed` when it is not empty, into
/// `dir/case`, after removing a case left there by an earlier try.
pub fn write_case(job: &DocJob, dir: &Path, np: u32, seed: &[CaseFile]) -> Result<(), DrainError> {
    let tree = job.case_tree(np, seed).map_err(|_| DrainError::Io {
        reason: "the case could not be made from the study".to_owned(),
    })?;
    let case = dir.join("case");
    match std::fs::remove_dir_all(&case) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(DrainError::io(&e)),
    }
    write_to_disk(&tree, &case).map_err(|e| DrainError::io(&e))
}
