//! The shapes of the reports that describe variants, sweeps and disk use: their JSON names are
//! read by scripts and agents, so they are pinned here. Also the variant a run was made for.

use cfdcraft_diskuse::{DiskUse, PrunePlan, Removal};
use cfdcraft_keys::Change;
use cfdcraft_model::resolve;
use cfdcraft_ops::scenario_ops::effective_of;
use cfdcraft_ops::{
    DiffData, PointCheck, PreflightData, PruneData, RawView, Report, Repro, ScenarioData,
    ScenarioSummary, ScenariosData, Stage, SweepData, list_runs,
};
use cfdcraft_runs::{
    KeyNumbers, Output, Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id,
};
use cfdcraft_scenarios::{Override, Scenario};
use cfdcraft_store::ContentHash;
use serde_json::{Value, json};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

fn json_of(report: &Report) -> Value {
    serde_json::from_str(&report.to_json()).unwrap()
}

fn summary() -> ScenarioSummary {
    ScenarioSummary {
        id: "fast-lid".to_owned(),
        parent: Some("base".to_owned()),
        name: None,
        pinned: false,
        notes: None,
        overrides: 1,
        kind: "setting".to_owned(),
        sweep: None,
        mesh_key: None,
        result_key: None,
        mesh_shared_with: None,
    }
}

#[test]
fn a_report_with_a_diff_serialises_with_the_kind_scenario_diff() {
    let report = Report::new("scenario").with_repro(Repro::ScenarioDiff(Box::new(DiffData {
        from: "base".to_owned(),
        to: "fast-lid".to_owned(),
        same: false,
        changes: vec![Change::BoundaryVelocity {
            boundary: "lid".to_owned(),
            from: Some(0.1),
            to: Some(0.2),
        }],
        reasons: vec!["the lid speed went from 0.1 to 0.2".to_owned()],
        sentence: "fast-lid differs from base: the lid speed went from 0.1 to 0.2.".to_owned(),
    })));

    let json = json_of(&report);
    assert_eq!(json["repro"]["kind"], "scenario_diff");
    assert_eq!(json["repro"]["from"], "base");
    assert_eq!(json["repro"]["same"], false);
    assert_eq!(json["repro"]["changes"][0]["kind"], "boundary_velocity");
    let back: Report = serde_json::from_str(&report.to_json()).unwrap();
    assert_eq!(back, report);
}

#[test]
fn the_other_new_variants_have_snake_case_kinds() {
    let kind_of = |repro: Repro| {
        json_of(&Report::new("scenario").with_repro(repro))["repro"]["kind"]
            .as_str()
            .unwrap()
            .to_owned()
    };

    let one = ScenarioData {
        summary: summary(),
        overrides: vec![Override::Remove {
            setting: "/solver/convergence".to_owned(),
        }],
        raw_overrides: vec![RawView {
            id: "relax".to_owned(),
            backend: "esi".to_owned(),
            file: "system/fvSolution".to_owned(),
            entry: "relaxationFactors/fields/p".to_owned(),
            value: "0.3".to_owned(),
        }],
    };
    let sweep = SweepData {
        sweep: "lid".to_owned(),
        base: "base".to_owned(),
        stored: false,
        points: Vec::new(),
    };
    let preflight = PreflightData {
        sweep: "lid".to_owned(),
        points: vec![PointCheck {
            id: "lid-001".to_owned(),
            label: "lid 0.1".to_owned(),
            ok: true,
            reasons: Vec::new(),
            mesh_key: None,
            result_key: None,
            mesh_shared_with: None,
        }],
        distinct_meshes: 1,
        failed: 0,
    };
    let disk = DiskUse {
        scenarios: Vec::new(),
        stored_bytes: 0,
        pruned_bytes: 0,
    };

    assert_eq!(
        kind_of(Repro::Scenarios(Box::new(ScenariosData {
            scenarios: vec![summary()],
        }))),
        "scenarios"
    );
    assert_eq!(kind_of(Repro::Scenario(Box::new(one))), "scenario");
    assert_eq!(kind_of(Repro::Sweep(Box::new(sweep))), "sweep");
    assert_eq!(kind_of(Repro::Preflight(Box::new(preflight))), "preflight");
    assert_eq!(kind_of(Repro::Disk(Box::new(disk))), "disk");
    assert_eq!(
        kind_of(Repro::Prune(Box::new(PruneData {
            applied: false,
            plan: PrunePlan {
                remove: Vec::new(),
                freed_bytes: 0,
                kept_pinned: 0,
                kept_shared: 0,
            },
        }))),
        "prune"
    );
}

#[test]
fn a_scenario_summary_has_the_frozen_members() {
    let json = serde_json::to_value(summary()).unwrap();
    let mut names: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    let mut frozen = [
        "id",
        "parent",
        "name",
        "pinned",
        "notes",
        "overrides",
        "kind",
        "sweep",
        "mesh_key",
        "result_key",
        "mesh_shared_with",
    ];
    frozen.sort_unstable();
    assert_eq!(names, frozen);
    for absent in [
        "name",
        "notes",
        "sweep",
        "mesh_key",
        "result_key",
        "mesh_shared_with",
    ] {
        assert!(json[absent].is_null(), "{absent}");
    }
    assert_eq!(json["overrides"], 1);
    assert_eq!(json["pinned"], false);
}

#[test]
fn a_prune_report_carries_the_plan() {
    let report = Report::new("prune").with_repro(Repro::Prune(Box::new(PruneData {
        applied: false,
        plan: PrunePlan {
            remove: vec![Removal {
                hash: ContentHash::of(b"a result"),
                size: 8,
                stage: Stage::Result,
                scenarios: vec!["fast-lid".to_owned()],
            }],
            freed_bytes: 8,
            kept_pinned: 1,
            kept_shared: 2,
        },
    })));

    let json = json_of(&report);
    assert_eq!(json["repro"]["kind"], "prune");
    assert_eq!(json["repro"]["applied"], false);
    assert_eq!(json["repro"]["plan"]["freed_bytes"], 8);
    assert_eq!(json["repro"]["plan"]["kept_pinned"], 1);
    assert_eq!(json["repro"]["plan"]["kept_shared"], 2);
    assert_eq!(json["repro"]["plan"]["remove"][0]["size"], 8);
    assert_eq!(
        json["repro"]["plan"]["remove"][0]["scenarios"][0],
        "fast-lid"
    );
    let back: Report = serde_json::from_str(&report.to_json()).unwrap();
    assert_eq!(back, report);
}

/// Append a run that recorded only a mesh, made for the variant `tag` of the document at `path`.
/// Its blob is named after `blob`, so it differs from the blob of an untagged mesh-only run.
fn add_tagged_run(path: &std::path::Path, tag: &str, blob: &[u8]) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let (study, scenario) = effective_of("runs", &document, Some(tag)).unwrap();
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let mesh_hash = ContentHash::of(blob);
    let record = RunRecord {
        id: next_run_id(&document.artifacts.runs),
        rerun_of: None,
        scenario,
        mesh: Some(Output {
            key: keys.mesh,
            recipe: recipes.mesh.clone(),
            blob: mesh_hash,
            size: blob.len() as u64,
        }),
        result: None,
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash,
            result_key: None,
            settings: study,
            tool,
            host: None,
        },
        checks: Vec::new(),
        key_numbers: KeyNumbers::new(),
        comparison: None,
    };
    document.add_run(record).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
}

#[test]
fn a_run_summary_names_its_variant() {
    let dir = scratch("scenario-data-run-summary");
    let path = new_document(&dir, "cavity", CAVITY);
    add_mesh_only_run(&path);

    let mut document = cfdcraft_doc::open(&path).unwrap().document;
    document.artifacts.scenarios.push(Scenario {
        id: "fast-lid".to_owned(),
        parent: Some("base".to_owned()),
        name: None,
        pinned: false,
        notes: None,
        overrides: vec![Override::Set {
            setting: "/boundary_conditions/lid/velocity/value".to_owned(),
            value: json!(0.2),
        }],
        origin: None,
    });
    cfdcraft_doc::save(&document, &path).unwrap();
    add_tagged_run(&path, "fast-lid", b"a mesh made for a variant");

    let done = list_runs(&path);
    passed(&done.report);
    let Some(Repro::Runs(data)) = &done.report.repro else {
        panic!("expected the list of runs, found {:?}", done.report.repro);
    };
    let tags: Vec<Option<&str>> = data.runs.iter().map(|r| r.scenario.as_deref()).collect();
    assert_eq!(tags, [None, Some("fast-lid")]);

    let json = json_of(&done.report);
    assert!(json["repro"]["runs"][0]["scenario"].is_null());
    assert_eq!(json["repro"]["runs"][1]["scenario"], "fast-lid");
}
