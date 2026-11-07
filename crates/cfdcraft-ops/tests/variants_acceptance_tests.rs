//! The variant criteria end to end, through the operations: variants and variants of variants
//! store only their own changes and read as a diff in study terms (`sc1_`), a setting-only variant
//! reuses its parent's mesh while a geometry variant gets its own (`sc2_`), and a raw dictionary
//! entry is visible in the study and the diff, moves the keys and the staleness reason, reaches
//! the written case and is refused on another backend (`sc4_`). No test needs a compute host or a
//! container engine.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cfdcraft_backend_esi::{CaseTree, Dialect};
use cfdcraft_doc::{BlobKind, open, store_dir};
use cfdcraft_keys::Change;
use cfdcraft_ops::scenario_ops::{scenario_diff, scenario_list, scenario_set, scenario_show};
use cfdcraft_ops::{
    DiffData, DocOutcome, ExitClass, RawView, Repro, ScenarioData, ScenarioSummary, StatusData,
    document_status, prepare, read_study, save_document, scenario_status, solve_document,
    solve_scenario,
};
use cfdcraft_scenarios::{Override, effective_study};
use cfdcraft_store::Store;
use cfdcraft_text::Line;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";
const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";
const RELAX_U: &str = "esi system/fvSolution relaxationFactors/equations/U";

fn lid(value: f64) -> Override {
    Override::Set {
        setting: LID.to_owned(),
        value: json!(value),
    }
}

fn cells_x(count: u32) -> Override {
    Override::Set {
        setting: CELLS_X.to_owned(),
        value: json!(count),
    }
}

/// A raw dictionary entry for the backend `backend`, set as a change.
fn raw_for(label: &str, backend: &str, file: &str, entry: &str, value: &str) -> Override {
    Override::Set {
        setting: format!("/raw_overrides/{label}"),
        value: json!({ "backend": backend, "file": file, "entry": entry, "value": value }),
    }
}

fn raw(label: &str, file: &str, entry: &str, value: &str) -> Override {
    raw_for(label, "esi", file, entry, value)
}

/// The cavity with the relaxation factor of the velocity written as a raw entry.
fn cavity_relaxed(value: &str) -> String {
    edit(CAVITY, |study| {
        study["raw_overrides"] = json!({
            "relax-u": {
                "backend": "esi",
                "file": "system/fvSolution",
                "entry": "relaxationFactors/equations/U",
                "value": value,
            }
        });
    })
}

/// Make or change a variant and check that the operation passed.
fn make(path: &Path, id: &str, parent: Option<&str>, sets: &[Override]) {
    passed(&scenario_set(path, id, parent, sets, None).report);
}

/// A new document of the cavity with `fast-lid` (lid 0.2), `fine` (cells in x 256) and `fine-fast`
/// (below `fine`, lid 0.3).
fn branches(tag: &str) -> PathBuf {
    let path = new_document(&scratch(tag), "cavity", CAVITY);
    make(&path, "fast-lid", None, &[lid(0.2)]);
    make(&path, "fine", None, &[cells_x(256)]);
    make(&path, "fine-fast", Some("fine"), &[lid(0.3)]);
    path
}

fn rows(path: &Path) -> Vec<ScenarioSummary> {
    let out = scenario_list(path);
    passed(&out.report);
    match out.report.repro {
        Some(Repro::Scenarios(data)) => data.scenarios,
        other => panic!("expected the list of variants, found {other:?}"),
    }
}

fn row(rows: &[ScenarioSummary], id: &str) -> ScenarioSummary {
    rows.iter()
        .find(|found| found.id == id)
        .unwrap_or_else(|| panic!("no variant {id} in the list"))
        .clone()
}

fn diff_data(out: &DocOutcome) -> &DiffData {
    match &out.report.repro {
        Some(Repro::ScenarioDiff(data)) => data,
        other => panic!("expected the data of a diff, found {other:?}"),
    }
}

fn shown(out: &DocOutcome) -> &ScenarioData {
    match &out.report.repro {
        Some(Repro::Scenario(data)) => data,
        other => panic!("expected the data of a variant, found {other:?}"),
    }
}

fn stages(out: &DocOutcome) -> &StatusData {
    match &out.report.repro {
        Some(Repro::Status(data)) => data,
        other => panic!("expected the status, found {other:?}"),
    }
}

fn study_text(path: &Path) -> String {
    match read_study(path) {
        Ok(text) => text,
        Err(report) => panic!("{}", report.render_text()),
    }
}

fn sorted(mut items: Vec<String>) -> Vec<String> {
    items.sort();
    items
}

fn text<'a>(tree: &'a CaseTree, name: &str) -> &'a str {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap_or_else(|| panic!("the case has no {name}"));
    std::str::from_utf8(&file.bytes).unwrap()
}

// ---- success criterion 1: variants store only their changes, and a diff reads in study terms

#[test]
fn sc1_variants_and_variants_of_variants_store_only_their_overrides() {
    let path = new_document(&scratch("sc1-store"), "cavity", CAVITY);
    let saved = study_text(&path);

    make(&path, "fine", None, &[cells_x(256)]);
    make(&path, "fine-fast", Some("fine"), &[lid(0.3)]);

    let document = open(&path).unwrap().document;
    let held = &document.artifacts.scenarios;
    assert_eq!(held.len(), 2);
    assert_eq!(held[0].id, "fine");
    assert_eq!(held[0].parent.as_deref(), Some("base"));
    assert_eq!(held[0].overrides, vec![cells_x(256)]);
    assert_eq!(held[1].id, "fine-fast");
    assert_eq!(held[1].parent.as_deref(), Some("fine"));
    assert_eq!(held[1].overrides, vec![lid(0.3)]);

    // The study is the one that was saved, and no variant keeps a copy of it.
    assert_eq!(study_text(&path), saved);
    assert_eq!(
        scenario_list(&path).report.study_hash.as_deref(),
        Some(CAVITY_HASH)
    );
    assert!(document.artifacts.runs.is_empty());

    // Both changes apply, root to leaf, when the study of the variant is built.
    let built = effective_study(&document.study, held, "fine-fast").unwrap();
    let built = serde_json::to_value(&built).unwrap();
    assert_eq!(built["mesh"]["cells_x"], json!(256));
    assert_eq!(
        built["boundary_conditions"]["lid"]["velocity"]["value"],
        json!(0.3)
    );
    let own = effective_study(&document.study, held, "fine").unwrap();
    let own = serde_json::to_value(&own).unwrap();
    assert_eq!(own["mesh"]["cells_x"], json!(256));
    assert_eq!(
        own["boundary_conditions"]["lid"]["velocity"]["value"],
        json!(0.1)
    );
}

#[test]
fn sc1_a_diff_between_any_two_reads_in_study_terms() {
    let path = branches("sc1-diff");
    let cases: [(&str, &str, &[&str]); 4] = [
        (
            "base",
            "fine-fast",
            &["lid velocity 0.1 → 0.3 m/s", "cells in x 128 → 256"],
        ),
        (
            "fast-lid",
            "fine",
            &["lid velocity 0.2 → 0.1 m/s", "cells in x 128 → 256"],
        ),
        ("fine", "fine-fast", &["lid velocity 0.1 → 0.3 m/s"]),
        ("fine-fast", "fine", &["lid velocity 0.3 → 0.1 m/s"]),
    ];
    for (from, to, reasons) in cases {
        let out = scenario_diff(&path, from, to);
        passed(&out.report);
        let data = diff_data(&out);
        assert!(!data.same, "{from} to {to}");
        let wanted = reasons.iter().map(|r| (*r).to_owned()).collect();
        assert_eq!(
            sorted(data.reasons.clone()),
            sorted(wanted),
            "{from} to {to}"
        );
        assert_eq!(
            data.sentence,
            format!("{to} differs from {from}: {}.", data.reasons.join("; "))
        );
        assert_eq!(say(out.line.as_ref().unwrap()), data.sentence);
    }

    for id in ["base", "fine-fast"] {
        let out = scenario_diff(&path, id, id);
        passed(&out.report);
        let data = diff_data(&out);
        assert!(data.same && data.changes.is_empty() && data.reasons.is_empty());
        assert_eq!(data.sentence, format!("{id} and {id} make the same case."));
        assert_eq!(
            out.line,
            Some(Line::ScenarioSame {
                from: id.to_owned(),
                to: id.to_owned()
            })
        );
    }
}

// ---- success criterion 2: a setting-only variant reuses the mesh, a geometry variant has its own

/// A document of the cavity with `fast-lid` and `fine`, and a runner whose three runs differ.
fn with_two_variants(tag: &str) -> (PathBuf, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    make(&path, "fast-lid", None, &[lid(0.2)]);
    make(&path, "fine", None, &[cells_x(256)]);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = vec![0.0, 0.05, 0.1];
    (path, runner)
}

#[test]
fn sc2_a_setting_only_variant_reuses_the_parents_mesh_and_a_geometry_variant_gets_its_own() {
    let (path, mut runner) = with_two_variants("sc2-reuse");

    let study = solve_document(&path, &mut runner);
    passed(&study.report);
    let study = data(&study.report).clone();
    assert_eq!(study.recorded, ["mesh", "result"]);

    let fast = solve_scenario(&path, Some("fast-lid"), &mut runner);
    passed(&fast.report);
    let fast = data(&fast.report).clone();
    assert_eq!(fast.recorded, ["result"]);
    assert_eq!(fast.reused, ["mesh"]);
    assert_eq!(fast.mesh_key, study.mesh_key);
    assert_ne!(fast.result_key, study.result_key);

    let fine = solve_scenario(&path, Some("fine"), &mut runner);
    passed(&fine.report);
    let fine = data(&fine.report).clone();
    assert_eq!(fine.recorded, ["mesh", "result"]);
    assert!(fine.reused.is_empty());
    assert_ne!(fine.mesh_key, study.mesh_key);
    assert_eq!(runner.calls, 3);

    // The recorded runs say the same: no mesh for the setting-only variant, one for the other.
    let document = open(&path).unwrap().document;
    let runs = &document.artifacts.runs;
    assert_eq!(runs.len(), 3);
    assert!(runs[0].mesh.is_some() && runs[0].result.is_some());
    assert!(runs[1].mesh.is_none() && runs[1].result.is_some());
    assert!(runs[2].mesh.is_some() && runs[2].result.is_some());
    assert_eq!(runs[1].scenario.as_deref(), Some("fast-lid"));
    assert_eq!(runs[2].scenario.as_deref(), Some("fine"));
    assert_eq!(runs[1].provenance.mesh_key, runs[0].provenance.mesh_key);
    assert_eq!(
        runs[1].provenance.mesh_hash,
        runs[0].mesh.as_ref().unwrap().blob
    );
    assert_ne!(runs[2].provenance.mesh_key, runs[0].provenance.mesh_key);

    // And so do the stored blobs: one mesh shared by the study and `fast-lid`, one for `fine`, and
    // a result for each of the three runs.
    let blobs = &document.artifacts.blobs;
    assert_eq!(blobs.len(), 5);
    let meshes = blobs.iter().filter(|b| matches!(b.kind, BlobKind::Mesh));
    let results = blobs.iter().filter(|b| matches!(b.kind, BlobKind::Result));
    assert_eq!(meshes.count(), 2);
    assert_eq!(results.count(), 3);
    let store = Store::open(store_dir(&path)).unwrap();
    let files: BTreeSet<PathBuf> = blobs.iter().map(|b| store.blob_path(&b.hash)).collect();
    assert_eq!(files.len(), 5);
    for blob in blobs {
        assert!(store.contains(&blob.hash));
        assert!(store.blob_path(&blob.hash).is_file());
    }

    // The list says which variant stands on which mesh.
    let listed = rows(&path);
    let (base, fast_row, fine_row) = (
        row(&listed, "base"),
        row(&listed, "fast-lid"),
        row(&listed, "fine"),
    );
    assert_eq!(base.kind, "base");
    assert_eq!(fast_row.kind, "setting");
    assert_eq!(fast_row.mesh_shared_with.as_deref(), Some("base"));
    assert_eq!(fast_row.mesh_key, base.mesh_key);
    assert_eq!(fine_row.kind, "geometry");
    assert_eq!(fine_row.mesh_shared_with, None);
    assert_ne!(fine_row.mesh_key, base.mesh_key);
    assert_eq!(base.mesh_key.as_deref(), Some(study.mesh_key.as_str()));
}

#[test]
fn sc2_status_of_a_variant_says_what_is_reused() {
    let (path, mut runner) = with_two_variants("sc2-status");
    passed(&solve_document(&path, &mut runner).report);

    let before = scenario_status(&path, Some("fast-lid"));
    passed(&before.report);
    let found = stages(&before);
    assert_eq!(found.mesh.state, "fresh");
    assert_eq!(found.mesh.run.as_deref(), Some("run-1"));
    assert_eq!(found.result.state, "never_made");

    // The geometry variant has nothing of its own to stand on yet.
    let fine = scenario_status(&path, Some("fine"));
    assert_eq!(stages(&fine).mesh.state, "never_made");
    assert_eq!(stages(&fine).result.state, "never_made");

    passed(&solve_scenario(&path, Some("fast-lid"), &mut runner).report);
    let after = scenario_status(&path, Some("fast-lid"));
    passed(&after.report);
    let found = stages(&after);
    assert_eq!(found.mesh.state, "fresh");
    assert_eq!(found.mesh.run.as_deref(), Some("run-1"));
    assert_eq!(found.result.state, "fresh");
    assert_eq!(found.result.run.as_deref(), Some("run-2"));

    // The study itself is untouched by the variant's run.
    let base = document_status(&path);
    assert_eq!(stages(&base).result.state, "fresh");
    assert_eq!(stages(&base).result.run.as_deref(), Some("run-1"));
}

// ---- success criterion 4: raw overrides

#[test]
fn sc4_a_raw_override_is_shown_in_the_study_in_the_diff_and_in_the_key() {
    let dir = scratch("sc4-key");
    let plain = new_document(&dir, "plain", CAVITY);
    let tuned = new_document(&dir, "tuned", &cavity_relaxed("0.5"));

    // It is in the study, as the variant list shows it.
    let out = scenario_show(&tuned, "base");
    passed(&out.report);
    assert_eq!(
        shown(&out).raw_overrides,
        vec![RawView {
            id: "relax-u".to_owned(),
            backend: "esi".to_owned(),
            file: "system/fvSolution".to_owned(),
            entry: "relaxationFactors/equations/U".to_owned(),
            value: "0.5".to_owned(),
        }]
    );
    let plain_out = scenario_show(&plain, "base");
    assert!(shown(&plain_out).raw_overrides.is_empty());

    // It changes the key of the result and leaves the mesh and the geometry alone.
    let (with, without) = (row(&rows(&tuned), "base"), row(&rows(&plain), "base"));
    assert!(with.result_key.is_some() && without.result_key.is_some());
    assert_ne!(with.result_key, without.result_key);
    assert_eq!(with.mesh_key, without.mesh_key);

    // A variant that changes it shows the change in the diff.
    let value = Override::Set {
        setting: "/raw_overrides/relax-u/value".to_owned(),
        value: json!("0.7"),
    };
    make(&tuned, "relaxed-more", None, &[value]);
    let out = scenario_diff(&tuned, "base", "relaxed-more");
    passed(&out.report);
    let data = diff_data(&out);
    let was = format!("{RELAX_U} = 0.5");
    let now = format!("{RELAX_U} = 0.7");
    assert_eq!(
        data.changes,
        vec![Change::RawOverride {
            id: "relax-u".to_owned(),
            from: Some(was.clone()),
            to: Some(now.clone()),
        }]
    );
    assert_eq!(
        data.reasons,
        [format!("raw override relax-u {was} → {now}")]
    );
    let listed = rows(&tuned);
    let moved = row(&listed, "relaxed-more");
    assert_eq!(moved.mesh_key, with.mesh_key);
    assert_ne!(moved.result_key, with.result_key);
    assert_eq!(moved.kind, "setting");
}

#[test]
fn sc4_a_raw_override_changes_the_staleness_reason() {
    let dir = scratch("sc4-stale");
    let path = new_document(&dir, "tuned", &cavity_relaxed("0.5"));
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&path, &mut runner).report);
    let fresh = document_status(&path);
    assert_eq!(stages(&fresh).result.state, "fresh");

    passed(&save_document(&cavity_relaxed("0.7"), &path, true).report);

    let out = document_status(&path);
    passed(&out.report);
    let found = stages(&out);
    assert_eq!(found.mesh.state, "fresh");
    assert_eq!(found.result.state, "stale");
    assert!(
        found
            .result
            .reasons
            .iter()
            .any(|reason| reason.contains("raw override relax-u")),
        "{:?}",
        found.result.reasons
    );
    assert!(
        found.result.sentence.contains("raw override relax-u"),
        "{}",
        found.result.sentence
    );
    assert!(
        found
            .result
            .changes
            .iter()
            .any(|change| matches!(change, Change::RawOverride { id, .. } if id == "relax-u"))
    );
}

#[test]
fn sc4_a_raw_override_reaches_the_written_case() {
    let relaxed = prepare(&cavity_relaxed("0.5"), Dialect::EsiV2512).unwrap();
    let plain = prepare(CAVITY, Dialect::EsiV2512).unwrap();

    assert!(text(&relaxed.tree, "system/fvSolution").contains("U 0.5;"));
    assert!(text(&relaxed.export_tree, "system/fvSolution").contains("U 0.5;"));
    assert!(!text(&relaxed.tree, "system/fvSolution").contains("U 0.7;"));
    assert!(text(&plain.tree, "system/fvSolution").contains("U 0.7;"));
    assert!(text(&plain.export_tree, "system/fvSolution").contains("U 0.7;"));
    assert!(!text(&plain.tree, "system/fvSolution").contains("U 0.5;"));
    assert_ne!(relaxed.hash, plain.hash);
}

#[test]
fn sc4_a_raw_override_on_a_mesh_dictionary_is_part_of_the_mesh_key() {
    let path = new_document(&scratch("sc4-mesh"), "cavity", CAVITY);
    make(
        &path,
        "meshy",
        None,
        &[raw("blk", "system/blockMeshDict", "scale", "1")],
    );
    make(
        &path,
        "relaxed",
        None,
        &[raw(
            "relax-u",
            "system/fvSolution",
            "relaxationFactors/equations/U",
            "0.5",
        )],
    );

    let listed = rows(&path);
    let (base, meshy, relaxed) = (
        row(&listed, "base"),
        row(&listed, "meshy"),
        row(&listed, "relaxed"),
    );
    assert!(meshy.mesh_key.is_some() && meshy.result_key.is_some());
    assert_ne!(meshy.mesh_key, base.mesh_key);
    assert_ne!(meshy.result_key, base.result_key);
    assert_eq!(meshy.kind, "geometry");
    // An entry in a solver dictionary leaves the mesh alone.
    assert_eq!(relaxed.mesh_key, base.mesh_key);
    assert_eq!(relaxed.kind, "setting");

    let out = scenario_diff(&path, "base", "meshy");
    passed(&out.report);
    let data = diff_data(&out);
    assert!(
        data.changes.contains(&Change::RawMesh),
        "{:?}",
        data.changes
    );
    assert!(
        data.changes
            .iter()
            .any(|change| matches!(change, Change::RawOverride { id, .. } if id == "blk"))
    );
}

#[test]
fn sc4_a_raw_override_for_another_backend_is_refused_and_never_ignored() {
    let dir = scratch("sc4-refused");
    let path = new_document(&dir, "cavity", CAVITY);
    let foreign = raw_for(
        "other",
        "foundation",
        "system/fvSolution",
        "SIMPLE/consistent",
        "yes",
    );
    let ghost = raw("ghost", "system/fvOptions", "limits/x", "1");
    let sneaky = raw(
        "sneaky",
        "system/fvSolution",
        "SIMPLE/consistent",
        "yes #include",
    );
    make(&path, "foreign", None, &[foreign]);
    make(&path, "ghosted", None, &[ghost]);
    make(&path, "sneaky", None, &[sneaky]);
    let mut runner = FixtureRunner::new(&dir.join("runs"));

    // Another backend: shown with the finding, refused on a solve.
    let out = scenario_show(&path, "foreign");
    assert!(
        codes(&out.report).contains(&"CFD-RAW-005"),
        "{}",
        out.report.render_text()
    );
    assert_eq!(shown(&out).raw_overrides.len(), 1);
    assert_eq!(shown(&out).raw_overrides[0].backend, "foundation");
    let out = solve_scenario(&path, Some("foreign"), &mut runner);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert!(codes(&out.report).contains(&"CFD-RAW-005"));
    assert!(!codes(&out.report).contains(&"CFD-RAW-007"));

    // A file the written case does not have.
    let out = solve_scenario(&path, Some("ghosted"), &mut runner);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert!(codes(&out.report).contains(&"CFD-RAW-007"));

    // A value that tries to include something.
    let out = solve_scenario(&path, Some("sneaky"), &mut runner);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert!(codes(&out.report).contains(&"CFD-RAW-004"));

    // None of them started a case or recorded anything.
    assert_eq!(runner.calls, 0);
    assert!(open(&path).unwrap().document.artifacts.runs.is_empty());
    assert!(tree(&store_dir(&path)).is_empty());
}
