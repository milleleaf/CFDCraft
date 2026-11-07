//! The live view: bounded pulls, the progress summary in the job, and the watcher that reads the
//! mirror and never the host.

use std::fs;

use cfdcraft_drain::watch::{self, WatchError};
use cfdcraft_drain::{Drain, TickOutcome};
use cfdcraft_queue::{Action, JobKind, JobState, NewJob};
use cfdcraft_settle::Status;

mod support;
use support::*;

const MIB: usize = 1024 * 1024;

/// A job added and started: the drain, the id and the run name.
fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    let run_id = w.run_id(&id);
    (w, drain, id, run_id)
}

fn mirror_len(w: &World, id: &str, rel: &str) -> u64 {
    fs::metadata(w.live(id).join(rel)).map_or(0, |m| m.len())
}

fn mirror_bytes(w: &World, id: &str, rel: &str) -> Vec<u8> {
    fs::read(w.live(id).join(rel)).unwrap_or_default()
}

#[test]
fn the_log_is_mirrored_without_a_gap_or_a_repeat() {
    let (w, mut drain, id, run_id) = started("live-gap");
    let log = cavity_log(1, 40);
    let end = log.len();
    // Seven uneven pieces; several end in the middle of a line.
    let cuts = [137, 500, 501, 1203, 2900, end - 7, end];
    let (mut at, mut lens) = (0, Vec::new());
    for cut in cuts {
        w.shared.append(&run_id, "log.simpleFoam", &log[at..cut]);
        at = cut;
        drain.tick().unwrap();
        lens.push(mirror_len(&w, &id, "log.simpleFoam"));
    }

    let want = w.shared.file(&run_id, "log.simpleFoam").unwrap();
    assert_eq!(mirror_bytes(&w, &id, "log.simpleFoam"), want);

    let prefix = format!("tail {run_id} log.simpleFoam ");
    let offsets: Vec<u64> = w
        .shared
        .calls()
        .iter()
        .filter_map(|call| call.strip_prefix(&prefix))
        .map(|offset| offset.parse().unwrap())
        .collect();
    assert_eq!(offsets.len(), cuts.len());
    assert_eq!(offsets[0], 0);
    // Each call starts where the mirror ended after the call before it.
    for (offset, len) in offsets[1..].iter().zip(&lens) {
        assert_eq!(offset, len, "the offset is the mirror length");
    }
    for pair in offsets.windows(2) {
        assert!(pair[0] <= pair[1], "an offset never goes down");
    }
}

#[test]
fn a_pull_is_capped_at_one_mebibyte_per_file_per_tick() {
    let (w, mut drain, id, run_id) = started("live-cap");
    let line = format!("{}\n", "x".repeat(1023));
    w.shared
        .append(&run_id, "log.simpleFoam", &line.repeat(3 * MIB / 1024));
    for step in 1..=3 {
        drain.tick().unwrap();
        assert_eq!(
            mirror_len(&w, &id, "log.simpleFoam"),
            (step * MIB) as u64,
            "after tick {step}"
        );
    }
    drain.tick().unwrap();
    assert_eq!(mirror_len(&w, &id, "log.simpleFoam"), (3 * MIB) as u64);
}

#[test]
fn a_remote_file_shorter_than_the_mirror_is_started_again() {
    let (w, mut drain, id, run_id) = started("live-shorter");
    let rel = "postProcessing/probe_1/0/U";
    w.shared.grow_probes(&run_id, 1, 50);
    drain.tick().unwrap();
    assert_eq!(
        mirror_bytes(&w, &id, rel),
        w.shared.file(&run_id, rel).unwrap()
    );
    assert!(mirror_len(&w, &id, rel) > 200);

    let short = "# Probe 0 (0.05 0.05 0.025)\n#       Probe 0\n#          Time\n1 (0.010000000000 -0.020000000000 0)\n";
    w.shared.replace(&run_id, rel, short);
    drain.tick().unwrap();
    assert_eq!(mirror_bytes(&w, &id, rel), short.as_bytes());
}

#[test]
fn monitor_files_are_mirrored_under_postprocessing() {
    let (w, mut drain, id, run_id) = started("live-monitors");
    w.shared.grow_probes(&run_id, 1, 20);
    drain.tick().unwrap();
    for probe in 1..=5 {
        let rel = format!("postProcessing/probe_{probe}/0/U");
        let want = w.shared.file(&run_id, &rel).unwrap();
        assert!(!want.is_empty());
        assert_eq!(mirror_bytes(&w, &id, &rel), want, "{rel}");
    }
}

/// A run with 300 iterations and the probes that go with them, pulled once.
fn at_iteration_300(tag: &str) -> (World, Drain<Shared>, String, String) {
    let (w, mut drain, id, run_id) = started(tag);
    w.shared.grow_log(&run_id, 1, 300);
    w.shared.grow_probes(&run_id, 1, 300);
    drain.tick().unwrap();
    (w, drain, id, run_id)
}

#[test]
fn each_tick_writes_progress_into_the_job() {
    let (w, _drain, id, _run_id) = at_iteration_300("live-progress");
    let progress = w.job(&id).progress.expect("progress was written");
    assert_eq!(progress.iterations, 300);
    assert!((progress.time.unwrap() - 300.0).abs() < 1e-9);
    assert_eq!(progress.monitors, 10, "five probes, two components each");

    let view = watch::monitors(&w.doc, &id).unwrap();
    let count = |wanted: &[Status]| {
        view.rows
            .iter()
            .filter(|r| wanted.contains(&r.state))
            .count() as u32
    };
    assert_eq!(progress.settled, count(&[Status::Settled]));
    assert_eq!(
        progress.drifting,
        count(&[Status::Drifting, Status::Oscillating])
    );
    let summary = progress.summary.expect("a one-line summary");
    assert_eq!(summary, format!("settled {} of 10", progress.settled));
}

#[test]
fn a_tick_that_changes_nothing_does_not_write_progress_again() {
    let (w, mut drain, _id, _run_id) = at_iteration_300("live-same");
    let before = fs::read(cfdcraft_doc::store_dir(&w.doc).join("queue.json")).unwrap();
    drain.tick().unwrap();
    let after = fs::read(cfdcraft_doc::store_dir(&w.doc).join("queue.json")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn watch_monitors_returns_the_table_for_a_running_job() {
    let (w, _drain, id, _run_id) = at_iteration_300("live-table");
    let calls = w.shared.calls().len();
    let view = watch::monitors(&w.doc, &id).unwrap();
    assert_eq!(
        w.shared.calls().len(),
        calls,
        "the watcher never calls the host"
    );

    assert_eq!(view.job, id);
    assert_eq!(view.state, JobState::Running);
    assert_eq!(view.iterations, 300);
    assert!(!view.ended);
    assert_eq!(view.rows.len(), 10);
    let names: Vec<&str> = view.rows.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"probe_1.Ux"), "{names:?}");
    assert!(names.contains(&"probe_5.Uy"), "{names:?}");
    assert_eq!(
        view.rows[0].label.as_deref(),
        Some(view.rows[0].name.as_str())
    );
    for row in &view.rows {
        assert!(row.last.is_finite() && row.mean.is_finite() && row.spread.is_finite());
    }
}

#[test]
fn watch_log_returns_a_redacted_tail_from_an_offset() {
    let (w, mut drain, id, run_id) = started("live-log");
    w.shared.grow_log(&run_id, 1, 5);
    w.shared.append(
        &run_id,
        "log.simpleFoam",
        "Reading /home/user/secret/path from cfdcraft-cavity-1-2 done\n",
    );
    drain.tick().unwrap();

    let first = watch::log(&w.doc, &id, 0, 4).unwrap();
    assert_eq!(first.text.lines().count(), 4);
    assert!(first.next_offset > 0);

    let (mut offset, mut all) = (0, String::new());
    loop {
        let view = watch::log(&w.doc, &id, offset, 7).unwrap();
        if view.next_offset == offset {
            break;
        }
        all.push_str(&view.text);
        all.push('\n');
        offset = view.next_offset;
    }
    assert!(all.contains("Time = 5"), "{all}");
    assert!(all.contains("Reading <path> from <run> done"), "{all}");
    assert!(!all.contains("/home/user"));
    assert!(!all.contains("cfdcraft-cavity"));
    assert_eq!(offset, mirror_len(&w, &id, "log.simpleFoam"));

    // An offset in the middle of a line starts at the beginning of that line.
    let at = watch::log(&w.doc, &id, first.next_offset, 2).unwrap();
    let mid = watch::log(&w.doc, &id, first.next_offset + 3, 2).unwrap();
    assert_eq!(mid, at);
}

#[test]
fn a_finished_job_can_still_be_watched_and_a_missing_mirror_says_so() {
    let (w, mut drain, id, run_id) = started("live-finished");
    w.play(&run_id);
    drain.tick().unwrap();
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Finished);

    let view = watch::monitors(&w.doc, &id).unwrap();
    assert_eq!(view.state, JobState::Finished);
    assert!(view.ended);
    assert!(watch::log(&w.doc, &id, 0, 5).is_ok());

    w.file()
        .update(|_| {
            Ok(vec![Action::Enqueue {
                jobs: vec![NewJob {
                    scenario: None,
                    kind: JobKind::Solve,
                    origin: None,
                }],
            }])
        })
        .unwrap();
    assert_eq!(
        watch::monitors(&w.doc, "q-002"),
        Err(WatchError::NoMirror {
            job: "q-002".to_owned()
        })
    );
    assert!(matches!(
        watch::log(&w.doc, "q-002", 0, 5),
        Err(WatchError::NoMirror { .. })
    ));
    assert!(matches!(
        watch::monitors(&w.doc, "q-009"),
        Err(WatchError::UnknownJob { .. })
    ));
}

#[test]
fn a_job_id_that_is_not_a_job_id_is_refused() {
    let w = world("live-badid");
    for bad in ["../x", "q-1/../..", "", "q-", "q-abc", "Q-001", "q-001/x"] {
        assert!(
            matches!(watch::monitors(&w.doc, bad), Err(WatchError::BadJob { .. })),
            "{bad:?}"
        );
        assert!(
            matches!(
                watch::log(&w.doc, bad, 0, 5),
                Err(WatchError::BadJob { .. })
            ),
            "{bad:?}"
        );
    }
}
