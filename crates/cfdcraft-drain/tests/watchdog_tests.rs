//! A run whose log goes wrong is asked to stop without writing, and ends failed with the code of
//! the rule it broke. A healthy run is never touched.

use std::fmt::Write as _;
use std::fs;

use cfdcraft_drain::finalize::{JobReport, ReportOutcome};
use cfdcraft_drain::watchdog;
use cfdcraft_drain::{Drain, TickOutcome};
use cfdcraft_queue::{Action, Job, JobState, StopCause, StopMode, StopRequest};
use cfdcraft_remote::ContainerState;

mod support;
use support::*;

const SIGFPE_CRASH: &str =
    include_str!("../../cfdcraft-backend-esi/tests/fixtures/of14-windaround-buildings.log");

const NO_WRITE: StopRequest = StopRequest {
    mode: StopMode::NoWrite,
    cause: StopCause::Diverged,
};

/// A world with one job that has been started: the world, its drain, the job id and the run name.
fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    let run_id = w.run_id(&id);
    (w, drain, id, run_id)
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

fn stop_of(w: &World, id: &str) -> Option<StopRequest> {
    w.job(id).stop
}

fn report_of(w: &World, id: &str) -> (JobReport, String) {
    let text = fs::read_to_string(w.live(id).join("report.json")).expect("a report was written");
    (serde_json::from_str(&text).expect("the report reads"), text)
}

/// The container ends, and the drain takes the two ticks that finish the job.
fn end_container(w: &World, drain: &mut Drain<Shared>, run_id: &str) {
    w.shared.set_container(
        run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed: false,
        },
    );
    drain.tick().unwrap();
    drain.tick().unwrap();
}

fn ended_failed(w: &World, id: &str, code: &str) -> JobReport {
    let job = w.job(id);
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.code.as_deref(), Some(code));
    assert!(w.runs().is_empty(), "nothing was recorded");
    assert!(w.shared.holder().is_none(), "the lock was given back");
    assert!(
        w.live(id).join("log.simpleFoam").is_file(),
        "the mirror is kept"
    );
    let (report, _) = report_of(w, id);
    assert_eq!(report.outcome, ReportOutcome::Failed);
    assert_eq!(report.code.as_deref(), Some(code));
    report
}

#[test]
fn an_escalating_log_asks_for_a_no_write_stop_and_ends_failed() {
    let (w, mut drain, id, run_id) = started("watchdog-escalating");
    w.shared.grow_log(&run_id, 1, 50);
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), None, "a healthy log asks for nothing");

    let climb = [1e-3, 1e-1, 1e1, 1e3, 5e3];
    w.shared
        .append(&run_id, "log.simpleFoam", &ux_lines(51, &climb));
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), Some(NO_WRITE));
    assert!(matches!(
        w.state(&id),
        JobState::Running | JobState::Stopping
    ));

    end_container(&w, &mut drain, &run_id);
    let report = ended_failed(&w, &id, "CFD-RUN-004");
    assert_eq!(
        report.lines,
        [
            "The run diverged (CFD-RUN-004): Ux reached 5000, over the limit 100. It was stopped without writing the unstable fields."
        ]
    );
    let (_, text) = report_of(&w, &id);
    for hidden in [run_id.as_str(), w.dir.to_str().unwrap(), ".ssh"] {
        assert!(!text.contains(hidden), "{hidden} is in {text}");
    }
}

#[test]
fn a_trap_ends_failed_with_the_trap_code() {
    let (w, mut drain, id, run_id) = started("watchdog-trap");
    w.shared.replace(&run_id, "log.simpleFoam", SIGFPE_CRASH);
    end_container(&w, &mut drain, &run_id);
    let report = ended_failed(&w, &id, "CFD-RUN-003");
    assert!(report.lines[0].starts_with("The run diverged (CFD-RUN-003)"));
}

#[test]
fn a_healthy_5000_iteration_run_never_trips() {
    let (w, mut drain, id, run_id) = started("watchdog-healthy");
    for from in (1..=5000).step_by(500) {
        w.shared.grow_log(&run_id, from, from + 499);
        drain.tick().unwrap();
        assert_eq!(stop_of(&w, &id), None, "tripped at iteration {from}");
        assert_eq!(w.state(&id), JobState::Running);
    }
}

#[test]
fn a_continued_run_with_a_tiny_first_residual_is_not_stopped() {
    let (w, mut drain, id, run_id) = started("watchdog-continued");
    w.shared
        .append(&run_id, "log.simpleFoam", &ux_lines(1, &[1e-9, 1e-3]));
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), None);
    let job = w.job(&id);
    assert!(
        watchdog::find(&drain, &job, false).unwrap().is_none(),
        "1e-3 is 1e6 times the first value but far under the absolute floor"
    );
}

#[test]
fn the_request_is_made_once() {
    let (w, mut drain, id, run_id) = started("watchdog-once");
    w.shared
        .append(&run_id, "log.simpleFoam", &ux_lines(1, &[1e-3, 5e3]));
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), Some(NO_WRITE));
    // the next tick applies the request (the job moves to stopping); after that nothing changes
    drain.tick().unwrap();
    let queue = w.queue();
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(w.queue(), queue, "later ticks change nothing");
}

#[test]
fn courant_counts_only_when_the_solver_is_transient() {
    let (w, mut drain, id, run_id) = started("watchdog-courant");
    w.shared.append(
        &run_id,
        "log.simpleFoam",
        "Time = 1\n\nCourant Number mean: 0.2 max: 60.5\n",
    );
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), None, "a steady run has no Courant rule");

    let job = w.job(&id);
    assert!(watchdog::find(&drain, &job, false).unwrap().is_none());
    let found = watchdog::find(&drain, &job, true)
        .unwrap()
        .expect("a rule fires");
    assert_eq!(found.code.code(), "CFD-RUN-005");

    watchdog::check_with(&mut drain, &job, true).unwrap();
    assert_eq!(stop_of(&w, &id), Some(NO_WRITE));
}

#[test]
fn growing_continuity_error_is_found() {
    let (w, mut drain, id, run_id) = started("watchdog-continuity");
    let mut text = String::new();
    for i in 0..20 {
        let local = if i < 10 { 1e-8 } else { 1e-2 };
        let _ = writeln!(
            text,
            "Time = {}\n\ntime step continuity errors : sum local = {local:e}, global = 1e-9, cumulative = 1e-9\n",
            i + 1
        );
    }
    w.shared.append(&run_id, "log.simpleFoam", &text);
    drain.tick().unwrap();
    assert_eq!(stop_of(&w, &id), Some(NO_WRITE));
    end_container(&w, &mut drain, &run_id);
    ended_failed(&w, &id, "CFD-RUN-006");
}

#[test]
fn the_stop_request_does_not_override_a_user_stop() {
    let (w, mut drain, id, run_id) = started("watchdog-user");
    let user = StopRequest {
        mode: StopMode::WriteNow,
        cause: StopCause::User,
    };
    drain
        .persist(Action::RequestStop {
            id: id.clone(),
            mode: user.mode,
            cause: user.cause,
        })
        .unwrap();
    w.shared
        .append(&run_id, "log.simpleFoam", &ux_lines(1, &[1e-3, 5e3]));
    drain.tick().unwrap();
    let job: Job = w.job(&id);
    assert_eq!(job.stop, Some(user), "the first request stays");
}
