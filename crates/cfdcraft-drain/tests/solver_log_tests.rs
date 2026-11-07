//! A job whose live specification names another solver log is mirrored, watched, stopped and
//! explained from that log, and a specification that names none still means the cavity's. The
//! pretend solver writes the pimple log under its own name; the cavity-named copy is only what the
//! results reader finds in the downloaded folder.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_drain::finalize::{JobReport, ReportOutcome};
use cfdcraft_drain::watch::solver_log_of;
use cfdcraft_drain::{Drain, LiveSpec, TickOutcome, watchdog};
use cfdcraft_logscan::ErrorId;
use cfdcraft_ops::queue_ops::queue_stop;
use cfdcraft_queue::{JobState, StopCause, StopMode, StopRequest};
use cfdcraft_remote::fake::FakeBox;
use cfdcraft_remote::{ContainerState, Host};
use cfdcraft_text::{Line, Text};
use serde_json::{Value, json};

mod support;
use support::*;

const LOG: &str = "log.pimpleFoam";
const CAVITY_LOG: &str = "log.simpleFoam";
const CONTROL: &str = "system/controlDict";

const NO_WRITE: StopRequest = StopRequest {
    mode: StopMode::NoWrite,
    cause: StopCause::Diverged,
};

/// Changes the specification file of a job's live folder, as JSON.
fn edit_spec(live: &Path, change: impl FnOnce(&mut Value)) {
    let path = live.join("spec.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, value.to_string()).unwrap();
}

/// A world on a host whose solver writes `log.pimpleFoam`, with one job that has started and whose
/// live specification names that log: the world, its drain, the job id and the run name.
fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    *w.shared.inner.borrow_mut() = FakeBox::with_solver_log(LOG);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    assert_eq!(w.state(&id), JobState::Running);
    edit_spec(&w.live(&id), |value| {
        value["spec"]["log_file"] = Value::from(LOG);
    });
    let run_id = w.run_id(&id);
    (w, drain, id, run_id)
}

fn stop_of(w: &World, id: &str) -> Option<StopRequest> {
    w.job(id).stop
}

fn report_of(w: &World, id: &str) -> JobReport {
    let text = fs::read_to_string(w.live(id).join("report.json")).expect("a report was written");
    serde_json::from_str(&text).expect("the report reads")
}

/// Iterations from `from` on, each with one Ux solve whose initial residual is the next value.
fn ux_lines(from: u64, residuals: &[f64]) -> String {
    let mut text = String::new();
    for (offset, r) in residuals.iter().enumerate() {
        let _ = writeln!(text, "Time = {}\n", from + offset as u64);
        let _ = writeln!(
            text,
            "smoothSolver:  Solving for Ux, Initial residual = {r:.6e}, Final residual = 1e-9, No Iterations 1\n"
        );
    }
    text
}

fn sentence(line: &Line) -> String {
    Text::english().render(line)
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

/// A started job that gets the given files from the pretend solver and whose container then exits
/// with an error; the drain takes the two ticks that finish it.
fn ended_with(tag: &str, files: &[(&str, &str)]) -> (World, String) {
    let (w, mut drain, id, run_id) = started(tag);
    for (rel, text) in files {
        w.shared.append(&run_id, rel, text);
    }
    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed: false,
        },
    );
    drain.tick().unwrap();
    drain.tick().unwrap();
    (w, id)
}

#[test]
fn a_pimple_job_finishes_and_is_recorded() {
    let (w, mut drain, id, run_id) = started("solver-log-finish");
    w.play(&run_id);
    let cavity = w
        .shared
        .file(&run_id, CAVITY_LOG)
        .expect("the played solver wrote its log");
    w.shared
        .append(&run_id, LOG, &String::from_utf8(cavity).unwrap());

    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finishing);
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finished);

    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].result.is_some());
    assert!(runs[0].verdict.is_some(), "the verdict is recorded");
    assert!(w.shared.holder().is_none(), "the lock was given back");
    let live = w.live(&id);
    assert!(live.join(LOG).is_file(), "the job's own log was mirrored");
    assert!(
        !live.join(CAVITY_LOG).exists(),
        "the cavity's log name is not this job's"
    );
}

#[test]
fn the_log_list_comes_from_the_spec() {
    let (w, mut drain, id, run_id) = started("solver-log-list");
    let live = w.live(&id);
    w.shared.append(&run_id, CAVITY_LOG, &cavity_log(1, 2));
    w.shared
        .append(&run_id, "log.blockMesh", "Create mesh\nEnd\n");
    w.shared
        .append(&run_id, LOG, &format!("{}End\n", cavity_log(1, 2)));
    drain.tick().unwrap();
    assert!(live.join(LOG).is_file());
    assert!(live.join("log.blockMesh").is_file());
    assert!(!live.join(CAVITY_LOG).exists());

    // A mesh log that has ended is not read again; the job's own log is, even after an end line.
    w.shared
        .append(&run_id, "log.blockMesh", "more mesh text\n");
    w.shared.append(&run_id, LOG, "Time = 3\n\n");
    drain.tick().unwrap();
    let mesh = fs::read_to_string(live.join("log.blockMesh")).unwrap();
    assert!(!mesh.contains("more mesh text"), "{mesh}");
    let solver = fs::read_to_string(live.join(LOG)).unwrap();
    assert!(solver.contains("Time = 3"), "{solver}");
}

#[test]
fn an_old_spec_means_the_cavity_log() {
    let (w, _drain, id, _run) = started("solver-log-old");
    let live = w.live(&id);
    assert_eq!(solver_log_of(&live), LOG);

    edit_spec(&live, |value| {
        let spec = value["spec"].as_object_mut().expect("a spec object");
        for key in [
            "log_file",
            "pressure_field",
            "temperature",
            "periodic",
            "forces",
            "heat",
        ] {
            spec.remove(key);
        }
    });
    let text = fs::read_to_string(live.join("spec.json")).unwrap();
    let old: LiveSpec = serde_json::from_str(&text).expect("the old shape still reads");
    assert_eq!(old.spec.log_file, CAVITY_LOG);
    assert_eq!(solver_log_of(&live), CAVITY_LOG);

    let bare = w.dir.join("bare-live");
    fs::create_dir_all(&bare).unwrap();
    assert_eq!(solver_log_of(&bare), CAVITY_LOG, "no spec file");
    assert_eq!(solver_log_of(&w.dir.join("absent")), CAVITY_LOG);
    fs::write(bare.join("spec.json"), "{ not json").unwrap();
    assert_eq!(solver_log_of(&bare), CAVITY_LOG, "an unreadable spec");
    fs::write(
        bare.join("spec.json"),
        json!({"spec": {"log_file": 7}}).to_string(),
    )
    .unwrap();
    assert_eq!(solver_log_of(&bare), CAVITY_LOG, "a name that is no text");
    fs::write(
        bare.join("spec.json"),
        json!({"spec": {"log_file": LOG}}).to_string(),
    )
    .unwrap();
    assert_eq!(solver_log_of(&bare), LOG);
}

#[test]
fn a_damaged_spec_cannot_name_a_path() {
    let w = world("solver-log-hostile");
    let bare = w.dir.join("hostile-live");
    fs::create_dir_all(&bare).unwrap();
    for name in ["", "../x", "/etc/passwd", "a/b", "a\\b", "..", ".", "log\n"] {
        let text = json!({"spec": {"log_file": name}}).to_string();
        fs::write(bare.join("spec.json"), text).unwrap();
        assert_eq!(solver_log_of(&bare), CAVITY_LOG, "{name:?}");
    }
}

#[test]
fn a_cavity_job_still_uses_its_own_log() {
    let w = world("solver-log-cavity");
    let id = w.add();
    let mut drain = w.drain();
    drain.tick().unwrap();
    let run_id = w.run_id(&id);
    assert_eq!(solver_log_of(&w.live(&id)), CAVITY_LOG);
    w.play(&run_id);
    drain.tick().unwrap();
    assert!(w.live(&id).join(CAVITY_LOG).is_file());
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finished);
}

#[test]
fn the_fake_host_can_name_its_solver_log() {
    const ID: &str = "cfdcraft-demo-1-2";
    for (mut host, name, other) in [
        (FakeBox::with_solver_log(LOG), LOG, CAVITY_LOG),
        (FakeBox::new(), CAVITY_LOG, LOG),
    ] {
        host.solver_obeys_stop(true);
        host.set_container(ID, ContainerState::Running);
        host.append(ID, name, "Time = 3\n");
        host.write_text(ID, CONTROL, "stopAt writeNow;\n").unwrap();
        let log = host.file(ID, name).expect("the named log exists");
        assert!(log.ends_with(b"Time = 3\nEnd\n"), "{name}");
        assert!(host.file(ID, other).is_none(), "{other} was not written");
        assert!(host.file(ID, "3/U").is_some());
        assert_eq!(
            host.inspect(ID).unwrap(),
            ContainerState::Exited {
                code: 0,
                oom_killed: false
            }
        );
    }
}

/// What a solver that is cut short at `time` has left in the run: the mesh, the logs, and the
/// fields of that time. The solver log does not end yet; it has its own name, and the cavity's name
/// holds a copy for the results reader.
fn put_partial(w: &World, run_id: &str, time: u64) {
    let dir = w.dir.join("partial");
    let _ = fs::remove_dir_all(&dir);
    let spec = FixtureSpec {
        end_time: time,
        cells: 16384,
        samples: 64,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    for rel in files_under(&dir) {
        let mut text = fs::read_to_string(dir.join(&rel)).unwrap();
        if rel == CAVITY_LOG {
            text = text.trim_end().strip_suffix("End").unwrap().to_owned();
            w.shared.replace(run_id, LOG, &text);
        }
        w.shared.replace(run_id, &rel, &text);
    }
}

/// Ticks until the job has ended, and returns its state.
fn settle(w: &World, drain: &mut Drain<Shared>, id: &str) -> JobState {
    for _ in 0..40 {
        drain.tick().unwrap();
        if w.state(id).is_terminal() {
            break;
        }
    }
    w.state(id)
}

#[test]
fn a_pimple_job_is_stopped_with_write_now() {
    let (w, mut drain, id, run_id) = started("solver-log-stop");
    w.shared.inner.borrow_mut().solver_obeys_stop(true);
    put_partial(&w, &run_id, 1200);
    passed(&queue_stop(&w.doc, &id).report);

    drain.tick().unwrap();
    assert_ne!(w.state(&id), JobState::Running, "the stop was carried out");
    let control = w.shared.file(&run_id, CONTROL).expect("the control file");
    assert!(
        String::from_utf8(control)
            .unwrap()
            .contains("stopAt writeNow;")
    );
    let log = w.shared.file(&run_id, LOG).expect("the job's log");
    assert!(log.ends_with(b"End\n"), "the solver ended its own log");
    // The results reader looks for the cavity's name in the downloaded folder.
    w.shared.append(&run_id, CAVITY_LOG, "End\n");

    assert_eq!(settle(&w, &mut drain, &id), JobState::Stopped);
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].result.is_none(), "a stopped run holds no result");
    assert_eq!(
        runs[0].checkpoint.as_ref().map(|c| c.time.as_str()),
        Some("1200")
    );
    assert!(w.shared.holder().is_none(), "the lock was given back");
    assert!(w.live(&id).join(LOG).is_file());
}

#[test]
fn the_watchdog_reads_the_jobs_log() {
    let (w, mut drain, id, run_id) = started("solver-log-watchdog");
    w.shared.append(&run_id, LOG, &cavity_log(1, 50));
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), None, "a clean log asks for nothing");

    let climb = [1e-3, 1e-1, 1e1, 1e3, 5e3];
    w.shared.append(&run_id, CAVITY_LOG, &ux_lines(51, &climb));
    drain.tick().unwrap();
    assert_eq!(
        stop_of(&w, &id),
        None,
        "a blow-up in another file is not this job's"
    );

    w.shared.append(&run_id, LOG, &ux_lines(51, &climb));
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), Some(NO_WRITE));

    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed: false,
        },
    );
    drain.tick().unwrap();
    drain.tick().unwrap();
    let report = assert_failed_with(&w, &id, "CFD-RUN-004");
    assert_eq!(
        report.lines,
        [
            "The run diverged (CFD-RUN-004): Ux reached 5000, over the limit 100. It was stopped without writing the unstable fields."
        ]
    );
    assert!(w.live(&id).join(LOG).is_file(), "the mirror is kept");
}

#[test]
fn the_courant_rule_reads_the_jobs_log() {
    let (w, mut drain, id, run_id) = started("solver-log-courant");
    w.shared.append(
        &run_id,
        LOG,
        "Time = 1\n\nCourant Number mean: 0.2 max: 80\n",
    );
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), None, "a steady run has no Courant rule");

    let job = w.job(&id);
    assert!(watchdog::find(&drain, &job, false).unwrap().is_none());
    let found = watchdog::find(&drain, &job, true)
        .unwrap()
        .expect("a rule fires");
    assert_eq!(found.code.code(), "CFD-RUN-005");
}

#[test]
fn the_explainer_reads_the_jobs_log() {
    let known =
        "--> FOAM FATAL IO ERROR:\nCould not open \"/w/system/fvSchemes\"\n\nFOAM exiting\n";
    let log = format!("{}{known}", cavity_log(1, 3));
    let other = "No space left on device\n";
    let (w, id) = ended_with(
        "solver-log-known",
        &[(LOG, log.as_str()), (CAVITY_LOG, other)],
    );
    let code = ErrorId::MissingFile.code();
    let report = assert_failed_with(&w, &id, code);
    let expected = sentence(&Line::SolverMissingFile {
        code: code.to_owned(),
    });
    assert_eq!(report.lines, [expected]);

    let odd = "the solver stopped for a reason nobody wrote down\n";
    let log = format!("{}{odd}", cavity_log(1, 3));
    let (w, id) = ended_with("solver-log-unknown", &[(LOG, log.as_str())]);
    let report = assert_failed_with(&w, &id, "CFD-SOLVER-099");
    let text = &report.lines[0];
    assert!(text.contains("nobody wrote down"), "{text}");

    // With no solver log the newest mesh log names the failure.
    let (w, id) = ended_with(
        "solver-log-mesh",
        &[("log.checkMesh", "Failed 1 mesh checks.\n")],
    );
    let code = ErrorId::PoorMesh.code();
    let report = assert_failed_with(&w, &id, code);
    let expected = sentence(&Line::SolverPoorMesh {
        code: code.to_owned(),
    });
    assert_eq!(report.lines, [expected]);
}

#[test]
fn the_explainer_log_order_is_solver_first() {
    let files = [
        ("log.blockMesh", "No space left on device\n"),
        ("log.checkMesh", "Failed 1 mesh checks.\n"),
    ];
    // The mesh check comes before the block mesh.
    let (w, id) = ended_with("solver-log-order-mesh", &files);
    assert_failed_with(&w, &id, ErrorId::PoorMesh.code());

    // The solver's own log comes before both.
    let mut with_solver = files.to_vec();
    with_solver.push((LOG, "Segmentation fault (core dumped)\n"));
    let (w, id) = ended_with("solver-log-order-solver", &with_solver);
    assert_failed_with(&w, &id, ErrorId::SegmentationFault.code());
}
