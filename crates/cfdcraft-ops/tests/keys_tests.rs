use cfdcraft_backend_esi::{Dialect, dialect::IMAGE};
use cfdcraft_ops::{ExitClass, Report, stage_keys, tool_identity};
use serde_json::Value;

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn artifact(report: &Report, kind: &str) -> Option<String> {
    report
        .artifacts
        .iter()
        .find(|a| a.kind == kind)
        .map(|a| a.name.clone())
}

fn is_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn edited(change: impl FnOnce(&mut Value)) -> String {
    let mut study: Value = serde_json::from_str(CAVITY_V1).unwrap();
    change(&mut study);
    study.to_string()
}

#[test]
fn the_cavity_reports_its_two_keys() {
    let report = stage_keys(CAVITY_V1);
    assert_eq!(report.class(), ExitClass::Passed);
    assert_eq!(report.operation, "keys");
    assert!(is_hash(report.study_hash.as_deref().unwrap()));
    let kinds: Vec<&str> = report.artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert_eq!(kinds, ["geometry_hash", "mesh_key", "result_key"]);
    for artifact in &report.artifacts {
        assert!(is_hash(&artifact.name), "{}", artifact.kind);
    }
    assert_ne!(
        artifact(&report, "mesh_key"),
        artifact(&report, "result_key")
    );
}

#[test]
fn the_keys_follow_the_lid_speed() {
    let slow = stage_keys(CAVITY_V1);
    let fast = stage_keys(&edited(|s| {
        s["boundary_conditions"]["lid"]["velocity"]["value"] = Value::from(0.2);
    }));
    assert_eq!(fast.class(), ExitClass::Passed);
    assert_ne!(artifact(&slow, "result_key"), artifact(&fast, "result_key"));
    assert_eq!(artifact(&slow, "mesh_key"), artifact(&fast, "mesh_key"));
    assert_eq!(
        artifact(&slow, "geometry_hash"),
        artifact(&fast, "geometry_hash")
    );
}

#[test]
fn the_tool_identity_comes_from_the_dialect() {
    let tool = tool_identity(Dialect::EsiV2512);
    assert!(tool.image_digest.starts_with("sha256:"));
    assert_eq!(tool.image_digest, IMAGE.split_once('@').unwrap().1);
    assert_eq!(tool.dialect, "esi-v2512");
    assert_eq!(tool.sidecar_lock, None);
    assert_eq!(tool.writer_version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn an_invalid_study_gives_no_keys() {
    let report = stage_keys(r#"{"schema_version": 1}"#);
    assert_eq!(report.class(), ExitClass::StudyInvalid);
    assert_eq!(report.operation, "keys");
    assert_eq!(artifact(&report, "mesh_key"), None);
    assert_eq!(artifact(&report, "result_key"), None);
}

#[test]
fn a_schema_0_study_has_the_keys_of_its_migrated_form() {
    let old = stage_keys(CAVITY_V0);
    let new = stage_keys(CAVITY_V1);
    assert_eq!(old.class(), ExitClass::Passed);
    assert!(old.migration.is_some());
    assert!(new.migration.is_none());
    assert_eq!(artifact(&old, "mesh_key"), artifact(&new, "mesh_key"));
    assert_eq!(artifact(&old, "result_key"), artifact(&new, "result_key"));
    assert_eq!(old.study_hash, new.study_hash);
}

#[test]
fn warnings_pass_through() {
    let report = stage_keys(&edited(|s| {
        s["mesh"]["cells_x"] = Value::from(2);
        s["mesh"]["cells_y"] = Value::from(2);
    }));
    assert_eq!(report.class(), ExitClass::Passed);
    assert!(report.diagnostics.iter().any(|d| d.code == "CFD-WARN-002"));
    assert!(artifact(&report, "mesh_key").is_some());
}
