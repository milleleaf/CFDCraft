//! Running a study on the compute host and judging the result from its evidence.
//!
//! `begin` is the quick part: the gates, the host profile, the tunnel, the preflight and the job
//! lock. `execute` is the long part: upload, run, download, judge. `run_study` is both in a row.
//! Nothing here prints; everything comes back as a [`Finished`].

use crate::{
    cases::cli_problem,
    ghia,
    prepare::{Prepared, prepare, rel_name},
    report::{EvidenceReport, ExitClass, HolderInfo, InfraReport, Report, RunInfo},
};
use cfdcraft_backend_esi::{
    CaseTree, Dialect, EvidenceExpectations, RunEvidence, SmokeExpectations, Verdict, dialect,
    evidence::{PartialExpectations, judge_partial},
    judge_run, judge_smoke, write_to_disk,
};
use cfdcraft_bench_ghia::GhiaTable;
use cfdcraft_executor::{HostProfile, Job, RunConfig, RunError, docker::RunSpec, lock};
use cfdcraft_model::{Diagnostic, ResolvedStudy};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

/// What the run is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Run the case and judge the evidence.
    Run,
    /// Run the case, judge the evidence, then compare the result with the reference.
    Validate,
    /// A few iterations to show the case loads and iterates. Never a validated result.
    Smoke,
}

impl RunMode {
    /// The operation name reports carry.
    pub fn operation(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Validate => "validate",
            Self::Smoke => "smoke",
        }
    }
}

/// Everything a run needs besides the study text.
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub mode: RunMode,
    /// The ssh key file. It is passed to ssh and appears in no report.
    pub key: PathBuf,
    pub hosts: PathBuf,
    pub profile: String,
    /// The parent of all runs: each run gets `<runs_dir>/<run id>/`.
    pub runs_dir: PathBuf,
    /// A directory exported earlier. It is run only if it is byte-identical to the study's export.
    pub case_dir: Option<PathBuf>,
    /// A tolerance file for the reference comparison.
    pub tolerance_override: Option<PathBuf>,
    /// A short directory for the tunnel socket.
    pub socket_dir: PathBuf,
    /// Set from any thread to stop the job.
    pub cancel: Arc<AtomicBool>,
    /// A short plain name that becomes part of the run id.
    pub label: String,
}

impl RunRequest {
    pub fn new(
        mode: RunMode,
        key: impl Into<PathBuf>,
        hosts: impl Into<PathBuf>,
        profile: &str,
        runs_dir: impl Into<PathBuf>,
        socket_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            mode,
            key: key.into(),
            hosts: hosts.into(),
            profile: profile.to_owned(),
            runs_dir: runs_dir.into(),
            case_dir: None,
            tolerance_override: None,
            socket_dir: socket_dir.into(),
            cancel: Arc::default(),
            label: "study".to_owned(),
        }
    }
}

/// How a run ended: the report to hand to a caller, and the executor's full error text for the
/// operator's stderr. The detail can name the key, the host and remote directories, so it is never
/// part of the report.
#[derive(Debug, Clone)]
pub struct Finished {
    pub report: Report,
    pub detail: Option<String>,
}

/// A job that holds the compute host's lock and has not started its container yet.
pub struct Begun {
    job: Job,
    prepared: Prepared,
    mode: RunMode,
    case: PathBuf,
    results: PathBuf,
    run_dir: PathBuf,
    cap_seconds: u64,
    /// The reference for a validate run.
    table: Option<GhiaTable>,
}

impl Begun {
    /// The run id, which is also the name of the container.
    pub fn run_id(&self) -> &str {
        self.job.run_id()
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    /// The wall-clock cap of the host profile, in seconds.
    pub fn cap_seconds(&self) -> u64 {
        self.cap_seconds
    }
}

fn finished(report: Report) -> Finished {
    Finished {
        report,
        detail: None,
    }
}

/// How long to tell a caller to wait: the holder's remaining cap time clamped to 30 through 600
/// seconds, 600 for a stale lock, 60 when no holder is known.
// Ceiling: the 60 s default for an unknown holder is an assumption; make it a setting if it proves wrong.
pub(crate) fn retry_after(remaining_seconds: Option<u64>, stale: bool) -> u64 {
    match remaining_seconds {
        _ if stale => 600,
        Some(left) => left.clamp(30, 600),
        None => 60,
    }
}

/// The report for a failure of the machine, the network or the container engine. Class 4.
pub(crate) fn infra_finished(
    operation: &str,
    error: &RunError,
    prepared: Option<&Prepared>,
    run_id: Option<&str>,
) -> Finished {
    let mut report = Report::from_diagnostics(
        operation,
        prepared.map(|p| p.warnings.clone()).unwrap_or_default(),
        prepared.map(|p| p.hash.clone()),
    )
    .with_migration(prepared.and_then(|p| p.migration.clone()));
    let (holder, stale) = match error {
        RunError::Busy { holder, stale } => (holder.as_ref(), *stale),
        _ => (None, false),
    };
    let now = lock::now_epoch().unwrap_or(0);
    let retry = error
        .is_busy()
        .then(|| retry_after(holder.map(|h| h.remaining_seconds(now)), stale));
    let message = error.public_message();
    let mut finding = Diagnostic::error(error.infra_code(), "compute host", "", message.clone());
    if let Some(seconds) = retry {
        finding = finding.with_hint(format!("Try again in about {seconds} seconds."));
    }
    report.diagnostics.push(finding);
    report.set_class(ExitClass::Infrastructure);
    report.infra = Some(InfraReport {
        code: error.infra_code().to_owned(),
        message,
        holder: holder.map(|h| HolderInfo {
            run_id: h.run_id.clone(),
            container: h.container.clone(),
            started_epoch: h.started_epoch,
            cap_seconds: h.cap_seconds,
        }),
        retry_after_seconds: retry,
        stale,
    });
    if let Some(id) = run_id {
        report.run = Some(RunInfo {
            run_id: id.to_owned(),
            container: id.to_owned(),
            duration_seconds: 0.0,
            process_exit: None,
        });
    }
    Finished {
        report,
        detail: Some(error.detail()),
    }
}

/// Write the report next to the run's other files. A failure to write is returned as text for the
/// operator; the report itself is still handed to the caller.
fn persist(run_dir: &Path, report: &Report) -> Option<String> {
    std::fs::create_dir_all(run_dir)
        .and_then(|()| {
            craft_atomic::atomic_write(&run_dir.join("report.json"), report.to_json().as_bytes())
        })
        .err()
        .map(|e| format!("the report could not be saved: {e}"))
}

/// The most iterations a smoke run may ask for.
pub const SMOKE_ITERATION_LIMIT: f64 = 60.0;

/// A mode the study cannot be run in. Checked before any host profile is read.
fn mode_problem(mode: RunMode, r: &ResolvedStudy) -> Option<Diagnostic> {
    match mode {
        RunMode::Run => None,
        RunMode::Smoke if r.solver.end_time > SMOKE_ITERATION_LIMIT => Some(Diagnostic::error(
            "CFD-RUN-001",
            "end time",
            "/solver/end_time",
            format!(
                "A smoke run is limited to {SMOKE_ITERATION_LIMIT} iterations; this study asks for {}.",
                r.solver.end_time
            ),
        )),
        RunMode::Smoke => None,
        RunMode::Validate => {
            let not_cavity = |message: String| {
                Diagnostic::error("CFD-RUN-002", "reference comparison", "", message).with_hint(
                    "Use a study of the square lid-driven cavity at Reynolds number 100.",
                )
            };
            let Some(lid) = r.lid_velocity() else {
                return Some(not_cavity(
                    "The reference comparison needs a lid-driven cavity: exactly one moving wall on the top face.".into(),
                ));
            };
            let re = lid * r.domain.length_y / r.fluid.kinematic_viscosity;
            let square = (r.domain.length_x / r.domain.length_y - 1.0).abs() <= 1e-10;
            (!square || (re - 100.0).abs() > 1e-8).then(|| {
                not_cavity(format!(
                    "The reference comparison is defined for a square cavity at Reynolds number 100; this study has Reynolds number {re}{}.",
                    if square { "" } else { " and is not square" }
                ))
            })
        }
    }
}

/// A class 2 or 3 report for a problem found after the gates passed but before anything started.
fn refused(operation: &str, prepared: &Prepared, found: Diagnostic) -> Finished {
    let mut all = prepared.warnings.clone();
    all.push(found);
    finished(
        Report::from_diagnostics(operation, all, Some(prepared.hash.clone()))
            .with_migration(prepared.migration.clone()),
    )
}

fn export_mismatch(path: &str) -> Diagnostic {
    Diagnostic::error(
        "CFD-RT-003",
        "exported case",
        "",
        format!("The case directory differs from the study's export at {path}."),
    )
    .with_hint("Export the study again into a new directory, or run it without a case directory.")
}

/// Every file under `dir`, by `/`-separated path. Symlinks, special files and empty directories
/// are not followed or ignored: each one is a difference.
// The finding is the error on purpose.
#[allow(clippy::result_large_err)]
fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), Diagnostic> {
    let unreadable = |e: std::io::Error| {
        cli_problem(format!(
            "The case directory {} cannot be read: {e}.",
            root.display()
        ))
    };
    let mut any = false;
    for entry in std::fs::read_dir(dir).map_err(unreadable)? {
        let entry = entry.map_err(unreadable)?;
        any = true;
        let path = entry.path();
        let name = rel_name(path.strip_prefix(root).unwrap_or(&path));
        let kind = entry.file_type().map_err(unreadable)?;
        if kind.is_dir() {
            collect_files(root, &path, out)?;
        } else if kind.is_file() {
            out.insert(name, std::fs::read(&path).map_err(unreadable)?);
        } else {
            return Err(export_mismatch(&name));
        }
    }
    if !any && dir != root {
        return Err(export_mismatch(&rel_name(
            dir.strip_prefix(root).unwrap_or(dir),
        )));
    }
    Ok(())
}

/// The directory must hold exactly the files and bytes of the study's export, nothing else.
// The finding is the error on purpose.
#[allow(clippy::result_large_err)]
fn verify_case_dir(dir: &Path, export: &CaseTree) -> Result<(), Diagnostic> {
    if !dir.is_dir() {
        return Err(cli_problem(format!(
            "{} is not a directory.",
            dir.display()
        )));
    }
    let mut found = BTreeMap::new();
    collect_files(dir, dir, &mut found)?;
    let expected: BTreeMap<String, &[u8]> = export
        .files
        .iter()
        .map(|f| (rel_name(&f.rel_path), f.bytes.as_slice()))
        .collect();
    let names: std::collections::BTreeSet<&String> = found.keys().chain(expected.keys()).collect();
    match names
        .into_iter()
        .find(|n| found.get(*n).map(Vec::as_slice) != expected.get(*n).copied())
    {
        Some(first) => Err(export_mismatch(first)),
        None => Ok(()),
    }
}

/// The quick part of a run. Returns the finished, classified report when the study is invalid or
/// refused (before any host profile is read or any connection is made), or when the host is busy
/// or unreachable.
// The finished report is the error on purpose.
#[allow(clippy::result_large_err)]
pub fn begin(study_json: &str, request: &RunRequest) -> Result<Begun, Finished> {
    let operation = request.mode.operation();
    let prepared = prepare(study_json, Dialect::EsiV2512)
        .map_err(|report| finished(report.for_operation(operation)))?;
    if let Some(found) = mode_problem(request.mode, &prepared.resolved) {
        return Err(refused(operation, &prepared, found));
    }
    let table = if request.mode == RunMode::Validate {
        let loaded = ghia::load_table(request.tolerance_override.as_deref()).map_err(|e| {
            let found = cli_problem(format!("The tolerance file cannot be used: {e}."));
            refused(operation, &prepared, found)
        })?;
        Some(loaded)
    } else {
        None
    };
    if let Some(dir) = &request.case_dir {
        verify_case_dir(dir, &prepared.export_tree)
            .map_err(|found| refused(operation, &prepared, found))?;
    }
    let setup = |message: String| {
        infra_finished(operation, &RunError::Setup(message), Some(&prepared), None)
    };
    let profile =
        HostProfile::load(&request.hosts, &request.profile).map_err(|e| setup(e.to_string()))?;
    let config = RunConfig {
        socket_dir: request.socket_dir.clone(),
        cancel: request.cancel.clone(),
    };
    let job = Job::begin(&profile, &request.key, config, &request.label)
        .map_err(|e| infra_finished(operation, &e, Some(&prepared), None))?;
    let run_dir = request.runs_dir.join(job.run_id());
    // A verified export directory is uploaded as it is; otherwise the export is written fresh.
    let case = match &request.case_dir {
        Some(dir) => dir.clone(),
        None => {
            let case = run_dir.join("case");
            write_to_disk(&prepared.export_tree, &case).map_err(|e| {
                infra_finished(
                    operation,
                    &RunError::Setup(format!("the case could not be written: {e}")),
                    Some(&prepared),
                    Some(job.run_id()),
                )
            })?;
            case
        }
    };
    Ok(Begun {
        results: run_dir.join("results"),
        cap_seconds: profile.wall_clock_seconds,
        table,
        job,
        prepared,
        mode: request.mode,
        case,
        run_dir,
    })
}

/// The three logs and the numeric time directories a run left in its results directory.
fn read_evidence(results: &Path) -> (String, String, String, Vec<f64>) {
    let log = |name: &str| std::fs::read_to_string(results.join(name)).unwrap_or_default();
    let times = std::fs::read_dir(results)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_string_lossy().parse::<f64>().ok())
        .collect();
    (
        log("log.blockMesh"),
        log("log.checkMesh"),
        log("log.simpleFoam"),
        times,
    )
}

/// True when the final time directory holds a non-empty `U` and `p`.
fn final_fields_present(results: &Path, end_time: f64) -> bool {
    std::fs::read_dir(results)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .parse::<f64>()
                .is_ok_and(|t| (t - end_time).abs() < 1e-6)
        })
        .any(|dir| {
            ["U", "p"].iter().all(|name| {
                std::fs::metadata(dir.path().join(name)).is_ok_and(|m| m.is_file() && m.len() > 0)
            })
        })
}

/// Judge a finished run from its logs and output directories. The solver's exit status takes no
/// part in it.
pub fn judge_results(results: &Path, r: &ResolvedStudy, mode: RunMode) -> EvidenceReport {
    let (block, mesh, solver, times) = read_evidence(results);
    let evidence = RunEvidence {
        block_mesh_log: &block,
        check_mesh_log: &mesh,
        solver_log: &solver,
        time_directories: &times,
    };
    let cells = r
        .mesh
        .cells_x
        .checked_mul(r.mesh.cells_y)
        .and_then(|x| x.checked_mul(r.mesh.cells_z));
    let volume = r.domain.length_x * r.domain.length_y * r.domain.length_z;
    let mut reasons = Vec::new();
    let verdict = match cells {
        None => {
            reasons.push("the study's cell count overflows".to_owned());
            None
        }
        Some(cell_count) if mode == RunMode::Smoke => Some(judge_smoke(
            &evidence,
            &SmokeExpectations {
                end_time: r.solver.end_time,
                cell_count,
                domain_volume: volume,
            },
        )),
        Some(cell_count) => Some(judge_run(
            &evidence,
            &EvidenceExpectations {
                end_time: r.solver.end_time,
                cell_count,
                domain_volume: volume,
                p_threshold: r.solver.convergence.p,
                u_threshold: r.solver.convergence.u,
            },
        )),
    };
    if let Some(Verdict::Failed(found)) = &verdict {
        reasons.extend(found.iter().cloned());
    }
    if !final_fields_present(results, r.solver.end_time) {
        reasons.push("the final-time U and p field files are missing or empty".to_owned());
    }
    let verdict = if !reasons.is_empty() {
        "failed"
    } else if mode == RunMode::Smoke {
        "smoke_passed"
    } else {
        "passed"
    };
    EvidenceReport {
        verdict: verdict.to_owned(),
        reasons,
    }
}

/// Judge a run that reached its end on the evidence that it finished: every log ends, nothing is
/// fatal, the final time was reached, the mesh is good, the residuals are well formed and the final
/// fields are there. There is no residual limit. Whether the run converged is a separate verdict.
/// The verdict word is `passed` or `failed`.
pub fn judge_finished_results(results: &Path, r: &ResolvedStudy) -> EvidenceReport {
    let mut report = judge_results(results, r, RunMode::Smoke);
    if report.verdict == "smoke_passed" {
        report.verdict = "passed".to_owned();
    }
    report
}

/// Judge a run that was stopped on request at `time`: every log ends, nothing is fatal, the mesh is
/// good, the residuals are well formed, the last logged time is the newest time folder, and that
/// folder holds a non-empty `U` and `p`. There is no residual limit and no end time to reach. The
/// verdict word is `passed` or `failed`.
pub fn judge_stopped_results(results: &Path, r: &ResolvedStudy, time: &str) -> EvidenceReport {
    let (block, mesh, solver, times) = read_evidence(results);
    let evidence = RunEvidence {
        block_mesh_log: &block,
        check_mesh_log: &mesh,
        solver_log: &solver,
        time_directories: &times,
    };
    let cells = r
        .mesh
        .cells_x
        .checked_mul(r.mesh.cells_y)
        .and_then(|x| x.checked_mul(r.mesh.cells_z));
    let mut reasons = Vec::new();
    match cells {
        None => reasons.push("the study's cell count overflows".to_owned()),
        Some(cell_count) => {
            let expected = PartialExpectations {
                end_time: r.solver.end_time,
                cell_count,
                domain_volume: r.domain.length_x * r.domain.length_y * r.domain.length_z,
            };
            if let Verdict::Failed(found) = judge_partial(&evidence, &expected) {
                reasons.extend(found);
            }
        }
    }
    // The time becomes part of a path, so only a plain decimal number is used as one.
    let plain = !time.is_empty()
        && time.bytes().all(|c| c.is_ascii_digit() || c == b'.')
        && time.bytes().any(|c| c.is_ascii_digit());
    let filled = |name: &str| {
        plain
            && std::fs::metadata(results.join(time).join(name))
                .is_ok_and(|m| m.is_file() && m.len() > 0)
    };
    if !(filled("U") && filled("p")) {
        reasons.push(format!(
            "the U and p field files of the stopped time {time} are missing or empty"
        ));
    }
    EvidenceReport {
        verdict: if reasons.is_empty() {
            "passed"
        } else {
            "failed"
        }
        .to_owned(),
        reasons,
    }
}

/// The long part of a run: upload the case, run it, download the results, judge them. The report
/// is also written to `<runs_dir>/<run id>/report.json`.
pub fn execute(begun: Begun) -> Finished {
    let Begun {
        job,
        prepared,
        mode,
        case,
        results,
        run_dir,
        table,
        ..
    } = begun;
    let operation = mode.operation();
    let run_id = job.run_id().to_owned();
    let outcome = RunSpec::allrun(dialect::IMAGE)
        .map_err(RunError::Setup)
        .and_then(|spec| job.execute(&case, &results, &spec));
    let mut done = match outcome {
        Err(e) => infra_finished(operation, &e, Some(&prepared), Some(&run_id)),
        Ok(outcome) => {
            let evidence = judge_results(&outcome.results_dir, &prepared.resolved, mode);
            let mut report = Report::from_diagnostics(
                operation,
                prepared.warnings.clone(),
                Some(prepared.hash.clone()),
            )
            .with_migration(prepared.migration.clone());
            let mut passed = evidence.verdict != "failed";
            if let (true, RunMode::Validate, Some(table)) = (passed, mode, &table) {
                let gate = ghia::compare_results(&outcome.results_dir, &prepared.resolved, table);
                passed = gate.passed;
                report.gate = Some(gate);
            }
            report.run = Some(RunInfo {
                run_id: run_id.clone(),
                container: outcome.container_name,
                duration_seconds: outcome.duration_seconds,
                process_exit: outcome.exit_code,
            });
            report.evidence = Some(evidence);
            report.set_class(if passed {
                ExitClass::Passed
            } else {
                ExitClass::GateFailed
            });
            finished(report)
        }
    };
    if let Some(problem) = persist(&run_dir, &done.report) {
        done.detail = Some(problem);
    }
    done
}

/// A whole run in one call.
pub fn run_study(study_json: &str, request: &RunRequest) -> Finished {
    match begin(study_json, request) {
        Ok(begun) => execute(begun),
        Err(done) => done,
    }
}

#[cfg(test)]
mod persist_tests {
    use super::*;

    fn fresh(tag: &str) -> std::path::PathBuf {
        // Beside the test binary, inside the build folder (a lib test has no CARGO_TARGET_TMPDIR).
        let dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join(format!("persist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn persist_replaces_the_report_and_leaves_nothing_else() {
        let dir = fresh("replace");
        let (a, b) = (Report::new("first"), Report::new("second"));
        assert_eq!(persist(&dir, &a), None);
        assert_eq!(persist(&dir, &b), None);
        let saved = std::fs::read_to_string(dir.join("report.json")).unwrap();
        assert_eq!(saved, b.to_json());
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["report.json"]);
    }

    #[test]
    fn persist_reports_a_run_folder_that_is_a_plain_file() {
        let dir = fresh("occupied");
        std::fs::write(&dir, b"x").unwrap();
        let text = persist(&dir, &Report::new("run")).expect("a failure is reported");
        assert!(text.starts_with("the report could not be saved"), "{text}");
    }
}
