//! The in-memory host: a run is started, read, stopped and collected through
//! the `Host` trait alone.

use cfdcraft_remote::{
    ContainerState, Holder, Host, HostError, ReleaseOutcome, RemoteFile, TimeDir, fake::FakeBox,
};
use std::path::{Path, PathBuf};

const ID: &str = "cfdcraft-demo-1-2";

/// A fresh folder under the workspace target folder.
fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/remote-fake-tests")
        .join(format!("{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A case folder with two files, one of them in a sub-folder.
fn case(tag: &str) -> PathBuf {
    let dir = scratch(tag).join("case");
    std::fs::create_dir_all(dir.join("system")).unwrap();
    std::fs::write(dir.join("Allrun"), "#!/bin/sh\n").unwrap();
    std::fs::write(dir.join("system/controlDict"), "stopAt endTime;\n").unwrap();
    dir
}

fn started(tag: &str) -> FakeBox {
    let mut b = FakeBox::new();
    b.start(ID, &case(tag), "demo", 1).unwrap();
    b
}

#[test]
fn start_poll_stop_download_on_the_fake_box() {
    let mut b = FakeBox::new();
    b.solver_obeys_stop(true);
    b.start(ID, &case("flow"), "demo", 1).unwrap();
    assert_eq!(b.holder().unwrap().unwrap().run_id, ID);
    assert_eq!(b.inspect(ID).unwrap(), ContainerState::Running);
    assert_eq!(b.file(ID, "Allrun"), Some(&b"#!/bin/sh\n"[..]));
    assert!(b.file(ID, "system/controlDict").is_some());

    b.append(ID, "log.simpleFoam", "Time = 1\n");
    b.append(ID, "log.simpleFoam", "Time = 2\n");
    let all = b.tail(ID, "log.simpleFoam", 0, 1000).unwrap();
    assert_eq!(all, b"Time = 1\nTime = 2\n");
    assert_eq!(
        b.tail(ID, "log.simpleFoam", 9, 1000).unwrap(),
        b"Time = 2\n"
    );
    assert_eq!(b.inspect(ID).unwrap(), ContainerState::Running);

    b.write_text(ID, "system/controlDict", "stopAt writeNow;\n")
        .unwrap();
    assert_eq!(
        b.inspect(ID).unwrap(),
        ContainerState::Exited {
            code: 0,
            oom_killed: false
        }
    );
    let log = b.tail(ID, "log.simpleFoam", 0, 1000).unwrap();
    assert!(log.ends_with(b"End\n"));
    let names: Vec<_> = b
        .time_dirs(ID)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, ["2"]);

    let dest = scratch("flow-out").join("results");
    b.download(ID, &dest).unwrap();
    assert!(dest.join("log.simpleFoam").is_file());
    assert!(dest.join("2/U").is_file());
    assert_eq!(b.release(ID).unwrap(), ReleaseOutcome::Released);
    assert_eq!(b.holder().unwrap(), None);
}

#[test]
fn a_second_start_while_the_lock_is_held_is_busy() {
    let mut b = started("busy");
    let other = "cfdcraft-demo-3-4";
    assert_eq!(
        b.start(other, &case("busy2"), "demo", 1),
        Err(HostError::Busy { stale: false })
    );
    assert_eq!(b.inspect(other).unwrap(), ContainerState::None);
}

#[test]
fn release_by_another_id_is_not_ours_and_with_no_lock_is_absent() {
    let mut b = FakeBox::new();
    assert_eq!(b.release(ID).unwrap(), ReleaseOutcome::Absent);
    b.set_holder(Some(Holder::new(ID, ID, 0, 60, "t").unwrap()));
    // An id that is only the start of the holder's id is not the holder.
    assert_eq!(
        b.release("cfdcraft-demo-1").unwrap(),
        ReleaseOutcome::NotOurs
    );
    assert!(b.holder().unwrap().is_some());
    assert_eq!(b.release(ID).unwrap(), ReleaseOutcome::Released);
}

#[test]
fn tail_beyond_the_end_returns_nothing_and_a_missing_file_returns_nothing() {
    let mut b = FakeBox::new();
    b.append(ID, "log", "abc");
    assert_eq!(b.tail(ID, "log", 3, 10).unwrap(), b"");
    assert_eq!(b.tail(ID, "log", 99, 10).unwrap(), b"");
    assert_eq!(b.tail(ID, "log", 1, 1).unwrap(), b"b");
    assert_eq!(b.tail(ID, "nothing", 0, 10).unwrap(), b"");
}

#[test]
fn files_lists_sizes_under_a_folder_and_time_dirs_count_kilobytes() {
    let mut b = FakeBox::new();
    b.append(ID, "0.5/U", &"x".repeat(1500));
    b.append(ID, "postProcessing/probes/0/p", "12345");
    b.append(ID, "postProcessing/other", "1");
    b.append(ID, "log", "1");
    assert_eq!(
        b.files(ID, "postProcessing").unwrap(),
        [
            RemoteFile {
                rel: "postProcessing/other".into(),
                size: 1
            },
            RemoteFile {
                rel: "postProcessing/probes/0/p".into(),
                size: 5
            },
        ]
    );
    assert_eq!(b.files(ID, ".").unwrap().len(), 4);
    assert_eq!(
        b.time_dirs(ID).unwrap(),
        [TimeDir {
            name: "0.5".into(),
            kb: 2
        }]
    );
}

#[test]
fn fail_next_makes_exactly_n_calls_unreachable_then_recovers() {
    let mut b = started("fail");
    b.fail_next(2);
    assert_eq!(b.inspect(ID), Err(HostError::Unreachable));
    assert_eq!(b.free_kb(), Err(HostError::Unreachable));
    assert_eq!(b.inspect(ID).unwrap(), ContainerState::Running);
    assert!(b.free_kb().unwrap() > 0);
    b.set_free_kb(7);
    assert_eq!(b.free_kb().unwrap(), 7);
}

#[test]
fn a_path_with_dot_dot_or_a_leading_slash_is_refused() {
    let mut b = FakeBox::new();
    for bad in ["../x", "a/../b", "/etc/passwd", "-x", "a b", "a'b", ""] {
        assert!(
            matches!(b.tail(ID, bad, 0, 1), Err(HostError::Refused { .. })),
            "tail {bad:?}"
        );
        assert!(
            matches!(b.read_text(ID, bad), Err(HostError::Refused { .. })),
            "read_text {bad:?}"
        );
        assert!(
            matches!(b.write_text(ID, bad, "x"), Err(HostError::Refused { .. })),
            "write_text {bad:?}"
        );
    }
    assert!(matches!(
        b.read_text("a b", "log"),
        Err(HostError::Refused { .. })
    ));
    assert!(b.file(ID, "../x").is_none());
}

#[test]
fn download_leaves_out_processor_folders() {
    let mut b = FakeBox::new();
    b.append(ID, "0.5/U", "u");
    b.append(ID, "processor0/0.5/U", "u");
    b.append(ID, "log", "l");
    let dest = scratch("proc").join("results");
    b.download(ID, &dest).unwrap();
    assert!(dest.join("0.5/U").is_file());
    assert!(dest.join("log").is_file());
    assert!(!dest.join("processor0").exists());
}

#[test]
fn the_call_log_records_every_call_in_order() {
    let mut b = FakeBox::new();
    b.inspect(ID).unwrap();
    b.holder().unwrap();
    b.fail_next(1);
    let _ = b.free_kb();
    b.tail(ID, "log", 4, 10).unwrap();
    b.release(ID).unwrap();
    assert_eq!(
        b.calls(),
        [
            format!("inspect {ID}"),
            "holder".to_string(),
            "free_kb".to_string(),
            format!("tail {ID} log 4"),
            format!("release {ID}"),
        ]
    );
}
