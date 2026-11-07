//! A run whose mesh has a level, a grading and a shape, and whose result was made with turbulence,
//! time-dependent and buoyant physics, thermal walls and a force monitor, survives a save and an
//! open unchanged, and the file is written at the current number.

use std::fs;
use std::path::PathBuf;

use cfdcraft_doc::{Document, open, save};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::{
    InitMode, ResolvedBuoyancy, ResolvedForce, ResolvedGrading, ResolvedPhysics, ResolvedShape,
    ResolvedStudy, ResolvedSurface, ResolvedTime, ResolvedTurbulence, Study, SurfaceQuantity,
    TurbulenceModel, resolve,
};
use cfdcraft_runs::{
    Check, KeyNumbers, Outcome, Output, Provenance, RunRecord, StageRecipes, ToolIdentity,
    verify_runs,
};
use cfdcraft_store::ContentHash;
use serde_json::{Value, json};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("doc-physics-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

/// A closed cavity heated from the left and cooled from the right.
fn heated() -> Study {
    let mut v: Value = serde_json::from_str(CAVITY).unwrap();
    v["solver"]["solver"] = "FromPhysics".into();
    v["boundary_conditions"] = json!({
        "hot": {"faces": ["XMin"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": {"value": 300.5, "unit": "K"}}},
        "cold": {"faces": ["XMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": {"value": 299.5, "unit": "K"}}},
        "adiabatic": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "adiabatic"}}
    });
    v["physics"] = json!({"buoyancy": {
        "gravity": 9.81,
        "beta": 0.003,
        "reference_temperature": {"value": 300.0, "unit": "K"},
        "prandtl": 0.71,
        "turbulent_prandtl": 0.85
    }});
    Study::from_json(&v.to_string()).unwrap()
}

/// The heated cavity as resolved, with the members the recipes carry set on top: a mesh level, a
/// grading, a shape, transient k-omega SST physics, a heat flux and a force monitor.
fn full(study: &Study) -> ResolvedStudy {
    let mut r = resolve(study).unwrap();
    r.mesh.level = 2;
    r.mesh.grading = Some(ResolvedGrading {
        x: Some(1.2),
        y: None,
    });
    r.domain.shape = Some(ResolvedShape::BackwardStep {
        step_height: 0.01,
        upstream_length: 0.05,
        startup_length: 0.0,
    });
    r.physics = Some(ResolvedPhysics {
        time: ResolvedTime::Transient { delta_t: 0.001 },
        turbulence: ResolvedTurbulence {
            model: TurbulenceModel::KOmegaSst,
            intensity: Some(0.05),
            viscosity_ratio: Some(10.0),
            reference_velocity: Some(1.0),
        },
        buoyancy: Some(ResolvedBuoyancy {
            gravity: 9.81,
            beta: 0.003,
            reference_temperature: 300.0,
            prandtl: 0.71,
            turbulent_prandtl: 0.85,
        }),
        init: InitMode::Uniform,
    });
    r.monitors.surfaces = vec![ResolvedSurface {
        label: "flux".to_owned(),
        boundary: "hot".to_owned(),
        quantity: SurfaceQuantity::HeatFlux,
    }];
    r.monitors.forces = vec![ResolvedForce {
        label: "drag".to_owned(),
        boundary: "hot".to_owned(),
        reference_velocity: 1.0,
        reference_length: 0.1,
    }];
    r
}

/// A complete run of `study` whose recipes are made from `resolved`.
fn record(study: Study, resolved: &ResolvedStudy) -> RunRecord {
    let recipes = StageRecipes::from_resolved(resolved, &tool());
    let keys = recipes.keys();
    let mesh_hash = ContentHash::of(b"mesh");
    RunRecord {
        id: "run-1".to_owned(),
        rerun_of: None,
        scenario: None,
        mesh: Some(Output {
            key: keys.mesh,
            recipe: recipes.mesh.clone(),
            blob: mesh_hash,
            size: 4,
        }),
        result: Some(Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob: ContentHash::of(b"result"),
            size: 6,
        }),
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash,
            result_key: Some(keys.result),
            settings: study,
            tool: tool(),
            host: None,
        },
        checks: vec![Check {
            name: "residuals".to_owned(),
            passed: true,
            detail: "all below the limit".to_owned(),
        }],
        key_numbers: KeyNumbers::from([("solver.iterations".to_owned(), 120.0)]),
        comparison: None,
        continued_from: None,
        outcome: Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    }
}

#[test]
fn a_run_with_physics_and_a_shape_survives_save_and_open() {
    let study = heated();
    let resolved = full(&study);
    let mut doc = Document::new(study.clone());
    doc.add_run(record(study, &resolved)).unwrap();

    let path = fresh("round-trip").join("study.cfdcraft");
    save(&doc, &path).unwrap();
    let opened = open(&path).unwrap();

    assert_eq!(opened.document, doc);
    assert!(opened.notes.is_empty());
    assert_eq!(opened.schema_version, 5);
    assert_eq!(opened.schema_version, CURRENT_SCHEMA);
    verify_runs(&opened.document.artifacts.runs).unwrap();

    let run = &opened.document.artifacts.runs[0];
    let mesh = &run.mesh.as_ref().unwrap().recipe;
    assert_eq!(mesh.level, 2);
    assert!(mesh.grading.is_some());
    assert!(mesh.shape.is_some());
    assert!(mesh.patches.iter().any(|patch| patch.thermal.is_some()));
    let result = &run.result.as_ref().unwrap().recipe;
    assert!(result.physics.is_some());
    assert_eq!(result.monitors.forces.len(), 1);
    assert!(result.boundaries.values().any(|b| b.thermal.is_some()));
}

#[test]
fn a_plain_run_writes_none_of_the_new_members() {
    let study: Study = Study::from_json(CAVITY).unwrap();
    let resolved = resolve(&study).unwrap();
    let mut doc = Document::new(study.clone());
    doc.add_run(record(study, &resolved)).unwrap();

    let path = fresh("plain").join("study.cfdcraft");
    save(&doc, &path).unwrap();
    let opened = open(&path).unwrap();
    assert_eq!(opened.document, doc);

    let run = &opened.document.artifacts.runs[0];
    let mesh = &run.mesh.as_ref().unwrap().recipe;
    assert_eq!(mesh.level, 0);
    assert!(mesh.grading.is_none() && mesh.shape.is_none());
    assert!(run.result.as_ref().unwrap().recipe.physics.is_none());
}
