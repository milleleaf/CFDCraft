//! The naming, pinning and disk criterion end to end, through the operations: a variant is named,
//! pinned and annotated, disk use is shown per variant, pruning is a dry run first and keeps what
//! a pinned variant needs and every run's key numbers, and nothing is removed by anything else
//! (`sc5_`). Also an older file brought forward by the first variant, and the reports of the new
//! operations being plain JSON that names nothing private. No test needs a compute host or a
//! container engine.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use cfdcraft_compare::Tolerance;
use cfdcraft_diskuse::DiskUse;
use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::disk_ops::{disk_use, prune};
use cfdcraft_ops::scenario_ops::{
    LabelUpdate, scenario_diff, scenario_label, scenario_list, scenario_set, scenario_show,
};
use cfdcraft_ops::sweep_ops::{sweep_create, sweep_plan, sweep_preflight};
use cfdcraft_ops::{
    DocOutcome, ExitClass, JobAction, JobOptions, PruneData, Report, Repro, ScenarioSummary,
    StatusData, document_status, list_runs, open_document, pack_document, plan_job,
    plan_scenario_job_with, plan_update_report, run_result, save_document, scenario_status,
    solve_document, solve_scenario, unpack_document, update_documents,
};
use cfdcraft_scenarios::Override;
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_sweep::{Parameter, Sweep, Target, Values};
use cfdcraft_text::Line;
use serde_json::{Value, json};
use zip::{ZipWriter, write::SimpleFileOptions};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";
const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

fn set(setting: &str, value: Value) -> Override {
    Override::Set {
        setting: setting.to_owned(),
        value,
    }
}

/// The five stored results of the solved document, each named by one run.
struct Blobs {
    /// The study's mesh, which `fast-lid` stands on too.
    mesh: ContentHash,
    /// The study's result.
    result: ContentHash,
    /// The result of `fast-lid`.
    fast: ContentHash,
    /// The mesh of `fine`.
    fine_mesh: ContentHash,
    /// The result of `fine`.
    fine: ContentHash,
}

impl Blobs {
    fn all(&self) -> [ContentHash; 5] {
        [self.mesh, self.result, self.fast, self.fine_mesh, self.fine]
    }
}

fn blobs(path: &Path) -> Blobs {
    let runs = open(path).unwrap().document.artifacts.runs;
    let mesh = |n: usize| runs[n].mesh.as_ref().unwrap().blob;
    let result = |n: usize| runs[n].result.as_ref().unwrap().blob;
    let found = Blobs {
        mesh: mesh(0),
        result: result(0),
        fast: result(1),
        fine_mesh: mesh(2),
        fine: result(2),
    };
    let distinct: std::collections::BTreeSet<ContentHash> = found.all().into_iter().collect();
    assert_eq!(distinct.len(), 5, "the five stored results all differ");
    found
}

/// A document of the cavity solved three times: the study itself (`run-1`, mesh and result),
/// `fast-lid` (`run-2`, a result on the study's mesh) and `fine` (`run-3`, its own mesh and result).
fn solved(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = vec![0.0, 0.05, 0.1];
    passed(&solve_document(&path, &mut runner).report);
    let made = [
        ("fast-lid", set(LID, json!(0.2))),
        ("fine", set(CELLS_X, json!(256))),
    ];
    for (id, change) in made {
        passed(&scenario_set(&path, id, Some("base"), &[change], None).report);
        passed(&solve_scenario(&path, Some(id), &mut runner).report);
    }
    path
}

fn folder(path: &Path) -> &Path {
    path.parent().unwrap()
}

fn pin(path: &Path, id: &str) {
    let update = LabelUpdate {
        pinned: Some(true),
        ..LabelUpdate::default()
    };
    passed(&scenario_label(path, id, &update).report);
}

fn usage(report: &Report) -> &DiskUse {
    match &report.repro {
        Some(Repro::Disk(data)) => data.as_ref(),
        other => panic!("expected the disk use, found {other:?}"),
    }
}

fn pruning(report: &Report) -> &PruneData {
    match &report.repro {
        Some(Repro::Prune(data)) => data.as_ref(),
        other => panic!("expected the data of a prune, found {other:?}"),
    }
}

fn stages(out: &DocOutcome) -> &StatusData {
    match &out.report.repro {
        Some(Repro::Status(data)) => data,
        other => panic!("expected the status, found {other:?}"),
    }
}

fn rows(path: &Path) -> Vec<ScenarioSummary> {
    let out = scenario_list(path);
    passed(&out.report);
    match out.report.repro {
        Some(Repro::Scenarios(data)) => data.scenarios,
        other => panic!("expected the list of variants, found {other:?}"),
    }
}

fn stored(path: &Path, hash: &ContentHash) -> bool {
    Store::open(store_dir(path)).unwrap().contains(hash)
}

fn sorted(mut hashes: Vec<ContentHash>) -> Vec<ContentHash> {
    hashes.sort();
    hashes
}

/// What the run records say, as data: the list of runs and the checks and key numbers of each.
fn records(path: &Path) -> Vec<Value> {
    let runs = open(path).unwrap().document.artifacts.runs;
    let mut found = vec![serde_json::to_value(list_runs(path).report.repro).unwrap()];
    for run in &runs {
        let out = run_result(path, Some(run.id.as_str()));
        passed(&out.report);
        found.push(serde_json::to_value(out.report.repro).unwrap());
    }
    found
}

// ---- success criterion 5: named, pinned and annotated; disk use; explicit pruning

#[test]
fn sc5_a_variant_is_named_pinned_and_annotated() {
    let path = solved("sc5-label");
    let notes = "checked against the reference\nsecond line";
    let update = LabelUpdate {
        name: Some("Fast lid".to_owned()),
        pinned: Some(true),
        notes: Some(notes.to_owned()),
    };
    let done = scenario_label(&path, "fast-lid", &update);
    passed(&done.report);
    assert!(matches!(done.line, Some(Line::ScenarioLabelled { .. })));

    // The list shows it, and so does the variant itself.
    let listed = rows(&path);
    let fast = listed.iter().find(|row| row.id == "fast-lid").unwrap();
    assert_eq!(fast.name.as_deref(), Some("Fast lid"));
    assert!(fast.pinned);
    assert_eq!(fast.notes.as_deref(), Some(notes));
    let fine = listed.iter().find(|row| row.id == "fine").unwrap();
    assert!(fine.name.is_none() && !fine.pinned && fine.notes.is_none());
    let out = scenario_show(&path, "fast-lid");
    passed(&out.report);
    match &out.report.repro {
        Some(Repro::Scenario(data)) => assert_eq!(&data.summary, fast),
        other => panic!("expected the data of a variant, found {other:?}"),
    }

    // The study itself can be named and pinned too, and is listed first.
    let base = LabelUpdate {
        name: Some("Reference".to_owned()),
        pinned: Some(true),
        ..LabelUpdate::default()
    };
    passed(&scenario_label(&path, "base", &base).report);
    let listed = rows(&path);
    assert_eq!(listed[0].id, "base");
    assert_eq!(listed[0].name.as_deref(), Some("Reference"));
    assert!(listed[0].pinned);

    // A label survives a pack and an unpack.
    let packed = folder(&path).join("cavity.cfdpack");
    passed(&pack_document(&path, &packed).report);
    let there = folder(&path).join("unpacked");
    passed(&unpack_document(&packed, &there).report);
    assert_eq!(rows(&there.join("cavity.cfdcraft")), rows(&path));

    // An empty name or notes text clears it, and a pin can be taken off.
    let clear = LabelUpdate {
        name: Some(String::new()),
        pinned: Some(false),
        notes: Some(String::new()),
    };
    passed(&scenario_label(&path, "fast-lid", &clear).report);
    let listed = rows(&path);
    let fast = listed.iter().find(|row| row.id == "fast-lid").unwrap();
    assert!(fast.name.is_none() && !fast.pinned && fast.notes.is_none());
}

#[test]
fn sc5_disk_use_per_variant_then_a_dry_run_then_an_explicit_prune() {
    let path = solved("sc5-prune");
    pin(&path, "fast-lid");
    let found = blobs(&path);
    let runs = open(&path).unwrap().document.artifacts.runs;
    let size = |n: usize| {
        let mesh = runs[n].mesh.as_ref().map_or(0, |out| out.size);
        mesh + runs[n].result.as_ref().map_or(0, |out| out.size)
    };

    // Disk use: bytes per variant, the shared mesh counted once in the total.
    let used = disk_use(&path);
    passed(&used.report);
    let before = usage(&used.report);
    let names: Vec<&str> = before
        .scenarios
        .iter()
        .map(|s| s.scenario.as_str())
        .collect();
    assert_eq!(names, ["base", "fast-lid", "fine"]);
    assert_eq!(before.scenarios[0].bytes, size(0));
    assert_eq!(
        before.scenarios[1].bytes,
        size(1) + runs[0].mesh.as_ref().unwrap().size
    );
    assert_eq!(before.scenarios[2].bytes, size(2));
    assert!(!before.scenarios[0].pinned && before.scenarios[1].pinned);
    assert_eq!(before.stored_bytes, size(0) + size(1) + size(2));
    assert_eq!(before.pruned_bytes, 0);

    // A dry run says what would go and changes nothing.
    let snap = snapshot(&path);
    let dry = prune(&path, None, false);
    passed(&dry.report);
    let plan = pruning(&dry.report);
    assert!(!plan.applied);
    assert_eq!(plan.plan.remove.len(), 3);
    assert_eq!(plan.plan.kept_pinned, 2);
    assert!(matches!(dry.line, Some(Line::PruneDryRun { .. })));
    assert_eq!(snapshot(&path), snap);
    assert!(open(&path).unwrap().document.artifacts.pruned.is_empty());
    let freed = plan.plan.freed_bytes;

    // Asking for it removes the results of unpinned variants and keeps the pinned one's.
    let done = prune(&path, None, true);
    passed(&done.report);
    assert!(pruning(&done.report).applied);
    assert_eq!(pruning(&done.report).plan.remove.len(), 3);
    assert!(matches!(done.line, Some(Line::Pruned { .. })));
    for gone in [found.result, found.fine_mesh, found.fine] {
        assert!(!stored(&path, &gone));
    }
    for kept in [found.mesh, found.fast] {
        assert!(stored(&path, &kept));
    }

    let after = disk_use(&path);
    let after = usage(&after.report);
    assert_eq!(after.pruned_bytes, freed);
    assert_eq!(after.stored_bytes + after.pruned_bytes, before.stored_bytes);
}

#[test]
fn sc5_fields_are_kept_for_pinned_runs_and_key_numbers_for_all() {
    let path = solved("sc5-kept");
    pin(&path, "fast-lid");
    let found = blobs(&path);
    let before = records(&path);
    let runs_before = open(&path).unwrap().document.artifacts.runs;

    passed(&prune(&path, None, true).report);

    // The pinned variant's result is in the store and reads back whole.
    let store = Store::open(store_dir(&path)).unwrap();
    for kept in [found.mesh, found.fast] {
        store.verify(&kept).unwrap();
    }

    // Every run record, with its checks and key numbers, is exactly what it was, pruned or not.
    let document = open(&path).unwrap().document;
    assert_eq!(document.artifacts.runs, runs_before);
    assert_eq!(records(&path), before);
    assert_eq!(
        document.artifacts.pruned,
        sorted(vec![found.result, found.fine_mesh, found.fine])
    );

    // A pruned stage reads as pruned, not as stale or missing, and says its key numbers are kept.
    let base = scenario_status(&path, Some("base"));
    passed(&base.report);
    assert_eq!(stages(&base).mesh.state, "fresh");
    assert_eq!(stages(&base).result.state, "pruned");
    assert_eq!(stages(&base).result.run.as_deref(), Some("run-1"));
    assert!(
        stages(&base)
            .result
            .sentence
            .contains("its key numbers are kept"),
        "{}",
        stages(&base).result.sentence
    );
    let fine = scenario_status(&path, Some("fine"));
    passed(&fine.report);
    assert_eq!(stages(&fine).mesh.state, "pruned");
    assert_eq!(stages(&fine).result.state, "pruned");
    let fast = scenario_status(&path, Some("fast-lid"));
    passed(&fast.report);
    assert_eq!(stages(&fast).mesh.state, "fresh");
    assert_eq!(stages(&fast).result.state, "fresh");
}

/// Every file under `root` with its size, sorted. Folders are left out: a folder's own size
/// changes with how many entries it holds on some systems, which is not a removal.
fn file_tree(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                let name = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .display()
                    .to_string();
                found.push(format!("{name} {}", meta.len()));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn sc5_nothing_is_deleted_automatically() {
    let dir = scratch("sc5-never");
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    let store = store_dir(&path);
    let mut kept = file_tree(&store);

    // After every step the store holds everything it held, and nothing is recorded as pruned.
    let after = |step: &str, path: &Path, kept: &mut Vec<String>| {
        let now = file_tree(&store);
        for entry in kept.iter() {
            assert!(now.contains(entry), "{step} removed {entry}");
        }
        *kept = now;
        let document = open(path).unwrap().document;
        assert!(document.artifacts.pruned.is_empty(), "{step}");
        let there = Store::open(store_dir(path)).unwrap();
        for blob in &document.artifacts.blobs {
            assert!(there.contains(&blob.hash), "{step}");
        }
    };

    passed(&solve_document(&path, &mut runner).report);
    after("solve", &path, &mut kept);
    let made = [
        ("fast-lid", set(LID, json!(0.2))),
        ("fine", set(CELLS_X, json!(256))),
    ];
    for (id, change) in made {
        passed(&scenario_set(&path, id, Some("base"), &[change], None).report);
        passed(&solve_scenario(&path, Some(id), &mut runner).report);
        after("solve of a variant", &path, &mut kept);
    }
    passed(&solve_scenario(&path, Some("fine"), &mut runner).report);
    after("second solve", &path, &mut kept);

    // A change of the study, an update (planned and applied), the statuses and a rerun.
    passed(&save_document(&with_lid_speed(0.15), &path, true).report);
    passed(&plan_update_report(std::slice::from_ref(&path), None, None).report);
    passed(&update_documents(std::slice::from_ref(&path), None, None, &mut runner).report);
    after("update", &path, &mut kept);
    passed(&document_status(&path).report);
    passed(&scenario_status(&path, Some("fine")).report);
    let again = JobAction::Rerun {
        run: "run-1".to_owned(),
        tolerance: Tolerance::same_image(),
        allow_tool_change: false,
    };
    let rerun = plan_job(&path, again)
        .expect("a plan")
        .run_with(&mut runner);
    passed(&rerun.report);
    after("rerun", &path, &mut kept);

    // Reading what is stored, planning a prune and sorting out the variants remove nothing.
    passed(&disk_use(&path).report);
    passed(&prune(&path, None, false).report);
    pin(&path, "fine");
    after("disk use, a dry run and a pin", &path, &mut kept);

    // Neither do packing and unpacking.
    let packed = dir.join("cavity.cfdpack");
    passed(&pack_document(&path, &packed).report);
    let there = dir.join("unpacked");
    passed(&unpack_document(&packed, &there).report);
    after("pack", &path, &mut kept);
    let unpacked = there.join("cavity.cfdcraft");
    let document = open(&unpacked).unwrap().document;
    assert!(document.artifacts.pruned.is_empty());
    assert!(!document.artifacts.blobs.is_empty());
    let copy = Store::open(store_dir(&unpacked)).unwrap();
    for blob in &document.artifacts.blobs {
        assert!(copy.contains(&blob.hash));
    }
}

#[test]
fn sc5_a_pruned_result_can_be_remade_on_request() {
    let path = solved("sc5-remake");
    let mut runner = FixtureRunner::new(&folder(&path).join("more-runs"));
    passed(&prune(&path, None, true).report);

    // Left alone by default: status says pruned and a solve starts nothing.
    let status = scenario_status(&path, Some("fine"));
    assert_eq!(stages(&status).mesh.state, "pruned");
    assert_eq!(stages(&status).result.state, "pruned");
    let idle = solve_scenario(&path, Some("fine"), &mut runner);
    passed(&idle.report);
    assert_eq!(idle.line, Some(Line::NothingToRun));
    assert_eq!(runner.calls, 0);

    // On request it is made again, and the stages are fresh.
    let options = JobOptions {
        remake_pruned: true,
    };
    let job =
        plan_scenario_job_with(&path, Some("fine"), JobAction::Solve, &options).expect("a plan");
    let done = job.run_with(&mut runner);
    passed(&done.report);
    assert_eq!(runner.calls, 1);
    assert_eq!(data(&done.report).recorded, ["mesh", "result"]);

    let status = scenario_status(&path, Some("fine"));
    passed(&status.report);
    assert_eq!(stages(&status).mesh.state, "fresh");
    assert_eq!(stages(&status).result.state, "fresh");
    let document = open(&path).unwrap().document;
    let store = Store::open(store_dir(&path)).unwrap();
    let remade = document.artifacts.runs.last().unwrap();
    assert_eq!(remade.scenario.as_deref(), Some("fine"));
    let mesh_blob = &remade.mesh.as_ref().unwrap().blob;
    let result_blob = &remade.result.as_ref().unwrap().blob;
    for blob in [mesh_blob, result_blob] {
        assert!(store.contains(blob));
        assert!(!document.artifacts.pruned.contains(blob));
    }
}

// ---- an older file takes variants

/// A ZIP file built straight with the ZIP crate, entry by entry.
fn archive(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn manifest(version: u32) -> Vec<u8> {
    json!({
        "format": "cfdcraft-study",
        "schema_version": version,
        "writer": { "name": "other-tool", "version": "9" }
    })
    .to_string()
    .into_bytes()
}

#[test]
fn a_schema_2_file_opens_migrates_and_takes_variants() {
    let dir = scratch("sc5-schema-2");
    let path = dir.join("old.cfdcraft");
    let index = json!({ "blobs": [], "links": [], "runs": [] });
    let bytes = archive(&[
        ("manifest.json", manifest(2)),
        ("study.json", CAVITY.as_bytes().to_vec()),
        ("artifacts.json", index.to_string().into_bytes()),
    ]);
    fs::write(&path, bytes).unwrap();

    // It opens with the one note, the study unchanged, and opening writes nothing.
    let opened = open_document(&path);
    passed(&opened.report);
    let note = opened.report.migration.as_ref().expect("a migration note");
    assert_eq!((note.from, note.to), (2, 3));
    assert_eq!(opened.report.study_hash.as_deref(), Some(CAVITY_HASH));
    let steps = open(&path).unwrap();
    assert_eq!(steps.notes.len(), 1);
    assert_eq!(steps.schema_version, 2);
    assert!(steps.document.artifacts.scenarios.is_empty());

    // It takes a variant, and the file is written at the current number.
    let made = scenario_set(&path, "fast-lid", None, &[set(LID, json!(0.2))], None);
    passed(&made.report);
    assert_eq!(made.report.study_hash.as_deref(), Some(CAVITY_HASH));
    assert!(made.report.migration.is_some());
    let saved = open(&path).unwrap();
    assert_eq!(saved.schema_version, 3);
    assert!(saved.notes.is_empty());
    assert_eq!(saved.document.artifacts.scenarios.len(), 1);

    // The study is the one that was there, so its hash is the hash of the cavity.
    let listed = scenario_list(&path);
    passed(&listed.report);
    assert_eq!(listed.report.study_hash.as_deref(), Some(CAVITY_HASH));
    assert_eq!(rows(&path).len(), 2);
}

// ---- the reports

/// A report that serialises to JSON, shows as text, and names neither the folder the document is
/// in nor the file it is.
fn plain(what: &str, out: &DocOutcome, dir: &Path) {
    let shown = dir.display().to_string();
    let json = out.report.to_json();
    let parsed: Value = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{what}: {e}"));
    if out.report.repro.is_some() {
        assert!(parsed["repro"]["kind"].is_string(), "{what}: {json}");
    }
    let mut texts = vec![json, out.report.render_text()];
    texts.extend(out.line.as_ref().map(say));
    for text in texts {
        assert!(!text.contains(&shown), "{what} names the folder: {text}");
        assert!(
            !text.contains("cavity.cfdcraft"),
            "{what} names the file: {text}"
        );
    }
}

fn lids() -> Sweep {
    let lid = Parameter {
        name: "lid_speed".to_owned(),
        target: Target::Number {
            setting: LID.to_owned(),
        },
        values: Values::List {
            items: vec![0.1, 0.2],
        },
    };
    let cells = Parameter {
        name: "cells_x".to_owned(),
        target: Target::Count {
            setting: CELLS_X.to_owned(),
        },
        values: Values::List {
            items: vec![20.0, 40.0],
        },
    };
    Sweep {
        id: "grid".to_owned(),
        base: "base".to_owned(),
        parameters: vec![lid, cells],
    }
}

fn too_many() -> Sweep {
    let cells = Parameter {
        name: "cells_x".to_owned(),
        target: Target::Count {
            setting: CELLS_X.to_owned(),
        },
        values: Values::Steps {
            start: 20.0,
            stop: 220.0,
            count: 201,
        },
    };
    Sweep {
        id: "big".to_owned(),
        base: "base".to_owned(),
        parameters: vec![cells],
    }
}

#[test]
fn every_new_report_is_json_and_names_nothing_private() {
    let path = solved("sc5-reports");
    let dir = folder(&path).to_owned();
    let other = set(LID, json!(0.4));
    let scope = ["nope".to_owned()];
    let label = LabelUpdate {
        name: Some("Pinned".to_owned()),
        pinned: Some(true),
        notes: Some("kept".to_owned()),
    };
    let outcomes = [
        (
            "scenario set",
            scenario_set(&path, "other", Some("fast-lid"), &[other], None),
        ),
        ("scenario list", scenario_list(&path)),
        ("scenario show", scenario_show(&path, "fast-lid")),
        ("scenario diff", scenario_diff(&path, "base", "fine")),
        ("scenario label", scenario_label(&path, "fast-lid", &label)),
        ("sweep plan", sweep_plan(&path, &lids())),
        ("sweep create", sweep_create(&path, &lids())),
        ("sweep preflight", sweep_preflight(&path, "grid")),
        ("disk", disk_use(&path)),
        ("prune dry run", prune(&path, None, false)),
        ("prune", prune(&path, None, true)),
        // The refusals say as little.
        ("unknown variant", scenario_show(&path, "nope")),
        ("unknown sweep", sweep_preflight(&path, "nope")),
        ("sweep over the limit", sweep_plan(&path, &too_many())),
        (
            "unknown variant to prune",
            prune(&path, Some(scope.as_slice()), true),
        ),
    ];
    for (what, out) in &outcomes {
        plain(what, out, &dir);
    }
    // The ones that should have passed did.
    for (what, out) in &outcomes[..11] {
        assert_eq!(out.report.class(), ExitClass::Passed, "{what}");
    }
    for (what, out) in &outcomes[11..] {
        assert_eq!(out.report.class(), ExitClass::StudyInvalid, "{what}");
    }
}
