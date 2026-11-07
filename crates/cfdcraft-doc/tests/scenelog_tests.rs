//! Variants, sweeps and the pruned list in a file: they save and open equal, older files are
//! brought forward, a list that breaks a rule is refused on read and on write, and adding a run
//! keeps the run tags and the pruned list in order. Every archive here is built straight with the
//! ZIP crate.

use std::io::{Cursor, Read, Write};

use cfdcraft_doc::{
    ArtifactIndex, BlobKind, BlobRef, DocError, Document, RunLogError, check_runs,
    check_scenario_records, from_bytes, to_bytes,
};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::{Study, resolve};
use cfdcraft_runs::{
    Check, KeyNumbers, Output, Provenance, RunRecord, Stage, StageRecipes, ToolIdentity,
};
use cfdcraft_scenarios::{Origin, Override, Scenario};
use cfdcraft_store::ContentHash;
use cfdcraft_sweep::{Parameter, Sweep, Target, Values};
use serde_json::{Value, json};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const DEFLATE: CompressionMethod = CompressionMethod::Deflated;
const LID: &str = "/boundary_conditions/lid/velocity/value";

fn cavity() -> Study {
    Study::from_json(CAVITY).unwrap()
}

/// The cavity with a faster lid: the same mesh, another result.
fn faster_lid() -> Study {
    let mut study = cavity();
    study
        .boundary_conditions
        .get_mut("lid")
        .unwrap()
        .velocity
        .as_mut()
        .unwrap()
        .value = 0.2;
    study
}

fn archive(entries: Vec<(&str, Vec<u8>, CompressionMethod)>) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, method) in entries {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
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

fn member(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

/// A file at `version` whose artifact index is `artifacts`.
fn file_with_artifacts(version: u32, artifacts: &Value) -> Vec<u8> {
    archive(vec![
        ("manifest.json", manifest(version), DEFLATE),
        ("study.json", CAVITY.as_bytes().to_vec(), DEFLATE),
        (
            "artifacts.json",
            artifacts.to_string().into_bytes(),
            DEFLATE,
        ),
    ])
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn mesh_hash() -> ContentHash {
    ContentHash::of(b"mesh")
}

/// A run of `study` that recorded both stages; its result blob is named after the run.
fn record_for(id: &str, study: &Study) -> RunRecord {
    let recipes = StageRecipes::from_resolved(&resolve(study).unwrap(), &tool());
    let keys = recipes.keys();
    RunRecord {
        id: id.to_owned(),
        rerun_of: None,
        scenario: None,
        mesh: Some(Output {
            key: keys.mesh,
            recipe: recipes.mesh.clone(),
            blob: mesh_hash(),
            size: 4,
        }),
        result: Some(Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob: ContentHash::of(id.as_bytes()),
            size: 6,
        }),
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash: mesh_hash(),
            result_key: Some(keys.result),
            settings: study.clone(),
            tool: tool(),
            host: Some("box".to_owned()),
        },
        checks: vec![Check {
            name: "residuals".to_owned(),
            passed: true,
            detail: "all below the limit".to_owned(),
        }],
        key_numbers: KeyNumbers::from([("solver.iterations".to_owned(), 120.0)]),
        comparison: None,
        continued_from: None,
        outcome: cfdcraft_runs::Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    }
}

/// A variant of `parent` that sets the lid speed.
fn variant(id: &str, parent: &str) -> Scenario {
    Scenario {
        id: id.to_owned(),
        parent: Some(parent.to_owned()),
        name: None,
        pinned: false,
        notes: None,
        overrides: vec![Override::Set {
            setting: LID.to_owned(),
            value: json!(0.2),
        }],
        origin: None,
    }
}

/// A sweep of the lid speed over three values.
fn lid_sweep(id: &str, base: &str) -> Sweep {
    Sweep {
        id: id.to_owned(),
        base: base.to_owned(),
        parameters: vec![Parameter {
            name: "lid_speed".to_owned(),
            target: Target::Quantity {
                setting: "/boundary_conditions/lid/velocity".to_owned(),
                unit: "m/s".to_owned(),
            },
            values: Values::List {
                items: vec![0.1, 0.2, 0.3],
            },
        }],
    }
}

/// An index that lists the stored results `run` names, so only the new rules can refuse it.
fn index_with_run(run: RunRecord) -> ArtifactIndex {
    let blobs = run
        .blob_uses()
        .into_iter()
        .map(|used| BlobRef {
            hash: used.hash,
            size: used.size,
            kind: match used.stage {
                Stage::Mesh => BlobKind::Mesh,
                Stage::Result => BlobKind::Result,
            },
        })
        .collect();
    ArtifactIndex {
        blobs,
        runs: vec![run],
        ..ArtifactIndex::default()
    }
}

fn stored(hash: ContentHash) -> BlobRef {
    BlobRef {
        hash,
        size: 1,
        kind: BlobKind::Result,
    }
}

fn of(text: &str) -> ContentHash {
    ContentHash::of(text.as_bytes())
}

fn scenarios_error(error: DocError) -> String {
    match error {
        DocError::Scenarios(reason) => reason,
        other => panic!("expected a refusal of the variant records, got {other}"),
    }
}

/// The reason `index` is refused with, after checking that writing and reading give the same one.
fn refused(index: &ArtifactIndex) -> String {
    let mut doc = Document::new(cavity());
    doc.artifacts = index.clone();
    let on_write = scenarios_error(to_bytes(&doc).unwrap_err());
    let bytes = file_with_artifacts(CURRENT_SCHEMA, &serde_json::to_value(index).unwrap());
    let on_read = scenarios_error(from_bytes(&bytes).unwrap_err());
    assert_eq!(on_write, on_read);
    on_write
}

/// `index` saves and opens equal.
fn accepted(index: &ArtifactIndex) {
    let mut doc = Document::new(cavity());
    doc.artifacts = index.clone();
    let opened = from_bytes(&to_bytes(&doc).unwrap()).unwrap();
    assert_eq!(opened.document, doc);
    // a hand-built file has no thumbnail member, so it reads back without one
    let bytes = file_with_artifacts(CURRENT_SCHEMA, &serde_json::to_value(index).unwrap());
    let mut without_thumbnail = doc.clone();
    without_thumbnail.thumbnail = None;
    assert_eq!(from_bytes(&bytes).unwrap().document, without_thumbnail);
}

// ---- the stored members and the schema steps

#[test]
fn a_document_with_a_variant_round_trips_through_its_bytes() {
    let mut doc = Document::new(cavity());
    doc.artifacts.scenarios.push(variant("fast-lid", "base"));
    let bytes = to_bytes(&doc).unwrap();

    let opened = from_bytes(&bytes).unwrap();
    assert_eq!(opened.document, doc);
    assert!(opened.notes.is_empty());
    let artifacts = String::from_utf8(member(&bytes, "artifacts.json")).unwrap();
    assert!(artifacts.contains("scenarios"), "{artifacts}");
}

#[test]
fn a_document_without_variants_writes_none_of_the_new_members() {
    let mut doc = Document::new(cavity());
    doc.add_run(record_for("run-1", &cavity())).unwrap();
    let artifacts = String::from_utf8(member(&to_bytes(&doc).unwrap(), "artifacts.json")).unwrap();
    assert!(artifacts.contains("\"runs\""), "{artifacts}");
    for absent in ["scenarios", "sweeps", "pruned"] {
        assert!(!artifacts.contains(absent), "{absent}: {artifacts}");
    }
}

#[test]
fn a_schema_2_file_opens_with_three_notes_and_no_variants() {
    let bytes = file_with_artifacts(2, &json!({"blobs": [], "links": [], "runs": []}));
    let opened = from_bytes(&bytes).unwrap();

    assert_eq!(opened.schema_version, 2);
    assert_eq!(opened.notes.len(), 3);
    assert_eq!((opened.notes[0].from, opened.notes[0].to), (2, 3));
    assert!(opened.notes[0].summary.contains("variants"));
    assert_eq!((opened.notes[1].from, opened.notes[1].to), (3, 4));
    assert_eq!((opened.notes[2].from, opened.notes[2].to), (4, 5));
    let artifacts = &opened.document.artifacts;
    assert!(artifacts.scenarios.is_empty());
    assert!(artifacts.sweeps.is_empty());
    assert!(artifacts.pruned.is_empty());
    assert_eq!(opened.document.study, cavity());
}

#[test]
fn a_schema_1_file_opens_with_four_notes() {
    let bytes = file_with_artifacts(1, &json!({"blobs": [], "links": []}));
    let opened = from_bytes(&bytes).unwrap();

    assert_eq!(opened.schema_version, 1);
    let pairs: Vec<(u32, u32)> = opened.notes.iter().map(|n| (n.from, n.to)).collect();
    assert_eq!(pairs, vec![(1, 2), (2, 3), (3, 4), (4, 5)]);
    assert!(opened.document.artifacts.scenarios.is_empty());
}

#[test]
fn a_schema_0_file_with_no_index_opens_with_five_notes() {
    let bytes = archive(vec![
        ("manifest.json", manifest(0), DEFLATE),
        ("study.json", CAVITY_V0.as_bytes().to_vec(), DEFLATE),
    ]);
    let opened = from_bytes(&bytes).unwrap();
    assert_eq!(opened.notes.len(), 5);
    assert_eq!(opened.document.artifacts, ArtifactIndex::default());
}

// ---- the rules, on read and on write

#[test]
fn a_list_that_breaks_a_rule_is_refused_on_read_and_on_write() {
    let index = ArtifactIndex {
        scenarios: vec![variant("child", "parent"), variant("parent", "base")],
        ..ArtifactIndex::default()
    };
    let reason = refused(&index);
    assert!(
        reason.contains("child") && reason.contains("parent"),
        "{reason}"
    );

    // The same variants in order pass.
    accepted(&ArtifactIndex {
        scenarios: vec![variant("parent", "base"), variant("child", "parent")],
        ..ArtifactIndex::default()
    });
}

#[test]
fn a_sweep_must_name_an_existing_base_and_pass_its_own_check() {
    let with = |sweeps: Vec<Sweep>, scenarios: Vec<Scenario>| ArtifactIndex {
        sweeps,
        scenarios,
        ..ArtifactIndex::default()
    };

    // A base that is the study, or a listed variant, passes.
    accepted(&with(vec![lid_sweep("lid-sweep", "base")], Vec::new()));
    accepted(&with(
        vec![lid_sweep("lid-sweep", "fast-lid")],
        vec![variant("fast-lid", "base")],
    ));

    let reason = refused(&with(vec![lid_sweep("lid-sweep", "nope")], Vec::new()));
    assert!(
        reason.contains("lid-sweep") && reason.contains("nope"),
        "{reason}"
    );

    let reason = refused(&with(
        vec![
            lid_sweep("lid-sweep", "base"),
            lid_sweep("lid-sweep", "base"),
        ],
        Vec::new(),
    ));
    assert!(reason.contains("twice"), "{reason}");

    let mut empty = lid_sweep("lid-sweep", "base");
    empty.parameters.clear();
    let reason = refused(&with(vec![empty], Vec::new()));
    assert!(reason.contains("no parameter"), "{reason}");

    let many: Vec<Sweep> = (0..201)
        .map(|n| lid_sweep(&format!("s{n}"), "base"))
        .collect();
    let reason = refused(&with(many, Vec::new()));
    assert!(reason.contains("201"), "{reason}");
}

#[test]
fn a_variant_origin_must_name_a_sweep_and_one_of_its_points() {
    let from_point = |sweep: &str, index: u32| {
        let mut scenario = variant("lid-sweep-001", "base");
        scenario.origin = Some(Origin {
            sweep: sweep.to_owned(),
            index,
        });
        ArtifactIndex {
            sweeps: vec![lid_sweep("lid-sweep", "base")],
            scenarios: vec![scenario],
            ..ArtifactIndex::default()
        }
    };

    accepted(&from_point("lid-sweep", 0));
    accepted(&from_point("lid-sweep", 2));

    let reason = refused(&from_point("ghost", 0));
    assert!(reason.contains("ghost"), "{reason}");

    // The sweep has three points, numbered from 0.
    let reason = refused(&from_point("lid-sweep", 3));
    assert!(reason.contains('3'), "{reason}");
}

#[test]
fn a_run_must_name_an_existing_variant() {
    let tagged = |tag: Option<&str>| {
        let mut run = record_for("run-1", &cavity());
        run.scenario = tag.map(str::to_owned);
        run
    };

    // An untagged run needs no variant at all.
    accepted(&index_with_run(tagged(None)));

    let reason = refused(&index_with_run(tagged(Some("nope"))));
    assert!(
        reason.contains("run-1") && reason.contains("nope"),
        "{reason}"
    );

    let mut listed = index_with_run(tagged(Some("fast-lid")));
    listed.scenarios.push(variant("fast-lid", "base"));
    accepted(&listed);
}

#[test]
fn the_pruned_list_must_be_sorted_unique_and_listed() {
    let mut hashes = vec![of("a"), of("b"), of("c")];
    hashes.sort();
    let listing = |pruned: Vec<ContentHash>| ArtifactIndex {
        blobs: hashes.iter().copied().map(stored).collect(),
        pruned,
        ..ArtifactIndex::default()
    };

    accepted(&listing(hashes.clone()));
    accepted(&listing(vec![hashes[0], hashes[2]]));

    let reason = refused(&listing(vec![hashes[1], hashes[0]]));
    assert!(reason.contains("sorted"), "{reason}");

    let reason = refused(&listing(vec![hashes[0], hashes[0]]));
    assert!(
        reason.contains("twice") && reason.contains(&hashes[0].to_string()),
        "{reason}"
    );

    let unlisted = of("never stored");
    let reason = refused(&listing(vec![unlisted]));
    assert!(reason.contains(&unlisted.to_string()), "{reason}");
}

#[test]
fn the_messages_never_name_a_path() {
    let hostile = "../../etc/passwd";

    let out_of_order = refused(&ArtifactIndex {
        scenarios: vec![variant("child", "parent"), variant("parent", "base")],
        ..ArtifactIndex::default()
    });
    let odd_base = refused(&ArtifactIndex {
        sweeps: vec![lid_sweep("lid-sweep", hostile)],
        ..ArtifactIndex::default()
    });
    let mut from_ghost = variant("lid-sweep-001", "base");
    from_ghost.origin = Some(Origin {
        sweep: hostile.to_owned(),
        index: 0,
    });
    let odd_origin = refused(&ArtifactIndex {
        scenarios: vec![from_ghost],
        ..ArtifactIndex::default()
    });
    let unlisted = refused(&ArtifactIndex {
        pruned: vec![of("never stored")],
        ..ArtifactIndex::default()
    });
    let mut run = record_for("run-1", &cavity());
    run.scenario = Some("nope".to_owned());
    let unknown_tag = refused(&index_with_run(run));
    let reasons = [out_of_order, odd_base, odd_origin, unlisted, unknown_tag];

    for reason in &reasons {
        assert!(
            !reason.contains('/') && !reason.contains('\\') && !reason.contains(".."),
            "{reason}"
        );
        assert!(!reason.contains("passwd"), "{reason}");
    }
}

#[test]
fn an_index_with_none_of_the_new_members_passes_and_the_run_checks_still_hold() {
    assert_eq!(check_scenario_records(&ArtifactIndex::default()), Ok(()));

    // A history that names a result the index does not list is still refused by its own rule.
    let mut bad = Document::new(cavity());
    bad.artifacts.runs.push(record_for("run-1", &cavity()));
    assert!(check_runs(&bad.artifacts).is_err());
    assert!(matches!(to_bytes(&bad), Err(DocError::RunLog(_))));

    let mut good = Document::new(cavity());
    good.add_run(record_for("run-1", &cavity())).unwrap();
    assert_eq!(
        from_bytes(&to_bytes(&good).unwrap()).unwrap().document,
        good
    );
}

// ---- adding a run

#[test]
fn adding_a_run_for_an_unknown_variant_is_refused() {
    let mut doc = Document::new(cavity());
    doc.add_run(record_for("run-1", &cavity())).unwrap();
    let before = doc.clone();

    let mut run = record_for("run-2", &faster_lid());
    run.scenario = Some("fast-lid".to_owned());
    let error = doc.add_run(run).unwrap_err();

    assert_eq!(
        error,
        RunLogError::UnknownScenario {
            scenario: "fast-lid".to_owned()
        }
    );
    assert_eq!(doc, before);
}

#[test]
fn adding_a_run_for_a_listed_variant_works() {
    let mut doc = Document::new(cavity());
    doc.artifacts.scenarios.push(variant("fast-lid", "base"));
    let mut run = record_for("run-1", &faster_lid());
    let result = run.result.as_ref().unwrap().blob;
    run.scenario = Some("fast-lid".to_owned());
    doc.add_run(run).unwrap();

    assert_eq!(doc.artifacts.runs.len(), 1);
    let listed: Vec<ContentHash> = doc.artifacts.blobs.iter().map(|b| b.hash).collect();
    assert!(listed.contains(&mesh_hash()), "{listed:?}");
    assert!(listed.contains(&result), "{listed:?}");
    assert_eq!(from_bytes(&to_bytes(&doc).unwrap()).unwrap().document, doc);
}

/// A document with one run, plus the hash of that run's result and one more stored result, both
/// on the pruned list.
fn document_with_pruned_results() -> (Document, ContentHash, ContentHash) {
    let mut doc = Document::new(cavity());
    doc.add_run(record_for("run-1", &cavity())).unwrap();
    let gone = ContentHash::of(b"run-1");
    let other = of("other");
    doc.artifacts.blobs.push(stored(other));
    doc.artifacts.pruned = vec![gone, other];
    doc.artifacts.pruned.sort();
    // The first run's own result is a blob of size 6; it is listed already.
    assert!(doc.artifacts.blobs.iter().any(|b| b.hash == gone));
    (doc, gone, other)
}

#[test]
fn storing_a_result_again_takes_it_off_the_pruned_list() {
    let (mut doc, gone, other) = document_with_pruned_results();

    // Another run stores the same result (same size, same kind).
    let mut again = record_for("run-2", &faster_lid());
    again.result.as_mut().unwrap().blob = gone;
    doc.add_run(again).unwrap();

    assert_eq!(doc.artifacts.pruned, vec![other]);
    assert_eq!(from_bytes(&to_bytes(&doc).unwrap()).unwrap().document, doc);
}

#[test]
fn a_run_that_names_a_pruned_hash_with_another_size_is_still_a_conflict() {
    let (mut doc, gone, _) = document_with_pruned_results();
    let before = doc.clone();

    let mut again = record_for("run-2", &faster_lid());
    let result = again.result.as_mut().unwrap();
    result.blob = gone;
    result.size = 7;

    assert_eq!(
        doc.add_run(again).unwrap_err(),
        RunLogError::BlobConflict(gone)
    );
    assert_eq!(doc, before);
}

#[test]
fn room_for_another_run_counts_the_new_members() {
    // A small document has room, with or without variants.
    let mut small = Document::new(cavity());
    small.add_run(record_for("run-1", &cavity())).unwrap();
    assert!(small.check_room_for_run().is_ok());
    for n in 0..200 {
        let mut scenario = variant(&format!("v{n}"), "base");
        scenario.notes = Some("x".repeat(2000));
        small.artifacts.scenarios.push(scenario);
    }
    assert!(small.check_room_for_run().is_ok());

    // A document whose index is already past the size limit has none, and more variants never
    // give it room.
    let mut full = Document::new(cavity());
    full.artifacts.blobs = (0..300_000u32)
        .map(|n| stored(ContentHash::of(&n.to_le_bytes())))
        .collect();
    assert!(matches!(
        full.check_room_for_run(),
        Err(RunLogError::HistoryFull { .. })
    ));
    full.artifacts.scenarios.push(variant("fast-lid", "base"));
    assert!(matches!(
        full.check_room_for_run(),
        Err(RunLogError::HistoryFull { .. })
    ));
}
