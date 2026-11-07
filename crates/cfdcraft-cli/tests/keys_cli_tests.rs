//! The `keys` verb, run through the real binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .output()
        .unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("keys-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn study_file(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("the binary ended by itself")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The 64 characters after `label: ` on its own line, when the line is there and well formed.
fn hash_after(text: &str, label: &str) -> Option<String> {
    let line = text.lines().find_map(|l| l.strip_prefix(label))?;
    let ok = line.len() == 64
        && line
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    ok.then(|| line.to_owned())
}

#[test]
fn keys_prints_the_study_hash_and_both_keys() {
    let dir = scratch("print");
    let file = study_file(&dir, "cavity.json", CAVITY_V1);
    let o = run(&["keys", &file]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    assert!(hash_after(&text, "study hash: ").is_some(), "{text}");
    let mesh = hash_after(&text, "mesh key: ").expect("a mesh key line");
    let result = hash_after(&text, "result key: ").expect("a result key line");
    assert_ne!(mesh, result);
}

#[test]
fn keys_json_is_one_report_and_nothing_else() {
    let dir = scratch("json");
    let file = study_file(&dir, "cavity.json", CAVITY_V1);
    let o = run(&["keys", &file, "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(err(&o), "");
    let report: Value = serde_json::from_slice(&o.stdout).expect("stdout is one JSON document");
    assert_eq!(report["operation"], "keys");
    assert_eq!(report["exit_class"], 0);
    let kinds: Vec<&str> = report["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["geometry_hash", "mesh_key", "result_key"]);
}

#[test]
fn the_same_study_prints_the_same_keys() {
    let dir = scratch("same");
    let file = study_file(&dir, "cavity.json", CAVITY_V1);
    let first = run(&["keys", &file]);
    let second = run(&["keys", &file]);
    assert_eq!(code(&first), 0);
    assert_eq!(out(&first), out(&second));
}

#[test]
fn an_invalid_study_exits_2_and_prints_no_key() {
    let dir = scratch("invalid");
    let file = study_file(&dir, "bad.json", r#"{"schema_version": 1}"#);
    let o = run(&["keys", &file]);
    assert_eq!(code(&o), 2);
    assert!(!out(&o).contains("mesh key"), "{}", out(&o));
    assert!(!err(&o).trim().is_empty());
}

#[test]
fn a_missing_file_exits_2() {
    let dir = scratch("missing");
    let file = dir.join("absent.json");
    let o = run(&["keys", file.to_str().unwrap()]);
    assert_eq!(code(&o), 2);
    assert!(out(&o).is_empty());
    assert!(err(&o).contains("cannot read study"), "{}", err(&o));
}
