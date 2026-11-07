//! A run whose log outgrew the live view still ends as it ended on the host.
//!
//! The live view of a job stops growing at its limit while the solver goes on, so the end of a long
//! log is then only on the host. A container that exited cleanly must not fail for a log that was
//! cut. The end of the log in the fixture is the real end of a fine-mesh cylinder run (32 000 steps,
//! 61 MB of log, 103 minutes of solver) that was judged failed with the unknown-error code although
//! the solver had written `End`.

use cfdcraft_drain::explain::{self, Classified};
use cfdcraft_drain::{Drain, TickOutcome};
use cfdcraft_queue::JobState;
use cfdcraft_remote::ContainerState;

mod support;
use support::*;

const END_OF_A_LONG_RUN: &str = include_str!("fixtures/pimplefoam-end-of-a-long-run.log");

/// One line of the filler that fills the live view.
const FILLER_LINE: &str = "Created 16384 cells\n";

/// A job that has started and whose first forty iterations were mirrored, with the live view then
/// filled to its limit by another file, so that nothing more of the solver log can be copied.
fn with_full_view(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    let run_id = w.run_id(&id);
    w.shared.grow_log(&run_id, 1, 40);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    // More than the limit of 64 MiB, in a file the solver log does not depend on.
    let filler = FILLER_LINE.repeat(4_000_000);
    std::fs::write(w.live(&id).join("log.blockMesh"), filler).unwrap();
    (w, drain, id, run_id)
}

#[test]
fn the_real_end_of_a_long_log_is_read_from_the_host_when_the_view_is_full() {
    let (w, mut drain, id, run_id) = with_full_view("long-log-classify");
    w.shared
        .append(&run_id, "log.simpleFoam", END_OF_A_LONG_RUN);
    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 0,
            oom_killed: false,
        },
    );
    let job = w.job(&id);
    assert_eq!(
        explain::classify(&mut drain, &job).unwrap(),
        Classified::Clean,
        "the host's log ends with End, whatever the mirror holds"
    );
}

#[test]
fn a_clean_run_whose_log_was_cut_in_the_live_view_is_recorded() {
    let (w, mut drain, id, run_id) = with_full_view("long-log-record");
    // The solver finished: the host holds the iterations already mirrored, then the played run.
    w.play(&run_id);
    let played = String::from_utf8(w.shared.file(&run_id, "log.simpleFoam").unwrap()).unwrap();
    w.shared
        .replace(&run_id, "log.simpleFoam", &(cavity_log(1, 40) + &played));

    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    assert_eq!(w.state(&id), JobState::Finishing);
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);

    let job = w.job(&id);
    assert_eq!(job.state, JobState::Finished, "{:?}", job.code);
    assert_eq!(w.runs().len(), 1, "the run was recorded");
}
