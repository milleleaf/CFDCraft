//! Mesh independence on a document: the level sweep, the report read from recorded key numbers,
//! the refusals, and the warnings. No run is made: the runs of each point are written into the
//! document as records, as a solve would leave them.

use std::path::{Path, PathBuf};

use cfdcraft_gci::{Convergence, GciReport, Warning};
use cfdcraft_model::resolve;
use cfdcraft_ops::gci_ops::{GciRequest, gci_create, gci_report, gci_warning_lines};
use cfdcraft_ops::scenario_ops::effective_of;
use cfdcraft_ops::sweep_ops::sweep_create;
use cfdcraft_ops::{DocOutcome, ExitClass, GciData, Report, Repro};
use cfdcraft_runs::{Output, Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id};
use cfdcraft_scenarios::Override;
use cfdcraft_store::ContentHash;
use cfdcraft_sweep::{Parameter, Sweep, Target, Values};
use cfdcraft_text::Line;
use serde_json::{Value, json};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::{CAVITY, codes, edit, new_document, passed, say, scratch};

const SWEEP: &str = "gci-cavity";
const QUANTITY: &str = "test.quantity";
const POINTS: [&str; 3] = ["gci-cavity-001", "gci-cavity-002", "gci-cavity-003"];
/// The three values of the published worked example, coarse to fine.
const NASA: [f64; 3] = [0.96178, 0.96854, 0.97050];

/// The cavity with 32 cells along each side, so levels 0, 1 and 2 make 32, 64 and 128.
fn coarse_cavity() -> String {
    edit(CAVITY, |study| {
        study["mesh"]["cells_x"] = json!(32);
        study["mesh"]["cells_y"] = json!(32);
    })
}

/// A new document of `study` with the level sweep stored in it.
fn with_sweep(tag: &str, study: &str) -> PathBuf {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", study);
    passed(&gci_create(&path, &GciRequest::new(SWEEP)).report);
    path
}

/// Append a run made for the variant `scenario`. With `Some(numbers)` it recorded a result holding
/// those key numbers; with `None` it recorded only a mesh. Returns the id of the run.
fn record(
    path: &Path,
    scenario: &str,
    numbers: Option<&[(&str, f64)]>,
    rerun_of: Option<&str>,
) -> String {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let (study, tag) = effective_of("gci", &document, Some(scenario)).unwrap();
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let id = next_run_id(&document.artifacts.runs);
    let bytes = format!("output of {id}").into_bytes();
    let hash = ContentHash::of(&bytes);
    let size = bytes.len() as u64;
    let (mesh, result, mesh_hash) = match numbers {
        None => (
            Some(Output {
                key: keys.mesh,
                recipe: recipes.mesh.clone(),
                blob: hash,
                size,
            }),
            None,
            hash,
        ),
        Some(_) => (
            None,
            Some(Output {
                key: keys.result,
                recipe: recipes.result.clone(),
                blob: hash,
                size,
            }),
            ContentHash::of(b"a mesh made earlier"),
        ),
    };
    let record = RunRecord {
        id: id.clone(),
        rerun_of: rerun_of.map(str::to_owned),
        scenario: tag,
        mesh,
        result,
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash,
            result_key: numbers.map(|_| keys.result),
            settings: study,
            tool,
            host: None,
        },
        checks: Vec::new(),
        key_numbers: numbers
            .unwrap_or_default()
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect(),
        comparison: None,
        continued_from: None,
        outcome: cfdcraft_runs::Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    };
    document.add_run(record).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
    id
}

/// Record one run for each point of the level sweep, in the order `order` names the points, with
/// the value of `QUANTITY` that `values` gives for that point (coarse to fine).
fn record_all(path: &Path, values: [f64; 3], order: [usize; 3]) {
    for at in order {
        record(path, POINTS[at], Some(&[(QUANTITY, values[at])]), None);
    }
}

fn gci_of(outcome: &DocOutcome) -> &GciData {
    match &outcome.report.repro {
        Some(Repro::Gci(data)) => data.as_ref(),
        other => panic!("expected the data of a mesh independence report, found {other:?}"),
    }
}

fn finest(outcome: &DocOutcome) -> &GciReport {
    gci_of(outcome)
        .reports
        .last()
        .expect("one analysis at least")
}

/// `found` is within the relative distance `relative` of `expected`.
fn close(found: Option<f64>, expected: f64, relative: f64) {
    let found = found.expect("the report holds a number here");
    assert!(
        ((found - expected) / expected).abs() <= relative,
        "{found} is not within {relative} of {expected}"
    );
}

fn refused_with(outcome: &DocOutcome, code: &str) {
    assert_eq!(outcome.report.class(), ExitClass::StudyInvalid);
    assert_eq!(outcome.report.exit_class, 2);
    assert_eq!(
        codes(&outcome.report),
        [code],
        "{}",
        outcome.report.render_text()
    );
    assert_eq!(outcome.line, None);
}

fn message(outcome: &DocOutcome) -> &str {
    &outcome.report.diagnostics[0].message
}

fn waiting(missing: &str) -> String {
    say(&Line::GciWaiting {
        sweep: SWEEP.to_owned(),
        missing: missing.to_owned(),
    })
}

#[test]
fn gci_create_stores_a_three_point_level_sweep() {
    let dir = scratch("gci-create");
    let path = new_document(&dir, "cavity", &coarse_cavity());

    let out = gci_create(&path, &GciRequest::new(SWEEP));
    passed(&out.report);
    assert_eq!(
        out.line,
        Some(Line::GciCreated {
            sweep: SWEEP.to_owned(),
            points: "3".to_owned(),
        })
    );
    assert_eq!(out.report.operation, "gci-create");

    let stored = cfdcraft_doc::open(&path).unwrap().document;
    assert_eq!(stored.artifacts.sweeps.len(), 1);
    let ids: Vec<&str> = stored
        .artifacts
        .scenarios
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(ids, POINTS);
    for (scenario, level) in stored.artifacts.scenarios.iter().zip(0_u32..) {
        assert_eq!(scenario.parent.as_deref(), Some("base"));
        assert_eq!(
            scenario.overrides,
            [Override::Set {
                setting: "/mesh/level".to_owned(),
                value: json!(level),
            }]
        );
    }
}

#[test]
fn the_three_points_have_distinct_meshes_of_32_64_and_128_cells() {
    let path = with_sweep("gci-meshes", &coarse_cavity());
    let document = cfdcraft_doc::open(&path).unwrap().document;
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");

    let mut meshes = Vec::new();
    for (point, cells) in POINTS.iter().zip([1_024_u64, 4_096, 16_384]) {
        let (study, _) = effective_of("gci", &document, Some(*point)).unwrap();
        let resolved = resolve(&study).unwrap();
        assert_eq!(resolved.cell_count(), Some(cells), "{point}");
        meshes.push(StageRecipes::from_resolved(&resolved, &tool).keys().mesh);
    }
    assert_ne!(meshes[0], meshes[1]);
    assert_ne!(meshes[1], meshes[2]);
    assert_ne!(meshes[0], meshes[2]);
}

#[test]
fn gci_report_reads_the_recorded_numbers() {
    let path = with_sweep("gci-report", &coarse_cavity());
    record_all(&path, NASA, [0, 1, 2]);

    let out = gci_report(&path, SWEEP, QUANTITY);
    passed(&out.report);
    assert_eq!(out.report.operation, "gci-report");
    let data = gci_of(&out);
    assert_eq!(data.sweep, SWEEP);
    assert_eq!(data.quantity, QUANTITY);

    let rows: Vec<_> = data
        .levels
        .iter()
        .map(|row| {
            (
                row.scenario.as_str(),
                row.level,
                row.cells,
                row.value,
                row.run.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("gci-cavity-001", 0, 1_024, NASA[0], "run-1"),
            ("gci-cavity-002", 1, 4_096, NASA[1], "run-2"),
            ("gci-cavity-003", 2, 16_384, NASA[2], "run-3"),
        ]
    );

    assert_eq!(data.reports.len(), 1);
    let report = finest(&out);
    assert_eq!(report.convergence, Convergence::Monotone);
    close(report.order, 1.78617, 1e-5);
    close(report.extrapolated, 0.971300, 1e-5);
    close(report.gci_fine, 0.00103083, 1e-5);
    close(Some(report.ratios[0]), 2.0, 1e-9);
    close(Some(report.ratios[1]), 2.0, 1e-9);
    assert_eq!(report.values, [NASA[2], NASA[1], NASA[0]]);
    assert!(report.warnings.is_empty());
}

#[test]
fn the_report_does_not_depend_on_the_order_of_the_runs() {
    let forward = with_sweep("gci-order-a", &coarse_cavity());
    record_all(&forward, NASA, [0, 1, 2]);
    let backward = with_sweep("gci-order-b", &coarse_cavity());
    record_all(&backward, NASA, [2, 1, 0]);

    let first = gci_report(&forward, SWEEP, QUANTITY);
    let second = gci_report(&backward, SWEEP, QUANTITY);
    passed(&first.report);
    passed(&second.report);
    assert_eq!(gci_of(&first).reports, gci_of(&second).reports);
    let values = |out: &DocOutcome| -> Vec<(String, f64)> {
        gci_of(out)
            .levels
            .iter()
            .map(|row| (row.scenario.clone(), row.value))
            .collect()
    };
    assert_eq!(values(&first), values(&second));
}

#[test]
fn the_latest_result_of_a_variant_is_used() {
    let path = with_sweep("gci-latest", &coarse_cavity());
    record(&path, POINTS[0], Some(&[(QUANTITY, NASA[0])]), None);
    // An older result of the middle point, then the newer one, then a rerun of the newer one.
    record(&path, POINTS[1], Some(&[(QUANTITY, 0.5)]), None);
    let newer = record(&path, POINTS[1], Some(&[(QUANTITY, NASA[1])]), None);
    record(&path, POINTS[1], Some(&[(QUANTITY, 0.1)]), Some(&newer));
    record(&path, POINTS[2], Some(&[(QUANTITY, NASA[2])]), None);

    let out = gci_report(&path, SWEEP, QUANTITY);
    passed(&out.report);
    let middle = &gci_of(&out).levels[1];
    assert_eq!(middle.run, newer);
    assert_eq!(middle.value, NASA[1]);
    close(finest(&out).order, 1.78617, 1e-5);
}

#[test]
fn a_study_with_no_level_can_still_be_swept() {
    let dir = scratch("gci-no-level");
    let study = coarse_cavity();
    let parsed: Value = serde_json::from_str(&study).unwrap();
    assert!(parsed["mesh"].get("level").is_none());
    let path = new_document(&dir, "cavity", &study);

    let out = gci_create(&path, &GciRequest::new(SWEEP));
    passed(&out.report);
    let document = cfdcraft_doc::open(&path).unwrap().document;
    assert_eq!(document.artifacts.scenarios.len(), 3);
}

#[test]
fn levels_that_cannot_make_a_series_are_refused_before_anything_is_stored() {
    let dir = scratch("gci-levels");
    let path = new_document(&dir, "cavity", &coarse_cavity());
    let before = std::fs::read(&path).unwrap();
    for levels in [
        vec![0, 1],
        vec![0, 1, 2, 3, 4, 5],
        vec![2, 1, 0],
        vec![0, 0, 1],
        vec![0, 1, 9],
    ] {
        let request = GciRequest {
            sweep: SWEEP.to_owned(),
            scenario: "base".to_owned(),
            levels: levels.clone(),
        };
        let out = gci_create(&path, &request);
        refused_with(&out, "CFD-SWP-002");
        assert_eq!(std::fs::read(&path).unwrap(), before, "{levels:?}");
    }
    let four = GciRequest {
        sweep: SWEEP.to_owned(),
        scenario: "base".to_owned(),
        levels: vec![0, 1, 2, 3],
    };
    let out = gci_create(&path, &four);
    passed(&out.report);
    assert_eq!(
        out.line,
        Some(Line::GciCreated {
            sweep: SWEEP.to_owned(),
            points: "4".to_owned(),
        })
    );
}

#[test]
fn a_sweep_that_is_not_finished_is_refused() {
    // The middle point has no run at all.
    let path = with_sweep("gci-wait-none", &coarse_cavity());
    record(&path, POINTS[0], Some(&[(QUANTITY, NASA[0])]), None);
    record(&path, POINTS[2], Some(&[(QUANTITY, NASA[2])]), None);
    let out = gci_report(&path, SWEEP, QUANTITY);
    refused_with(&out, "CFD-GCI-001");
    assert_eq!(message(&out), waiting("gci-cavity-002"));

    // Nothing at all: every point is named.
    let path = with_sweep("gci-wait-all", &coarse_cavity());
    let out = gci_report(&path, SWEEP, QUANTITY);
    refused_with(&out, "CFD-GCI-001");
    assert_eq!(
        message(&out),
        waiting("gci-cavity-001, gci-cavity-002, gci-cavity-003")
    );

    // The middle point's latest run recorded no result.
    let path = with_sweep("gci-wait-mesh", &coarse_cavity());
    record(&path, POINTS[0], Some(&[(QUANTITY, NASA[0])]), None);
    record(&path, POINTS[1], None, None);
    record(&path, POINTS[2], Some(&[(QUANTITY, NASA[2])]), None);
    let out = gci_report(&path, SWEEP, QUANTITY);
    refused_with(&out, "CFD-GCI-001");
    assert_eq!(message(&out), waiting("gci-cavity-002"));

    // The middle point's result does not hold the quantity: the line says which one.
    let path = with_sweep("gci-wait-number", &coarse_cavity());
    record(&path, POINTS[0], Some(&[(QUANTITY, NASA[0])]), None);
    record(&path, POINTS[1], Some(&[("other.quantity", 1.0)]), None);
    record(&path, POINTS[2], Some(&[(QUANTITY, NASA[2])]), None);
    let out = gci_report(&path, SWEEP, QUANTITY);
    refused_with(&out, "CFD-GCI-001");
    assert_eq!(message(&out), waiting("gci-cavity-002 (no test.quantity)"));
}

#[test]
fn an_unknown_sweep_or_a_pack_is_refused() {
    let path = with_sweep("gci-unknown", &coarse_cavity());
    let out = gci_report(&path, "no-such-sweep", QUANTITY);
    refused_with(&out, "CFD-GCI-001");
    assert_eq!(
        message(&out),
        say(&Line::SweepUnknown {
            sweep: "no-such-sweep".to_owned(),
        })
    );

    let packed = path.with_file_name("cavity.cfdpack");
    std::fs::copy(&path, &packed).unwrap();
    let out = gci_report(&packed, SWEEP, QUANTITY);
    refused_with(&out, "CFD-DOC-009");
}

#[test]
fn levels_that_are_not_refined_are_refused() {
    let dir = scratch("gci-not-refined");
    let path = new_document(&dir, "cavity", &coarse_cavity());
    // A sweep over the cell count that lists the same count twice.
    let sweep = Sweep {
        id: "same-mesh".to_owned(),
        base: "base".to_owned(),
        parameters: vec![Parameter {
            name: "cells_x".to_owned(),
            target: Target::Count {
                setting: "/mesh/cells_x".to_owned(),
            },
            values: Values::List {
                items: vec![32.0, 32.0, 64.0],
            },
        }],
    };
    passed(&sweep_create(&path, &sweep).report);
    for point in ["same-mesh-001", "same-mesh-002", "same-mesh-003"] {
        record(&path, point, Some(&[(QUANTITY, 1.0)]), None);
    }

    let out = gci_report(&path, "same-mesh", QUANTITY);
    refused_with(&out, "CFD-GCI-002");
    assert_eq!(
        message(&out),
        say(&Line::ScenarioRefused {
            reason: "the meshes are not refined: two points have the same cell count".to_owned(),
        })
    );
}

/// A cavity study with the extra members `change` adds, run by the solver the physics chooses.
fn with_physics(physics: Value) -> String {
    edit(&coarse_cavity(), |study| {
        study["solver"]["solver"] = json!("FromPhysics");
        study["physics"] = physics;
    })
}

#[test]
fn a_transient_study_adds_the_transient_warning() {
    let study = with_physics(json!({
        "time": { "mode": "transient", "delta_t": { "value": 0.01, "unit": "s" } },
    }));
    let path = with_sweep("gci-transient", &study);
    record_all(&path, NASA, [0, 1, 2]);

    let out = gci_report(&path, SWEEP, QUANTITY);
    passed(&out.report);
    assert!(finest(&out).warnings.contains(&Warning::Transient));
    assert!(
        gci_warning_lines(gci_of(&out)).contains(&Line::GciWarnTransient {
            quantity: QUANTITY.to_owned(),
        })
    );
}

#[test]
fn a_turbulent_study_adds_the_wall_function_warning() {
    let study = with_physics(json!({
        "turbulence": {
            "model": "k_omega_sst",
            "intensity": 0.05,
            "viscosity_ratio": 10.0,
            "reference_velocity": { "value": 0.1, "unit": "m/s" },
        },
    }));
    let path = with_sweep("gci-turbulent", &study);
    record_all(&path, NASA, [0, 1, 2]);

    let out = gci_report(&path, SWEEP, QUANTITY);
    passed(&out.report);
    assert!(finest(&out).warnings.contains(&Warning::WallFunction));
    assert!(
        gci_warning_lines(gci_of(&out)).contains(&Line::GciWarnWallFunction {
            quantity: QUANTITY.to_owned(),
        })
    );
}

#[test]
fn an_oscillatory_series_warns() {
    let path = with_sweep("gci-oscillatory", &coarse_cavity());
    // Coarse to fine: 0.9, 1.1, 1.0.
    record_all(&path, [0.9, 1.1, 1.0], [0, 1, 2]);

    let out = gci_report(&path, SWEEP, QUANTITY);
    passed(&out.report);
    assert_eq!(finest(&out).convergence, Convergence::Oscillatory);
    assert_eq!(
        out.line,
        Some(Line::GciSummary {
            quantity: QUANTITY.to_owned(),
            order: "1.00".to_owned(),
            gci_percent: "12.50".to_owned(),
            class: "oscillatory".to_owned(),
        })
    );
    assert!(
        gci_warning_lines(gci_of(&out)).contains(&Line::GciWarnOscillatory {
            quantity: QUANTITY.to_owned(),
        })
    );
}

#[test]
fn warning_lines_name_the_quantity() {
    let report = GciReport {
        convergence: Convergence::Oscillatory,
        ratios: [2.0, 2.0],
        values: [1.0, 1.1, 0.9],
        order: Some(3.0),
        extrapolated: Some(0.9),
        relative_error_fine: Some(0.1),
        extrapolated_error_fine: Some(0.1),
        gci_fine: Some(0.1),
        gci_coarse: Some(0.1),
        asymptotic_ratio: Some(1.3),
        warnings: vec![
            Warning::Oscillatory,
            Warning::OrderOutOfRange,
            Warning::AsymptoticOutOfRange,
            Warning::Transient,
            Warning::WallFunction,
            Warning::NoiseLimited,
            Warning::NotRefined,
        ],
    };
    let data = GciData {
        sweep: SWEEP.to_owned(),
        quantity: QUANTITY.to_owned(),
        levels: Vec::new(),
        reports: vec![report],
    };
    let quantity = || QUANTITY.to_owned();
    assert_eq!(
        gci_warning_lines(&data),
        [
            Line::GciWarnOscillatory {
                quantity: quantity()
            },
            Line::GciWarnOrder {
                quantity: quantity(),
                order: "3.000".to_owned(),
            },
            Line::GciWarnAsymptotic {
                quantity: quantity(),
                ratio: "1.300".to_owned(),
            },
            Line::GciWarnTransient {
                quantity: quantity()
            },
            Line::GciWarnWallFunction {
                quantity: quantity()
            },
            Line::GciWarnNoise {
                quantity: quantity()
            },
            Line::GciWarnNotRefined {
                quantity: quantity()
            },
        ]
    );

    // A clean monotone series has none.
    let path = with_sweep("gci-clean", &coarse_cavity());
    record_all(&path, NASA, [0, 1, 2]);
    let out = gci_report(&path, SWEEP, QUANTITY);
    assert!(gci_warning_lines(gci_of(&out)).is_empty());
}

#[test]
fn no_order_gives_the_no_order_sentence() {
    let path = with_sweep("gci-no-order", &coarse_cavity());
    // Coarse to fine: 1.15, 1.1, 1.0, which changes by less each time but not fast enough.
    record_all(&path, [1.15, 1.1, 1.0], [0, 1, 2]);

    let out = gci_report(&path, SWEEP, QUANTITY);
    passed(&out.report);
    assert_eq!(out.report.exit_class, 0);
    let report = finest(&out);
    assert_eq!(report.order, None);
    assert_eq!(report.gci_fine, None);
    assert_eq!(
        out.line,
        Some(Line::GciOrderNone {
            quantity: QUANTITY.to_owned(),
            class: "not converging".to_owned(),
        })
    );
}

#[test]
fn the_report_round_trips_through_json() {
    let path = with_sweep("gci-json", &coarse_cavity());
    record_all(&path, NASA, [0, 1, 2]);
    let out = gci_report(&path, SWEEP, QUANTITY);
    passed(&out.report);

    let text = out.report.to_json();
    let back: Report = serde_json::from_str(&text).unwrap();
    assert_eq!(back, out.report);

    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["repro"]["kind"], "gci");
    assert_eq!(parsed["repro"]["sweep"], SWEEP);
    assert_eq!(parsed["repro"]["quantity"], QUANTITY);
    assert_eq!(parsed["repro"]["levels"][2]["scenario"], POINTS[2]);
    assert_eq!(parsed["repro"]["reports"][0]["convergence"], "monotone");

    let data = serde_json::to_string(gci_of(&out)).unwrap();
    let again: GciData = serde_json::from_str(&data).unwrap();
    assert_eq!(&again, gci_of(&out));
}
