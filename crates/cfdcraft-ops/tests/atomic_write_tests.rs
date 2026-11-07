//! Study storage through the shared crash-safe exclusive create. Runs on every platform: it needs
//! no fake host, only a workspace folder.

use cfdcraft_model::Study;
use cfdcraft_ops::{ExitClass, Report, Workspace};
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap()
}

fn open(tag: &str) -> (Workspace, PathBuf) {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("atomic-write-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("ws");
    (Workspace::open(&dir).unwrap(), dir)
}

fn codes(r: &Report) -> Vec<&str> {
    r.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn a_study_is_stored_as_pretty_json_with_a_newline_and_never_replaced() {
    let (ws, dir) = open("store");
    let r = ws.create_study("cavity", &json(CAVITY));
    assert_eq!(r.class(), ExitClass::Passed, "{}", r.render_text());
    let path = dir.join("studies").join("cavity.json");
    let (parsed, _) = Study::from_json_with_note(&json(CAVITY).to_string()).unwrap();
    let expected = serde_json::to_string_pretty(&parsed).unwrap() + "\n";
    let stored = std::fs::read_to_string(&path).unwrap();
    assert_eq!(stored, expected);

    let again = ws.create_study("cavity", &json(CAVITY_V0));
    assert_eq!(codes(&again), ["CFD-WS-003"]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), stored);

    let names: Vec<_> = std::fs::read_dir(dir.join("studies"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["cavity.json"]);
}

#[test]
fn an_older_schema_is_stored_migrated() {
    let (ws, dir) = open("migrated");
    let r = ws.create_study("cavity", &json(CAVITY_V0));
    assert_eq!(r.class(), ExitClass::Passed, "{}", r.render_text());
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("studies/cavity.json")).unwrap())
            .unwrap();
    assert_eq!(stored["schema_version"], 1);
}

#[test]
fn eight_callers_creating_one_name_give_one_study_and_seven_refusals() {
    let (ws, dir) = open("race");
    let ws = Arc::new(ws);
    let gate = Arc::new(Barrier::new(8));
    let value = json(CAVITY);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (ws, gate, value) = (ws.clone(), gate.clone(), value.clone());
            std::thread::spawn(move || {
                gate.wait();
                ws.create_study("cavity", &value)
            })
        })
        .collect();
    let reports: Vec<Report> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let passed = reports
        .iter()
        .filter(|r| r.class() == ExitClass::Passed)
        .count();
    let refused = reports
        .iter()
        .filter(|r| codes(r) == ["CFD-WS-003"])
        .count();
    assert_eq!((passed, refused), (1, 7));
    let stored = std::fs::read_to_string(dir.join("studies/cavity.json")).unwrap();
    assert!(Study::from_json_with_note(&stored).is_ok());
}
