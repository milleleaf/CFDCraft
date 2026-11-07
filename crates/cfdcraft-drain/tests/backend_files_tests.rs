//! The drain mirrors the files a backend's run plan names when the live specification lists them,
//! reads the process count a job was started with from a file it wrote, and does not call the
//! finished job of another backend a failure for lacking an OpenFOAM line. A job on an OpenFOAM
//! line keeps its specification, its host calls and its explanation as they were.

use std::fs;
use std::path::Path;

use cfdcraft_drain::case::{NP_FILE, lines_for, restore_np};
use cfdcraft_drain::explain::{self, Classified, TIME_LIMIT};
use cfdcraft_drain::watch::mirrored_files_of;
use cfdcraft_drain::{Drain, TickOutcome};
use cfdcraft_logscan::ErrorId;
use cfdcraft_queue::JobState;
use cfdcraft_remote::ContainerState;
use cfdcraft_text::Line;
use serde_json::{Value, json};

mod support;
use support::*;

const OTHER: &str = "su2-8.6.0";

/// Changes the specification file of a job's live folder, as JSON.
fn edit_spec(live: &Path, change: impl FnOnce(&mut Value)) {
    let path = live.join("spec.json");
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, value.to_string()).unwrap();
}

/// A world with one job that has started: the world, its drain, the job id and the run name.
fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    assert_eq!(w.state(&id), JobState::Running);
    let run_id = w.run_id(&id);
    (w, drain, id, run_id)
}

#[test]
fn a_live_folder_without_a_file_list_means_the_openfoam_set() {
    let dir = scratch("files-none");
    assert!(mirrored_files_of(&dir).is_empty(), "no specification");

    fs::write(dir.join("spec.json"), "not json").unwrap();
    assert!(mirrored_files_of(&dir).is_empty(), "unreadable");

    fs::write(dir.join("spec.json"), json!({ "spec": {} }).to_string()).unwrap();
    assert!(mirrored_files_of(&dir).is_empty(), "no files member");

    let named = json!({ "files": ["a.log", "../x", "out/b.csv", "/abs", "-flag"] });
    fs::write(dir.join("spec.json"), named.to_string()).unwrap();
    assert_eq!(mirrored_files_of(&dir), ["a.log", "out/b.csv"]);
}

#[test]
fn a_started_openfoam_job_writes_a_spec_with_no_file_list() {
    let (w, drain, id, _run) = started("files-openfoam-spec");
    let text = fs::read_to_string(w.live(&id).join("spec.json")).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert!(value.get("files").is_none(), "{text}");
    assert!(!text.contains("files"), "{text}");
    assert!(mirrored_files_of(&w.live(&id)).is_empty());

    // The count the job ran with is in its file, and is the count the drain keeps.
    let written = fs::read_to_string(w.live(&id).join(NP_FILE)).unwrap();
    assert_eq!(
        Some(written.trim().parse::<u32>().unwrap()),
        drain.scratch().np
    );
}

#[test]
fn a_file_list_in_the_spec_is_the_set_that_is_mirrored() {
    let (w, mut drain, id, run_id) = started("files-listed");
    let live = w.live(&id);
    edit_spec(&live, |value| {
        value["files"] = json!(["solver.log", "out/result.csv", "../x"]);
    });
    w.shared
        .append(&run_id, "solver.log", "step one\nstep two\n");
    w.shared.append(&run_id, "out/result.csv", "a,b\n1,2\n");
    w.shared.append(&run_id, "log.simpleFoam", "Time = 1\n\n");
    w.shared.grow_probes(&run_id, 1, 3);

    let before = w.shared.calls().len();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);

    assert_eq!(
        fs::read_to_string(live.join("solver.log")).unwrap(),
        "step one\nstep two\n"
    );
    assert_eq!(
        fs::read_to_string(live.join("out/result.csv")).unwrap(),
        "a,b\n1,2\n"
    );
    // The solver goes on writing: the next piece is appended, not fetched again from the start.
    w.shared.append(&run_id, "solver.log", "step three\n");
    drain.tick().unwrap();
    assert_eq!(
        fs::read_to_string(live.join("solver.log")).unwrap(),
        "step one\nstep two\nstep three\n"
    );

    // No OpenFOAM name was looked for, and the hostile name was never sent to the host.
    let calls = w.shared.calls();
    let after = &calls[before..];
    assert!(after.iter().any(|call| call.contains("solver.log")));
    assert!(after.iter().all(|call| !call.contains("../x")), "{after:?}");
    assert!(
        after
            .iter()
            .all(|call| !call.contains("log.simpleFoam") && !call.contains("log.blockMesh")),
        "{after:?}"
    );
    assert!(
        after
            .iter()
            .all(|call| !call.starts_with("files ") || !call.contains("postProcessing")),
        "{after:?}"
    );
    assert!(!live.join("log.simpleFoam").exists());
    assert!(!live.join("postProcessing").exists());
    assert!(!live.join("x").exists());
}

#[test]
fn the_np_a_job_ran_with_is_read_from_its_file() {
    let (w, _drain, id, _run) = started("files-np");
    let live = w.live(&id);
    let job = w.job(&id);
    let parallel = |lines: &[Line]| lines.iter().any(|l| matches!(l, Line::ParallelRun { .. }));

    // The file wins.
    fs::write(live.join(NP_FILE), "3\n").unwrap();
    let mut fresh = w.drain();
    restore_np(&mut fresh, &job);
    assert_eq!(fresh.scratch().np, Some(3));
    assert!(parallel(&lines_for(&fresh, &job)));

    // A live folder from before the file existed is read from the uploaded script.
    fs::remove_file(live.join(NP_FILE)).unwrap();
    fs::write(
        live.join("case").join("Allrun"),
        "#!/bin/sh\nmpirun -np 2 simpleFoam -parallel\n",
    )
    .unwrap();
    let mut fresh = w.drain();
    restore_np(&mut fresh, &job);
    assert_eq!(fresh.scratch().np, Some(2));
    assert!(parallel(&lines_for(&fresh, &job)));

    // With neither, the job ran alone.
    fs::write(live.join("case").join("Allrun"), "#!/bin/sh\nsimpleFoam\n").unwrap();
    let mut fresh = w.drain();
    restore_np(&mut fresh, &job);
    assert_eq!(fresh.scratch().np, Some(1));
    assert!(!parallel(&lines_for(&fresh, &job)));
}

#[test]
fn a_finished_log_of_another_backend_is_not_called_a_failure() {
    let (w, mut drain, id, run_id) = started("files-classify");
    // A log with no end line: for an OpenFOAM job that is a failure to explain.
    w.shared
        .append(&run_id, "log.simpleFoam", &cavity_log(1, 5));
    drain.tick().unwrap();
    let job = w.job(&id);
    let mut other = job.clone();
    other.backend = Some(OTHER.to_owned());

    let ours = explain::classify(&mut drain, &job).unwrap();
    assert!(matches!(ours, Classified::Failed { .. }), "{ours:?}");
    let theirs = explain::classify(&mut drain, &other).unwrap();
    assert_eq!(theirs, Classified::Clean);
}

#[test]
fn the_container_facts_still_fail_another_backends_job() {
    let (w, mut drain, id, run_id) = started("files-container");
    let mut other = w.job(&id);
    other.backend = Some(OTHER.to_owned());

    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed: true,
        },
    );
    match explain::classify(&mut drain, &other).unwrap() {
        Classified::Failed { code, .. } => assert_eq!(code, ErrorId::OutOfMemory.code()),
        found => panic!("expected the out-of-memory failure, got {found:?}"),
    }

    for status in [124, 137] {
        w.shared.set_container(
            &run_id,
            ContainerState::Exited {
                code: status,
                oom_killed: false,
            },
        );
        match explain::classify(&mut drain, &other).unwrap() {
            Classified::Failed { code, .. } => assert_eq!(code, TIME_LIMIT, "status {status}"),
            found => panic!("expected the time-limit failure, got {found:?}"),
        }
    }

    // Any other exit is not judged here: the backend's own judge decides at the record step.
    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed: false,
        },
    );
    assert_eq!(
        explain::classify(&mut drain, &other).unwrap(),
        Classified::Clean
    );
}
