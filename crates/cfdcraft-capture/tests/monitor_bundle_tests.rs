use std::path::{Path, PathBuf};

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_capture::{
    CaptureError, checkpoint_bundle, entry_names, latest_time_dir, monitor_names,
    monitor_names_limited, read_entry, result_bundle,
};

fn results(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("capture-monitor-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let spec = FixtureSpec {
        end_time: 5000,
        cells: 64,
        samples: 4,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    dir
}

fn put(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const MONITORS: [&str; 5] = [
    "postProcessing/balance_inlet/0/surfaceFieldValue.dat",
    "postProcessing/line_1/1000/line_U.xy",
    "postProcessing/probe_1/0/U",
    "postProcessing/probe_1/1200/U",
    "postProcessing/surface_1/0/surfaceFieldValue.dat",
];

fn with_monitors(test: &str) -> PathBuf {
    let dir = results(test);
    for name in MONITORS {
        put(&dir, name, "monitor text\n");
    }
    put(&dir, "postProcessing/other/x", "not a monitor\n");
    dir
}

#[test]
fn monitor_files_are_listed_in_order_and_only_function_object_folders() {
    let dir = with_monitors("list");
    assert_eq!(monitor_names(&dir).unwrap(), MONITORS);
}

#[test]
fn the_result_bundle_holds_the_monitor_files() {
    let dir = with_monitors("result");
    let bytes = result_bundle(&dir, 5000.0).unwrap().to_bytes().unwrap();
    let names = entry_names(&bytes).unwrap();
    for name in MONITORS {
        assert!(names.iter().any(|n| n == name), "{name}");
    }
    assert!(!names.iter().any(|n| n.contains("other")));
    assert_eq!(names.len(), 7 + MONITORS.len());
    assert_eq!(
        read_entry(&bytes, MONITORS[2]).unwrap(),
        b"monitor text\n".to_vec()
    );

    let plain = results("result-plain");
    let bytes = result_bundle(&plain, 5000.0).unwrap().to_bytes().unwrap();
    assert_eq!(
        entry_names(&bytes).unwrap(),
        [
            "5000/U",
            "5000/p",
            "log.blockMesh",
            "log.checkMesh",
            "log.simpleFoam",
            "postProcessing/centreline/5000/horizontal_U.xy",
            "postProcessing/centreline/5000/vertical_U.xy",
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_link_among_the_monitor_files_is_refused() {
    let dir = with_monitors("link");
    let link = dir.join("postProcessing/probe_1/0/U");
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(dir.join("log.blockMesh"), link).unwrap();
    for error in [
        monitor_names(&dir).unwrap_err(),
        result_bundle(&dir, 5000.0).unwrap_err(),
    ] {
        assert!(matches!(error, CaptureError::NotAFile(_)), "{error:?}");
        assert!(error.to_string().contains("probe_1/0/U"), "{error}");
    }
}

#[cfg(unix)]
#[test]
fn a_link_for_a_function_object_folder_is_refused() {
    let dir = results("link-folder");
    std::os::unix::fs::symlink(&dir, dir.join("postProcessing/probe_9")).unwrap();
    let error = monitor_names(&dir).unwrap_err();
    assert!(matches!(error, CaptureError::NotAFile(_)), "{error:?}");
}

#[test]
fn more_monitor_files_than_the_limit_are_refused() {
    let dir = results("many");
    for i in 0..201 {
        put(&dir, &format!("postProcessing/probe_1/{i}/U"), "x\n");
    }
    assert_eq!(monitor_names_limited(&dir, 201).unwrap().len(), 201);
    let error = monitor_names_limited(&dir, 200).unwrap_err();
    assert!(matches!(error, CaptureError::Invalid(_)), "{error:?}");
}

#[test]
fn lines_with_many_write_times_are_not_refused() {
    // Eight line monitors with 30 write times each are 240 files, over the old limit of 200.
    let dir = results("lines");
    for line in 1..=8 {
        for time in 1..=30 {
            put(
                &dir,
                &format!("postProcessing/line_{line}/{time}/line_U.xy"),
                "x\n",
            );
        }
    }
    assert_eq!(monitor_names(&dir).unwrap().len(), 240);
    let bytes = result_bundle(&dir, 5000.0).unwrap().to_bytes().unwrap();
    let names = entry_names(&bytes).unwrap();
    assert_eq!(names.iter().filter(|n| n.contains("/line_")).count(), 240);
}

#[test]
fn the_latest_time_folder_is_the_largest_positive_number() {
    let dir = results("latest");
    for name in ["0", "1000", "1200", "constant", "system"] {
        std::fs::create_dir_all(dir.join(name)).unwrap();
    }
    // The fixture also holds 5000 and postProcessing; take 5000 away to test the rest.
    std::fs::remove_dir_all(dir.join("5000")).unwrap();
    assert_eq!(latest_time_dir(&dir).unwrap(), "1200");

    let only_zero = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("capture-monitor-zero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&only_zero);
    for name in ["0", "constant", "system", "postProcessing"] {
        std::fs::create_dir_all(only_zero.join(name)).unwrap();
    }
    let error = latest_time_dir(&only_zero).unwrap_err();
    assert!(matches!(error, CaptureError::Missing(_)), "{error:?}");
}

fn stopped(test: &str) -> PathBuf {
    let dir = with_monitors(test);
    std::fs::create_dir_all(dir.join("1200")).unwrap();
    put(&dir, "1200/U", "field U at 1200\n");
    put(&dir, "1200/p", "field p at 1200\n");
    dir
}

#[test]
fn a_checkpoint_bundle_holds_the_fields_the_logs_and_the_monitor_files() {
    let dir = stopped("checkpoint");
    put(&dir, "1200/phi", "flux at 1200\n");
    let bytes = checkpoint_bundle(&dir, "1200").unwrap().to_bytes().unwrap();
    let names = entry_names(&bytes).unwrap();
    for name in [
        "1200/U",
        "1200/p",
        "1200/phi",
        "log.blockMesh",
        "log.checkMesh",
        "log.simpleFoam",
    ]
    .into_iter()
    .chain(MONITORS)
    {
        assert!(names.iter().any(|n| n == name), "{name}");
    }
    assert_eq!(names.len(), 6 + MONITORS.len());
    assert_eq!(read_entry(&bytes, "1200/U").unwrap(), b"field U at 1200\n");

    std::fs::remove_file(dir.join("1200/phi")).unwrap();
    let without = checkpoint_bundle(&dir, "1200").unwrap().to_bytes().unwrap();
    assert!(
        !entry_names(&without)
            .unwrap()
            .iter()
            .any(|n| n.ends_with("phi"))
    );

    std::fs::remove_file(dir.join("1200/U")).unwrap();
    let error = checkpoint_bundle(&dir, "1200").unwrap_err();
    assert!(matches!(error, CaptureError::Missing(_)), "{error:?}");
}

#[test]
fn a_checkpoint_time_that_is_not_a_listed_folder_is_refused() {
    let dir = stopped("time-argument");
    for time in ["../1200", "constant", "1200/..", "7", ""] {
        let error = checkpoint_bundle(&dir, time).unwrap_err();
        assert!(
            matches!(error, CaptureError::Missing(_)),
            "{time}: {error:?}"
        );
    }
}

#[test]
fn a_checkpoint_bundle_is_deterministic() {
    let a = stopped("same-a");
    let b = stopped("same-b");
    let (a, b) = (
        checkpoint_bundle(&a, "1200").unwrap(),
        checkpoint_bundle(&b, "1200").unwrap(),
    );
    assert_eq!(a.to_bytes().unwrap(), b.to_bytes().unwrap());
    assert_eq!(a.hash().unwrap(), b.hash().unwrap());
}
