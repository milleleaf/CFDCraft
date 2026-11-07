//! A run that ends badly says why in plain words: a known solver error gets its code and a fix,
//! an unknown one shows the redacted last lines, and no report names the host.

use std::fmt::Write as _;
use std::fs;

use cfdcraft_drain::explain::{self, Classified};
use cfdcraft_drain::finalize::{JobReport, ReportOutcome};
use cfdcraft_drain::{Drain, TickOutcome};
use cfdcraft_logscan::ErrorId;
use cfdcraft_queue::JobState;
use cfdcraft_remote::ContainerState;
use cfdcraft_text::{Line, Text};

mod support;
use support::*;

const MISSING_FILE_LOG: &str =
    include_str!("../../cfdcraft-backend-esi/tests/fixtures/of14-snappyHexMesh.log");

const ALL: [ErrorId; 12] = [
    ErrorId::MissingFile,
    ErrorId::NumericalBlowup,
    ErrorId::UnknownType,
    ErrorId::MissingKeyword,
    ErrorId::LinearSolverNotConverged,
    ErrorId::PoorMesh,
    ErrorId::OutOfMemory,
    ErrorId::DiskFull,
    ErrorId::DecompositionMismatch,
    ErrorId::NotEnoughSlots,
    ErrorId::SegmentationFault,
    ErrorId::Unknown,
];

/// The end of a log that shows `id`. Matching on every variant makes a new one a compile error
/// here. The numerical case avoids the words that the divergence rules read as a trap: a trap is
/// a divergence, not a solver error.
fn tail_for(id: ErrorId) -> &'static str {
    match id {
        ErrorId::MissingFile => {
            "--> FOAM FATAL IO ERROR:\nCould not open \"/w/system/fvSchemes\"\n\nFOAM exiting\n"
        }
        ErrorId::NumericalBlowup => "#0  sigFpe handler entered in the pressure solve\n",
        ErrorId::UnknownType => {
            "--> FOAM FATAL IO ERROR:\nUnknown patchField type fancyWall for patch lid\n"
        }
        ErrorId::MissingKeyword => {
            "--> FOAM FATAL IO ERROR:\nkeyword relaxationFactors is undefined in dictionary \"system/fvSolution\"\n"
        }
        ErrorId::LinearSolverNotConverged => {
            "--> FOAM FATAL ERROR:\nMaximum number of iterations exceeded\n"
        }
        ErrorId::PoorMesh => "Failed 1 mesh checks.\n",
        ErrorId::OutOfMemory => "terminate called after throwing an instance of 'std::bad_alloc'\n",
        ErrorId::DiskFull => "No space left on device\n",
        ErrorId::DecompositionMismatch => "Wrong number of processor directories\n",
        ErrorId::NotEnoughSlots => "There are not enough slots available in the system\n",
        ErrorId::SegmentationFault => "Segmentation fault (core dumped)\n",
        ErrorId::Unknown => "the solver stopped for a reason nobody wrote down\n",
    }
}

/// The sentence a person is shown for `id`, built the way the explainer is meant to.
fn sentence_for(id: ErrorId, tail: &str) -> String {
    let code = id.code().to_owned();
    let line = match id {
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
        ErrorId::Unknown => Line::SolverUnknown {
            tail: tail.to_owned(),
        },
    };
    Text::english().render(&line)
}

fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    let run_id = w.run_id(&id);
    (w, drain, id, run_id)
}

fn report_text(w: &World, id: &str) -> String {
    fs::read_to_string(w.live(id).join("report.json")).expect("a report was written")
}

fn report_of(w: &World, id: &str) -> JobReport {
    serde_json::from_str(&report_text(w, id)).expect("the report reads")
}

/// A started job whose solver log becomes `log` and whose container then exits; the drain takes
/// the two ticks that finish it.
fn ended_with(tag: &str, log: &str, oom_killed: bool) -> (World, Drain<Shared>, String, String) {
    let (w, mut drain, id, run_id) = started(tag);
    w.shared.append(&run_id, "log.simpleFoam", log);
    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed,
        },
    );
    drain.tick().unwrap();
    drain.tick().unwrap();
    (w, drain, id, run_id)
}

fn assert_failed_with(w: &World, id: &str, code: &str) -> JobReport {
    let job = w.job(id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some(code));
    assert!(w.runs().is_empty(), "nothing was recorded");
    assert!(w.shared.holder().is_none(), "the lock was given back");
    let report = report_of(w, id);
    assert_eq!(report.outcome, ReportOutcome::Failed);
    assert_eq!(report.code.as_deref(), Some(code));
    report
}

#[test]
fn each_known_error_ends_failed_with_its_code_and_sentence() {
    for (n, id) in ALL.into_iter().enumerate() {
        if id == ErrorId::Unknown {
            continue;
        }
        let tail = tail_for(id);
        let log = format!("{}{tail}", cavity_log(1, 3));
        let (w, _drain, job, _run) = ended_with(&format!("explain-known-{n}"), &log, false);
        let report = assert_failed_with(&w, &job, id.code());
        let expected = sentence_for(id, tail);
        assert_eq!(report.lines, std::slice::from_ref(&expected), "{id:?}");
        assert!(expected.contains(id.code()), "{expected}");
        assert!(
            expected.ends_with('.'),
            "a sentence that ends with what to do: {expected}"
        );
    }
}

#[test]
fn the_real_missing_file_log_maps_to_cfd_solver_001() {
    let (w, _drain, id, _run) = ended_with("explain-real", MISSING_FILE_LOG, false);
    let report = assert_failed_with(&w, &id, "CFD-SOLVER-001");
    assert_eq!(report.lines, [sentence_for(ErrorId::MissingFile, "")]);
}

#[test]
fn an_unknown_error_shows_a_redacted_tail() {
    let mut log = String::new();
    for n in 1..=5 {
        let _ = writeln!(log, "early-{n} first lines are not shown");
    }
    for n in 1..=20 {
        match n {
            3 => {
                let _ = writeln!(log, "late-3 reading /home/dt/.ssh/key from the host");
            }
            4 => {
                let _ = writeln!(log, "late-4 container cfdcraft-cavity-abc123def went away");
            }
            5 => {
                let _ = writeln!(log, "late-5 id 0123456789abcdef0123 was lost");
            }
            _ => {
                let _ = writeln!(log, "late-{n} something odd");
            }
        }
    }
    let (w, _drain, id, run_id) = ended_with("explain-unknown", &log, false);
    let report = assert_failed_with(&w, &id, "CFD-SOLVER-099");
    let text = &report.lines[0];
    assert!(text.contains("(CFD-SOLVER-099)"), "{text}");
    assert!(text.contains("late-20 something odd"), "{text}");
    assert!(text.contains("late-1 something odd"), "{text}");
    assert!(!text.contains("early-"), "only the last 20 lines: {text}");
    for hidden in [
        "/home/dt",
        ".ssh",
        "cfdcraft-cavity",
        "0123456789abcdef0123",
        run_id.as_str(),
    ] {
        assert!(!text.contains(hidden), "{hidden} is in {text}");
    }
    assert!(text.contains("<path>"), "{text}");
}

#[test]
fn a_log_that_ends_normally_is_clean() {
    let (w, mut drain, id, run_id) = started("explain-clean");
    w.play(&run_id);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finishing);
    let job = w.job(&id);
    assert_eq!(
        explain::classify(&mut drain, &job).unwrap(),
        Classified::Clean
    );
    drain.tick().unwrap();
    assert_eq!(
        w.state(&id),
        JobState::Finished,
        "the happy path is unchanged"
    );
}

#[test]
fn a_container_that_died_without_a_known_message_is_unknown_not_clean() {
    let (w, _drain, id, _run) = ended_with("explain-silent", &cavity_log(1, 5), false);
    let report = assert_failed_with(&w, &id, "CFD-SOLVER-099");
    assert!(
        report.lines[0].contains("Solving for p"),
        "{}",
        report.lines[0]
    );
}

#[test]
fn an_out_of_memory_exit_is_explained_from_the_container_state() {
    let (w, _drain, id, _run) = ended_with("explain-oom", &cavity_log(1, 5), true);
    let report = assert_failed_with(&w, &id, "CFD-SOLVER-007");
    assert_eq!(report.lines, [sentence_for(ErrorId::OutOfMemory, "")]);
}

/// A started job whose log stops without an end and whose container exits with `code`.
fn exited_with(tag: &str, code: i32, oom_killed: bool) -> (World, Drain<Shared>, String, String) {
    let (w, mut drain, id, run_id) = started(tag);
    w.shared
        .append(&run_id, "log.simpleFoam", &cavity_log(1, 5));
    w.shared
        .set_container(&run_id, ContainerState::Exited { code, oom_killed });
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finishing);
    (w, drain, id, run_id)
}

#[test]
fn a_run_ended_at_the_time_limit_says_so_instead_of_an_unknown_error() {
    for (n, code) in [124, 137].into_iter().enumerate() {
        let (w, mut drain, id, _run) = exited_with(&format!("explain-limit-{n}"), code, false);
        drain.tick().unwrap();
        let report = assert_failed_with(&w, &id, "CFD-RUN-009");
        assert_eq!(report.lines.len(), 1);
        let text = &report.lines[0];
        assert!(text.contains(&format!("exit status {code}")), "{text}");
        assert!(text.contains("time limit"), "{text}");
        assert!(text.contains("cannot be continued"), "{text}");
        assert!(!text.contains("CFD-SOLVER-099"), "{text}");
    }
}

#[test]
fn the_exit_status_is_read_again_after_a_restart() {
    let (w, drain, id, _run) = exited_with("explain-limit-restart", 124, false);
    drop(drain);
    let mut drain = w.drain();
    assert_eq!(
        drain.scratch().exit_code,
        None,
        "a new process knows nothing"
    );
    drain.tick().unwrap();
    assert_failed_with(&w, &id, "CFD-RUN-009");
}

#[test]
fn an_out_of_memory_kill_is_not_taken_for_the_time_limit() {
    let (w, mut drain, id, _run) = exited_with("explain-limit-oom", 137, true);
    drain.tick().unwrap();
    assert_failed_with(&w, &id, "CFD-SOLVER-007");
}

#[test]
fn the_failed_report_names_no_host_text() {
    let logs = [
        MISSING_FILE_LOG.to_owned(),
        "reading /home/dt/.ssh/key in /opt/openfoam14/etc\n".to_owned(),
    ];
    for (n, log) in logs.iter().enumerate() {
        let (w, _drain, id, run_id) = ended_with(&format!("explain-host-{n}"), log, false);
        let text = report_text(&w, &id);
        let work_dir = w.dir.to_str().unwrap();
        for hidden in [work_dir, run_id.as_str(), "ssh", "@", "/opt/", "/home/"] {
            assert!(!text.contains(hidden), "{hidden} is in {text}");
        }
        let rendered: JobReport = serde_json::from_str(&text).unwrap();
        for line in &rendered.lines {
            assert!(!line.contains("ssh") && !line.contains('@'), "{line}");
        }
    }
}
