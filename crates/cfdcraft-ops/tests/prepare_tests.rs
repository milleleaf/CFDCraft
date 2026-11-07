use cfdcraft_backend_esi::{CaseTree, Dialect};
use cfdcraft_model::Severity;
use cfdcraft_ops::{ExitClass, Report, prepare, prepare::prepare_with_tamper};
use std::path::{Path, PathBuf};

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");
const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cfdcraft-backend-esi/tests/golden")
        .join(name)
}

fn bytes_of(tree: &CaseTree, name: &str) -> Vec<u8> {
    tree.files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap()
        .bytes
        .clone()
}

fn edit(tree: &mut CaseTree, name: &str, change: impl Fn(&str) -> String) {
    let file = tree
        .files
        .iter_mut()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    file.bytes = change(std::str::from_utf8(&file.bytes).unwrap()).into_bytes();
}

fn failed(result: Result<cfdcraft_ops::Prepared, Report>) -> Report {
    result.expect_err("the chain should have stopped")
}

#[test]
fn the_cavity_passes_every_gate_and_equals_the_golden_case() {
    let p = prepare(CAVITY_V0, Dialect::EsiV2512).unwrap();
    assert_eq!(p.hash, CAVITY_HASH);
    assert_eq!(p.tree.files.len(), 8);
    for file in &p.tree.files {
        assert_eq!(
            file.bytes,
            std::fs::read(golden("cavity-v2512").join(&file.rel_path)).unwrap(),
            "{}",
            file.rel_path.display()
        );
    }
    assert_eq!(p.export_tree.files.len(), 10);
    for name in ["Allrun", "README.md"] {
        assert_eq!(
            bytes_of(&p.export_tree, name),
            std::fs::read(golden("cavity-v2512-export").join(name)).unwrap()
        );
    }
    assert_eq!(p.migration.as_ref().map(|n| (n.from, n.to)), Some((0, 1)));
    assert!(p.warnings.is_empty());

    let v1 = prepare(CAVITY_V1, Dialect::EsiV2512).unwrap();
    assert!(v1.migration.is_none());
    assert_eq!(v1.hash, CAVITY_HASH);
}

#[test]
fn the_second_golden_case_passes_the_guard_and_the_round_trip() {
    let p = prepare(CHANNEL_V1, Dialect::EsiV2512).unwrap();
    for file in &p.tree.files {
        assert_eq!(
            file.bytes,
            std::fs::read(golden("channel-v2512").join(&file.rel_path)).unwrap(),
            "{}",
            file.rel_path.display()
        );
    }
}

#[test]
fn a_study_with_a_missing_outlet_is_class_2_and_has_no_tree() {
    let outlet = "    \"outlet\": {\n      \"faces\": [\"XMax\"],\n      \"kind\": \"PressureOutlet\"\n    },\n";
    let broken = CHANNEL_V1.replace(outlet, "");
    assert_ne!(broken, CHANNEL_V1, "the fixture changed shape");
    let report = failed(prepare(&broken, Dialect::EsiV2512));
    assert_eq!(report.class(), ExitClass::StudyInvalid);
    assert_eq!(report.status, "failed");
    assert!(report.diagnostics.iter().any(|d| d.code == "CFD-BC-001"));
    assert!(report.artifacts.is_empty());

    let malformed = failed(prepare("{", Dialect::EsiV2512));
    assert_eq!(malformed.class(), ExitClass::StudyInvalid);
    assert_eq!(malformed.diagnostics[0].code, "CFD-STUDY-001");
}

#[test]
fn a_zeroed_viscosity_is_refused_by_the_read_back() {
    let report = failed(prepare_with_tamper(CAVITY_V1, Dialect::EsiV2512, |tree| {
        edit(tree, "constant/transportProperties", |t| {
            t.replace("0.0001;", "0;")
        });
    }));
    assert_eq!(report.class(), ExitClass::GuardRefused);
    let d = report
        .diagnostics
        .iter()
        .find(|d| d.code == "CFD-RT-001")
        .expect("a read-back finding");
    assert_eq!(d.pointer, "/fluid/kinematic_viscosity");
    assert_eq!(report.study_hash.as_deref(), Some(CAVITY_HASH));
}

#[test]
fn a_code_stream_entry_is_refused_with_file_line_and_entry() {
    let tamper = |tree: &mut CaseTree| {
        edit(tree, "system/controlDict", |t| {
            t.replace(
                "application simpleFoam;",
                "application simpleFoam;\nx #codeStream;",
            )
        });
    };
    let report = failed(prepare_with_tamper(CAVITY_V1, Dialect::EsiV2512, tamper));
    assert_eq!(report.class(), ExitClass::GuardRefused);
    let d = &report.diagnostics[0];
    assert_eq!(d.code, "CFD-GUARD-001");
    let at = d.location.as_ref().unwrap();
    assert_eq!(at.file, "system/controlDict");
    assert_eq!(at.line, 10);
    assert_eq!(at.entry.as_deref(), Some("#codeStream"));
}

#[test]
fn the_guard_runs_before_the_read_back() {
    let report = failed(prepare_with_tamper(CAVITY_V1, Dialect::EsiV2512, |tree| {
        edit(tree, "constant/transportProperties", |t| {
            t.replace("0.0001;", "0;")
        });
        edit(tree, "system/controlDict", |t| {
            t.replace(
                "application simpleFoam;",
                "application simpleFoam;\nx #codeStream;",
            )
        });
    }));
    assert_eq!(report.class(), ExitClass::GuardRefused);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.code.starts_with("CFD-GUARD-")),
        "{:?}",
        report.diagnostics
    );
}

#[test]
fn warnings_are_carried_and_do_not_change_the_class() {
    let fast = CAVITY_V1.replace("\"value\": 0.0001,", "\"value\": 0.000001,");
    assert_ne!(fast, CAVITY_V1, "the fixture changed shape");
    let p = prepare(&fast, Dialect::EsiV2512).unwrap();
    assert!(p.warnings.iter().any(|d| d.code == "CFD-WARN-001"));
    assert!(p.warnings.iter().all(|d| d.severity == Severity::Warning));

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("warn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let report = cfdcraft_ops::compile_study(&fast, &dir);
    assert_eq!(report.class(), ExitClass::Passed);
    assert!(report.diagnostics.iter().any(|d| d.code == "CFD-WARN-001"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_report_round_trips_through_json_with_a_numeric_class() {
    let report = failed(prepare("{", Dialect::EsiV2512));
    let json = report.to_json();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["exit_class"], 2);
    assert_eq!(value["exit_class_name"], "study_invalid");
    assert_eq!(value["status"], "failed");
    assert_eq!(serde_json::from_str::<Report>(&json).unwrap(), report);
}

#[test]
fn every_class_has_a_distinct_code_and_name() {
    use ExitClass::*;
    let all = [
        Passed,
        GateFailed,
        StudyInvalid,
        GuardRefused,
        Infrastructure,
    ];
    let codes: Vec<u8> = all.iter().map(|c| c.code()).collect();
    assert_eq!(codes, [0, 1, 2, 3, 4]);
    for c in all {
        let mut r = Report::new("x");
        r.set_class(c);
        assert_eq!(r.class(), c);
    }
}
