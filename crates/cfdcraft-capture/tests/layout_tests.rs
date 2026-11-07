use std::path::{Path, PathBuf};

use cfdcraft_capture::{
    CaptureError, CapturePlan, entry_names, key_numbers_with, mesh_bundle_with, result_bundle_with,
};

fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("capture-layout-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn put(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn plan() -> CapturePlan {
    CapturePlan::files(
        "run.log".to_owned(),
        names(&["extra.log"]),
        names(&["out/result.csv", "history.csv"]),
        names(&["mesh/mesh.dat"]),
    )
}

fn folder(test: &str) -> PathBuf {
    let dir = scratch(test);
    put(&dir, "run.log", "solver log\n");
    put(&dir, "extra.log", "other log\n");
    put(&dir, "out/result.csv", "1,2\n");
    put(&dir, "history.csv", "a,b\n");
    put(&dir, "mesh/mesh.dat", "nodes\n");
    put(&dir, "out/unrelated.txt", "not kept\n");
    put(&dir, "notes.txt", "not kept\n");
    dir
}

#[test]
fn a_file_plan_keeps_exactly_the_named_files() {
    let dir = folder("exact");
    let plan = plan();

    let result = result_bundle_with(&dir, 1.0, &plan).unwrap();
    assert_eq!(
        entry_names(&result.to_bytes().unwrap()).unwrap(),
        ["extra.log", "history.csv", "out/result.csv", "run.log"]
    );

    let mesh = mesh_bundle_with(&dir, &plan.mesh).unwrap();
    assert_eq!(
        entry_names(&mesh.to_bytes().unwrap()).unwrap(),
        ["mesh/mesh.dat"]
    );
}

#[test]
fn a_name_listed_twice_is_kept_once() {
    let dir = folder("twice");
    let mut plan = plan();
    plan.logs.push("run.log".to_owned());
    plan.fields.push("history.csv".to_owned());
    let result = result_bundle_with(&dir, 1.0, &plan).unwrap();
    assert_eq!(result.names().len(), 4);
}

#[test]
fn a_missing_named_file_is_refused_and_named() {
    let dir = folder("missing");
    std::fs::remove_file(dir.join("out/result.csv")).unwrap();
    match result_bundle_with(&dir, 1.0, &plan()) {
        Err(CaptureError::Missing(name)) => assert_eq!(name, "out/result.csv"),
        other => panic!("expected a missing file, got {other:?}"),
    }
    std::fs::remove_file(dir.join("mesh/mesh.dat")).unwrap();
    match mesh_bundle_with(&dir, &plan().mesh) {
        Err(CaptureError::Missing(name)) => assert_eq!(name, "mesh/mesh.dat"),
        other => panic!("expected a missing file, got {other:?}"),
    }
}

#[test]
fn an_empty_mesh_list_gives_an_empty_deterministic_bundle() {
    let dir = folder("empty");
    let none: Vec<String> = Vec::new();
    let first = mesh_bundle_with(&dir, &none).unwrap();
    let second = mesh_bundle_with(&dir, &none).unwrap();
    assert!(first.names().is_empty());
    assert_eq!(first.hash().unwrap(), second.hash().unwrap());
}

#[test]
fn a_file_plan_has_no_log_derived_numbers() {
    let dir = folder("numbers");
    let numbers = key_numbers_with(&dir, 1.0, &plan()).unwrap();
    assert!(numbers.is_empty());
}

#[cfg(unix)]
#[test]
fn a_link_where_a_named_file_belongs_is_not_followed() {
    let dir = folder("link");
    std::fs::remove_file(dir.join("history.csv")).unwrap();
    std::os::unix::fs::symlink(dir.join("notes.txt"), dir.join("history.csv")).unwrap();
    assert!(matches!(
        result_bundle_with(&dir, 1.0, &plan()),
        Err(CaptureError::NotAFile(name)) if name == "history.csv"
    ));
}
