//! A run that goes wrong protects itself and the shared machine: a diverging run stops itself, a
//! solver error is explained with a fix, a run is not started below the disk floor and stops
//! before it would cross it, and only the time steps it needs are kept. Nothing a person is shown
//! names the host.

use std::fmt::Write as _;

use cfdcraft_drain::disk::{BELOW_FLOOR_CODE, KB_PER_GB};
use cfdcraft_logscan::ErrorId;
use cfdcraft_queue::{JobState, StopCause, StopMode, StopRequest};
use cfdcraft_remote::ContainerState;
use cfdcraft_text::{Line, Text};

mod support;
use support::*;

#[path = "support/acceptance.rs"]
mod acceptance;
use acceptance::*;

const MISSING_FILE_LOG: &str =
    include_str!("../../cfdcraft-backend-esi/tests/fixtures/of14-snappyHexMesh.log");

const NO_WRITE: StopRequest = StopRequest {
    mode: StopMode::NoWrite,
    cause: StopCause::Diverged,
};

const WRITE_NOW_FOR_DISK: StopRequest = StopRequest {
    mode: StopMode::WriteNow,
    cause: StopCause::Disk,
};

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

/// The sentence a person is shown for `id`.
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

/// A run whose residuals climb until the drain stops it, ended: the world, the job and the run.
fn diverged(tag: &str) -> (World, String, String) {
    let (w, mut drain, id, run_id) = started_job(tag);
    w.shared.grow_log(&run_id, 1, 50);
    drain.tick().unwrap();
    assert_eq!(w.job(&id).stop, None, "a healthy log asks for nothing");

    let climb = [1e-3, 1e-1, 1e1, 1e3, 5e3];
    w.shared
        .append(&run_id, "log.simpleFoam", &escalating(51, &climb));
    drain.tick().unwrap();
    assert_eq!(
        w.job(&id).stop,
        Some(NO_WRITE),
        "the run asked itself to stop"
    );

    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed: false,
        },
    );
    drain.tick().unwrap();
    drain.tick().unwrap();
    (w, id, run_id)
}

/// A run that ends with a known solver error, ended: the world and the job.
fn explained(tag: &str) -> (World, String) {
    let (w, id, _run) = fail_with_log(tag, MISSING_FILE_LOG, false);
    (w, id)
}

/// A job that was asked to start on a host with 10 GB free: the world and the job.
fn refused_for_disk(tag: &str) -> (World, String) {
    let w = world(tag);
    let id = w.add();
    w.shared.inner.borrow_mut().set_free_kb(10 * KB_PER_GB);
    w.drain().tick().unwrap();
    (w, id)
}

/// A run on a host whose free disk falls to 19 GB while it runs: the world and the job, ended.
fn stopped_for_disk(tag: &str) -> (World, String) {
    let (w, mut drain, id, run_id) = started_job(tag);
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    put_partial(&w, &run_id, 1200, 16384);
    w.shared.inner.borrow_mut().set_free_kb(19 * KB_PER_GB);
    drain.tick().unwrap();
    assert_eq!(
        w.job(&id).stop,
        Some(WRITE_NOW_FOR_DISK),
        "the run was asked to write now before it crossed the floor"
    );
    assert_eq!(settle(&w, &mut drain, &id, 40), JobState::Stopped);
    (w, id)
}

#[test]
fn a_diverging_run_stops_itself() {
    // Criterion 4: a diverging run stops itself.
    let (w, id, run_id) = diverged("sc4-diverged");
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some("CFD-RUN-004"));
    assert!(w.runs().is_empty(), "nothing was recorded");
    assert!(w.shared.holder().is_none(), "the lock was given back");
    assert_eq!(
        w.shared.container(&run_id),
        ContainerState::None,
        "the container was removed"
    );
    let report = report_of(&w, &id);
    assert_eq!(report.lines.len(), 1, "{:?}", report.lines);
    let line = &report.lines[0];
    assert!(
        line.starts_with("The run diverged (CFD-RUN-004): Ux reached "),
        "{line}"
    );
    assert!(line.contains(", over the limit "), "{line}");
    assert!(
        line.ends_with("It was stopped without writing the unstable fields."),
        "{line}"
    );
}

#[test]
fn each_known_solver_error_is_explained_with_a_fix() {
    // Criterion 4: a solver error is explained in plain words, with a fix.
    for (n, id) in ALL.into_iter().enumerate() {
        if id == ErrorId::Unknown {
            continue;
        }
        let tail = tail_for(id);
        let log = format!("{}{tail}", cavity_log(1, 3));
        let (w, job, _run) = fail_with_log(&format!("sc4-known-{n}"), &log, false);
        assert_eq!(w.state(&job), JobState::Failed, "{id:?}");
        assert_eq!(w.job(&job).code.as_deref(), Some(id.code()), "{id:?}");
        assert!(w.runs().is_empty(), "{id:?}: nothing was recorded");
        let report = report_of(&w, &job);
        let expected = sentence_for(id, tail);
        assert_eq!(report.lines, std::slice::from_ref(&expected), "{id:?}");
        assert!(expected.contains(id.code()), "{expected}");
        assert!(
            expected.ends_with('.'),
            "a sentence that says what to do: {expected}"
        );
    }
    let codes: Vec<&str> = ALL.iter().map(|id| id.code()).collect();
    assert_eq!(codes[0], "CFD-SOLVER-001");
    assert_eq!(codes[10], "CFD-SOLVER-011");

    // A real log from the solver maps to its code too.
    let (w, job, _run) = fail_with_log("sc4-real", MISSING_FILE_LOG, false);
    assert_eq!(w.job(&job).code.as_deref(), Some("CFD-SOLVER-001"));

    // A container that ran out of memory is explained from the container state.
    let (w, job, _run) = fail_with_log("sc4-oom", &cavity_log(1, 5), true);
    assert_eq!(w.job(&job).code.as_deref(), Some("CFD-SOLVER-007"));
}

#[test]
fn an_unknown_solver_error_shows_the_redacted_last_lines() {
    // Criterion 4: an error nobody wrote down shows the end of the log, with the host hidden.
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
            _ => {
                let _ = writeln!(log, "late-{n} something odd");
            }
        }
    }
    let (w, job, run_id) = fail_with_log("sc4-unknown", &log, false);
    assert_eq!(w.state(&job), JobState::Failed);
    assert_eq!(w.job(&job).code.as_deref(), Some("CFD-SOLVER-099"));
    let report = report_of(&w, &job);
    let text = &report.lines[0];
    assert!(text.contains("(CFD-SOLVER-099)"), "{text}");
    assert!(text.contains("late-20 something odd"), "{text}");
    assert!(!text.contains("early-"), "only the last 20 lines: {text}");
    for hidden in ["/home/dt", ".ssh", "cfdcraft-cavity", run_id.as_str()] {
        assert!(!text.contains(hidden), "{hidden} is in {text}");
    }
}

#[test]
fn a_run_does_not_start_below_the_disk_floor() {
    // Criterion 4: a run never takes the disk below the floor, so it is not started below it.
    let (w, id) = refused_for_disk("sc4-below-floor");
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some(BELOW_FLOOR_CODE));
    assert_eq!(BELOW_FLOOR_CODE, "CFD-DISK-003");
    assert!(start_calls(&w).is_empty(), "{:?}", w.shared.calls());
    assert!(w.shared.holder().is_none(), "no lock was taken");
    assert_eq!(
        report_of(&w, &id).lines,
        ["The compute host has 10 GB free, below its floor of 20 GB, so the run was not started."]
    );
}

#[test]
fn a_run_stops_before_the_disk_falls_below_the_floor_and_keeps_its_latest_time() {
    // Criterion 4: a run stops before it would take the disk below the floor, and keeps the time
    // steps it has written.
    let (w, id) = stopped_for_disk("sc4-disk-stop");
    let report = report_of(&w, &id);
    assert!(
        report
            .lines
            .iter()
            .any(|l| l.contains("fell to 19 GB, the floor being 20 GB")),
        "{:?}",
        report.lines
    );
    let runs = w.runs();
    assert_eq!(runs.len(), 1, "the stopped run is recorded");
    assert!(runs[0].result.is_none());
    assert_eq!(
        runs[0].checkpoint.as_ref().map(|c| c.time.as_str()),
        Some("1200")
    );
    assert!(w.shared.holder().is_none(), "the host was given back");
}

#[test]
fn only_the_needed_time_steps_are_kept() {
    // Criterion 4: the case keeps the latest two time steps and the monitors keep everything.
    let (w, mut drain, id, run_id) = started_job("sc4-purge");
    let control = control_of(&w, &run_id);
    assert!(control.contains("purgeWrite 2;"), "{control}");
    assert!(control.contains("runTimeModifiable true;"), "{control}");
    assert_eq!(control.matches("purgeWrite").count(), 1);
    // Purging is a setting of the times the solver writes, not of the monitor objects below it.
    let (_, monitors) = control
        .split_once("functions")
        .expect("the case lists its monitors");
    assert!(monitors.contains("probe_1"));
    assert!(!monitors.contains("purgeWrite"), "{monitors}");

    // Whatever the solver purges, the monitor files of the run are kept.
    play_on(&w, &run_id);
    assert_eq!(settle(&w, &mut drain, &id, 8), JobState::Finished);
    for probe in 1..=5 {
        let rel = format!("postProcessing/probe_{probe}/0/U");
        assert!(w.live(&id).join(&rel).is_file(), "{rel}");
    }
}

#[test]
fn the_failure_reports_name_no_host_text() {
    // Criterion 4: what a person is told never names the host, its directories or the run name.
    let (w, id, _run) = diverged("sc4-host-diverged");
    let ended = [
        (w, id),
        explained("sc4-host-explained"),
        refused_for_disk("sc4-host-floor"),
        stopped_for_disk("sc4-host-disk"),
    ];
    for (w, id) in &ended {
        let text = report_text(w, id);
        let work_dir = w.dir.to_str().unwrap();
        let mut hidden = vec![work_dir, "ssh", "cfdcraft-cavity"];
        let run = w.job(id).run_id;
        if let Some(run) = run.as_deref() {
            hidden.push(run);
        }
        for word in hidden {
            assert!(!text.contains(word), "{word} is in {text}");
        }
    }
}
