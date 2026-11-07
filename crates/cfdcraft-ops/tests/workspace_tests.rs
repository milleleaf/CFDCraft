//! The named workspace: names that cannot escape, studies stored only when valid, compile and
//! export by name, and a run started by name against the fake box.
#![cfg(unix)]

mod support;

use cfdcraft_model::Study;
use cfdcraft_ops::{ExitClass, Report, Workspace};
use std::path::PathBuf;
use support::*;

const GROUP: &str = "ops-workspace-tests";
const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap()
}

/// A fresh workspace under `target/`, beside the fake box.
fn open(tag: &str) -> (Workspace, PathBuf) {
    let root = new_root(GROUP, tag);
    (Workspace::open(root.join("ws")).unwrap(), root.join("ws"))
}

fn codes(r: &Report) -> Vec<&str> {
    r.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn only_plain_lower_case_names_are_accepted() {
    for ok in ["cavity", "cavity-1", "a", "0", &"a".repeat(40)] {
        assert!(Workspace::valid_name(ok), "{ok}");
    }
    let long = "a".repeat(41);
    for bad in [
        "", "../x", "a/b", "a\\b", ".hidden", "A", "Cavity", "-x", "a b", "a.json", &long, "a\0b",
        "a\nb", "a\n", "é", "..", "a_b",
    ] {
        assert!(!Workspace::valid_name(bad), "{bad:?}");
    }
    let (ws, _) = open("names");
    for bad in ["../x", "", "A", "a/b"] {
        let r = ws.create_study(bad, &json(CAVITY));
        assert_eq!(r.class(), ExitClass::StudyInvalid);
        assert_eq!(codes(&r), ["CFD-WS-001"], "{bad:?}");
        assert!(
            bad.len() < 3 || !r.to_json().contains(bad),
            "{bad:?} is echoed"
        );
        assert_eq!(codes(&ws.validate_study(bad)), ["CFD-WS-001"]);
    }
}

#[test]
fn a_valid_study_is_stored_and_parses_back_equal() {
    let (ws, dir) = open("create");
    let r = ws.create_study("cavity", &json(CAVITY));
    assert_eq!(r.class(), ExitClass::Passed, "{}", r.render_text());
    assert_eq!(r.operation, "create_study");
    assert_eq!(r.study_hash.as_deref().map(str::len), Some(64));
    let stored = std::fs::read_to_string(dir.join("studies/cavity.json")).unwrap();
    assert_eq!(
        Study::from_json(&stored).unwrap(),
        Study::from_json(CAVITY).unwrap()
    );
    // No temporary file is left behind.
    let names: Vec<_> = std::fs::read_dir(dir.join("studies"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["cavity.json"]);
    let v = ws.validate_study("cavity");
    assert_eq!(v.class(), ExitClass::Passed);
    assert_eq!(v.study_hash, r.study_hash);
}

#[test]
fn an_invalid_study_is_not_stored_and_a_schema_0_study_is_stored_migrated() {
    let (ws, dir) = open("invalid");
    // A side of the domain left without a boundary condition.
    let mut bad = json(CAVITY);
    let bcs = bad["boundary_conditions"].as_object_mut().unwrap();
    let first = bcs.keys().next().unwrap().clone();
    bcs.remove(&first);
    let r = ws.create_study("broken", &bad);
    assert_eq!(r.class(), ExitClass::StudyInvalid);
    assert!(codes(&r).contains(&"CFD-BC-001"), "{:?}", codes(&r));
    assert!(!dir.join("studies/broken.json").exists());

    let r = ws.create_study("old", &json(CAVITY_V0));
    assert_eq!(r.class(), ExitClass::Passed, "{}", r.render_text());
    assert_eq!(r.migration.as_ref().map(|n| (n.from, n.to)), Some((0, 1)));
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("studies/old.json")).unwrap())
            .unwrap();
    assert_eq!(stored["schema_version"], 1);
}

#[test]
fn a_name_is_taken_once_and_a_missing_study_is_not_found() {
    let (ws, dir) = open("twice");
    assert_eq!(
        ws.create_study("cavity", &json(CAVITY)).class(),
        ExitClass::Passed
    );
    let first = std::fs::read(dir.join("studies/cavity.json")).unwrap();
    let r = ws.create_study("cavity", &json(CHANNEL));
    assert_eq!(r.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&r), ["CFD-WS-003"]);
    assert_eq!(
        std::fs::read(dir.join("studies/cavity.json")).unwrap(),
        first
    );
    let r = ws.validate_study("nope");
    assert_eq!(r.class(), ExitClass::StudyInvalid);
    assert_eq!(codes(&r), ["CFD-WS-002"]);
}

#[test]
fn opening_creates_six_directories_and_refuses_symbolic_links() {
    let root = new_root(GROUP, "open").join("ws");
    Workspace::open(&root).unwrap();
    for d in [
        "studies",
        "compiled",
        "exports",
        "runs",
        "documents",
        "packs",
    ] {
        assert!(root.join(d).is_dir(), "{d}");
    }
    Workspace::open(&root).unwrap();

    let linked = new_root(GROUP, "linked");
    let ws = linked.join("ws");
    let elsewhere = linked.join("elsewhere");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, ws.join("exports")).unwrap();
    let err = Workspace::open(&ws).unwrap_err();
    assert!(err.contains("symbolic link"), "{err}");
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);

    let file = new_root(GROUP, "file").join("ws");
    std::fs::create_dir_all(&file).unwrap();
    std::fs::write(file.join("runs"), "x").unwrap();
    assert!(Workspace::open(&file).is_err());
}

#[test]
fn the_report_has_a_json_schema_naming_its_fields() {
    let schema = serde_json::to_string(&schemars::schema_for!(Report)).unwrap();
    for field in ["exit_class", "diagnostics", "artifacts"] {
        assert!(schema.contains(field), "{field}");
    }
}

// ---- compile and export by name ----

fn golden(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../cfdcraft-backend-esi/tests/golden")
        .join(name)
}

/// Every file under `dir` by `/`-separated relative path.
fn files_under(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        out: &mut std::collections::BTreeMap<String, Vec<u8>>,
    ) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(name, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn golden_compiled() -> std::collections::BTreeMap<String, Vec<u8>> {
    files_under(&golden("cavity-v2512"))
}

fn golden_export() -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut all = golden_compiled();
    all.extend(files_under(&golden("cavity-v2512-export")));
    all
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn assert_relative(r: &Report, ws_dir: &std::path::Path) {
    let json = r.to_json();
    assert!(
        !json.contains(&ws_dir.to_string_lossy().into_owned()),
        "{json}"
    );
    // Built in two parts so the source does not itself read as a home path.
    let home_prefix = format!("/{}/", "Users");
    assert!(
        !json.contains(&home_prefix) && !json.contains("target/"),
        "{json}"
    );
    for a in &r.artifacts {
        assert!(
            !a.name.starts_with('/') && !a.name.contains(".."),
            "{}",
            a.name
        );
    }
}

#[test]
fn compile_by_name_writes_the_golden_case() {
    let (ws, dir) = open("compile");
    ws.create_study("cavity", &json(CAVITY));
    let r = ws.compile("cavity");
    assert_eq!(r.class(), ExitClass::Passed, "{}", r.render_text());
    assert_eq!(r.operation, "compile");
    assert_eq!(files_under(&dir.join("compiled/cavity")), golden_compiled());
    assert_eq!(r.artifacts.len(), golden_compiled().len());
    assert!(
        r.artifacts
            .iter()
            .all(|a| golden_compiled().contains_key(&a.name))
    );
    assert_relative(&r, &dir);
}

#[test]
fn export_by_name_adds_the_run_script_and_readme() {
    let (ws, dir) = open("export");
    ws.create_study("cavity", &json(CAVITY));
    let r = ws.export("cavity");
    assert_eq!(r.class(), ExitClass::Passed, "{}", r.render_text());
    assert_eq!(files_under(&dir.join("exports/cavity")), golden_export());
    for name in ["Allrun", "README.md"] {
        assert!(r.artifacts.iter().any(|a| a.name == name), "{name}");
    }
    assert_relative(&r, &dir);
}

#[test]
fn output_directories_are_replaced_not_merged() {
    let (ws, dir) = open("stale");
    ws.create_study("cavity", &json(CAVITY));
    for (sub, expected) in [
        ("compiled", golden_compiled()),
        ("exports", golden_export()),
    ] {
        let target = dir.join(sub).join("cavity");
        assert_eq!(
            if sub == "compiled" {
                ws.compile("cavity")
            } else {
                ws.export("cavity")
            }
            .class(),
            ExitClass::Passed
        );
        std::fs::write(target.join("stray.txt"), "x").unwrap();
        std::fs::create_dir_all(target.join("extra/deeper")).unwrap();
        std::fs::write(target.join("extra/deeper/file"), "y").unwrap();
        std::fs::write(target.join("system/controlDict"), "tampered").unwrap();
        let again = if sub == "compiled" {
            ws.compile("cavity")
        } else {
            ws.export("cavity")
        };
        assert_eq!(again.class(), ExitClass::Passed);
        assert_eq!(files_under(&target), expected, "{sub}");
        assert!(!target.join("extra").exists());
        assert_eq!(
            names(&dir.join(sub)),
            ["cavity"],
            "no temporary directory is left"
        );
    }
}

#[test]
fn a_study_edited_invalid_after_storing_leaves_earlier_outputs_alone() {
    let (ws, dir) = open("edited");
    ws.create_study("cavity", &json(CAVITY));
    ws.compile("cavity");
    ws.export("cavity");
    let before = (
        files_under(&dir.join("compiled/cavity")),
        files_under(&dir.join("exports/cavity")),
    );
    let mut bad = json(CAVITY);
    let bcs = bad["boundary_conditions"].as_object_mut().unwrap();
    let first = bcs.keys().next().unwrap().clone();
    bcs.remove(&first);
    for edited in [bad.to_string(), "not json".to_owned()] {
        std::fs::write(dir.join("studies/cavity.json"), &edited).unwrap();
        for r in [ws.compile("cavity"), ws.export("cavity")] {
            assert_eq!(r.class(), ExitClass::StudyInvalid, "{}", r.render_text());
            assert_relative(&r, &dir);
        }
        assert_eq!(files_under(&dir.join("compiled/cavity")), before.0);
        assert_eq!(files_under(&dir.join("exports/cavity")), before.1);
        assert_eq!(names(&dir.join("compiled")), ["cavity"]);
        assert_eq!(names(&dir.join("exports")), ["cavity"]);
    }
}

#[test]
fn a_missing_study_is_not_found_and_makes_no_directory() {
    let (ws, dir) = open("missing");
    for r in [
        ws.compile("missing"),
        ws.export("missing"),
        ws.compile("../x"),
        ws.export("A"),
    ] {
        assert_eq!(r.class(), ExitClass::StudyInvalid);
        assert!(
            matches!(codes(&r)[..], ["CFD-WS-002"] | ["CFD-WS-001"]),
            "{:?}",
            codes(&r)
        );
    }
    for sub in ["compiled", "exports", "runs", "studies"] {
        assert!(names(&dir.join(sub)).is_empty(), "{sub}");
    }
}

// ---- starting a run by name ----

use cfdcraft_ops::{ResultOutcome, RunMode, RunRegistry, ServerSettings, StartOutcome};
use std::time::{Duration, Instant};

/// A study with a short end time, so a smoke run is allowed.
fn short_cavity() -> serde_json::Value {
    let mut v = json(CAVITY);
    v["solver"]["end_time"] = serde_json::json!({"value": 20, "unit": "s"});
    v["solver"]["write_interval"] = serde_json::json!({"value": 20, "unit": "s"});
    v
}

fn wait_finished(ws: &Workspace, reg: &RunRegistry, id: &str) -> Report {
    let start = Instant::now();
    loop {
        if let ResultOutcome::Finished(r) = ws.run_result(reg, id) {
            return *r;
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the run did not finish"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn started(outcome: StartOutcome) -> String {
    match outcome {
        StartOutcome::Started { run_id } => run_id,
        other => panic!("expected Started, got {other:?}"),
    }
}

fn refused(outcome: StartOutcome) -> Report {
    match outcome {
        StartOutcome::Refused(report) => *report,
        other => panic!("expected Refused, got {other:?}"),
    }
}

/// Child side. Does nothing unless the parent set the root.
#[test]
fn workspace_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let ws = Workspace::open(root.join("ws")).unwrap();
    let runs = root.join("ws/runs");
    let settings = ServerSettings::new(
        root.join("dummy-key"),
        root.join("hosts.toml"),
        "box",
        workspace_root().join("target/cfdcraft-sockets"),
    )
    .unwrap();
    let reg = RunRegistry::new();
    assert_eq!(
        ws.create_study("cavity", &json(CAVITY)).class(),
        ExitClass::Passed
    );
    match std::env::var("CFC_TEST_SCENARIO").unwrap().as_str() {
        "run" => {
            let id = started(ws.start_run(&reg, &settings, "cavity", RunMode::Run, false));
            assert!(id.contains("cavity"), "{id}");
            let report = wait_finished(&ws, &reg, &id);
            assert_eq!(report.class(), ExitClass::GateFailed);
            assert!(runs.join(&id).join("report.json").is_file());
            assert!(runs.join(&id).join("case/Allrun").is_file());
            assert!(matches!(
                ws.run_status(&reg, &id),
                cfdcraft_ops::Status::Finished { exit_class: 1, .. }
            ));
        }
        "export" => {
            assert!(ssh_log(&root).is_empty());
            let r = refused(ws.start_run(&reg, &settings, "cavity", RunMode::Run, true));
            assert_eq!(r.class(), ExitClass::StudyInvalid);
            assert_eq!(codes(&r), ["CFD-WS-002"]);
            assert!(ssh_log(&root).is_empty(), "the box was contacted");
            assert_eq!(ws.export("cavity").class(), ExitClass::Passed);
            let id = started(ws.start_run(&reg, &settings, "cavity", RunMode::Run, true));
            wait_finished(&ws, &reg, &id);
            // The stored export was uploaded as it is: no second copy was written for the run.
            assert!(!runs.join(&id).join("case").exists());
            assert!(
                ssh_log(&root).iter().any(|l| l.starts_with("tar -xf")),
                "{:?}",
                ssh_log(&root)
            );
        }
        "tampered" => {
            assert_eq!(ws.export("cavity").class(), ExitClass::Passed);
            let file = root.join("ws/exports/cavity/system/controlDict");
            let mut bytes = std::fs::read(&file).unwrap();
            bytes[0] ^= 1;
            std::fs::write(&file, bytes).unwrap();
            let r = refused(ws.start_run(&reg, &settings, "cavity", RunMode::Run, true));
            assert_eq!(r.class(), ExitClass::GuardRefused);
            assert_eq!(codes(&r), ["CFD-RT-003"]);
            assert!(ssh_log(&root).is_empty(), "the box was contacted");
        }
        "modes" => {
            assert_eq!(
                ws.create_study("channel", &json(CHANNEL)).class(),
                ExitClass::Passed
            );
            assert_eq!(
                ws.create_study("short", &short_cavity()).class(),
                ExitClass::Passed
            );
            let r = refused(ws.start_run(&reg, &settings, "cavity", RunMode::Smoke, false));
            assert_eq!(
                (r.class(), codes(&r)),
                (ExitClass::StudyInvalid, vec!["CFD-RUN-001"])
            );
            let r = refused(ws.start_run(&reg, &settings, "channel", RunMode::Validate, false));
            assert_eq!(
                (r.class(), codes(&r)),
                (ExitClass::StudyInvalid, vec!["CFD-RUN-002"])
            );
            assert!(ssh_log(&root).is_empty(), "the box was contacted");
            for (name, mode) in [
                ("cavity", RunMode::Validate),
                ("short", RunMode::Smoke),
                ("cavity", RunMode::Run),
            ] {
                let id = started(ws.start_run(&reg, &settings, name, mode, false));
                let report = wait_finished(&ws, &reg, &id);
                assert_eq!(report.operation, mode.operation());
            }
        }
        "busy" => match ws.start_run(&reg, &settings, "cavity", RunMode::Run, false) {
            StartOutcome::Busy {
                holder,
                retry_after_seconds,
                stale,
            } => {
                assert_eq!(holder.unwrap().run_id, "other-run-1");
                assert!((30..=600).contains(&retry_after_seconds));
                assert!(!stale);
            }
            other => panic!("expected Busy, got {other:?}"),
        },
        "ids" => {
            for bad in ["", "../../etc", "a/b", "a b", "x\n", "é"] {
                assert_eq!(ws.run_status(&reg, bad), cfdcraft_ops::Status::NotFound);
                assert!(matches!(ws.run_result(&reg, bad), ResultOutcome::NotFound));
            }
        }
        other => panic!("unknown scenario {other}"),
    }
}

fn scenario(tag: &str, which: &str, setup: impl FnOnce(&std::path::Path)) -> PathBuf {
    let root = new_root(GROUP, tag);
    setup(&root);
    let mut c = worker(&root, "workspace_worker", "ok");
    c.env("CFC_TEST_SCENARIO", which);
    assert_worker_ok(&c.output().unwrap());
    root
}

#[test]
fn a_run_starts_by_name_and_lives_in_the_workspace() {
    let root = scenario("run", "run", |_| {});
    assert!(!lock_dir(&root).exists());
}

#[test]
fn a_run_can_be_the_stored_export() {
    scenario("run-export", "export", |_| {});
}

#[test]
fn a_hand_edited_export_is_refused_before_the_box_is_contacted() {
    scenario("tampered", "tampered", |_| {});
}

#[test]
fn modes_are_checked_before_the_box_is_contacted() {
    scenario("modes", "modes", |_| {});
}

#[test]
fn a_busy_box_is_reported_with_its_holder() {
    let root = scenario("busy", "busy", |r| hold_lock(r, now_epoch(), 3600));
    assert!(lock_dir(&root).join("holder.json").exists());
}

#[test]
fn run_ids_outside_the_plain_alphabet_are_not_found() {
    scenario("ids", "ids", |_| {});
}

#[test]
fn settings_are_built_once_by_the_host_and_do_not_echo_the_key_path() {
    let root = new_root(GROUP, "settings");
    let ok = ServerSettings::new(
        root.join("dummy-key"),
        root.join("hosts.toml"),
        "box",
        root.join("sockets"),
    );
    assert!(ok.is_ok());
    let missing = root.join("no-such-key-file");
    let err = ServerSettings::new(
        &missing,
        root.join("hosts.toml"),
        "box",
        root.join("sockets"),
    )
    .unwrap_err();
    assert!(!err.contains(missing.to_string_lossy().as_ref()), "{err}");
    assert!(!err.contains("no-such-key-file"), "{err}");

    // Every operation is called from names, modes and a flag alone: there is nowhere to pass a
    // key, host, profile or path.
    let ws = Workspace::open(root.join("ws")).unwrap();
    let reg = RunRegistry::new();
    let settings = ok.unwrap();
    let _: Report = ws.create_study("a", &json(CAVITY));
    let _: Report = ws.validate_study("a");
    let _: Report = ws.compile("a");
    let _: Report = ws.export("a");
    let _ = ws.start_run(&reg, &settings, "a", RunMode::Run, false);
    let _ = ws.run_status(&reg, "a");
    let _ = ws.run_result(&reg, "a");
}
