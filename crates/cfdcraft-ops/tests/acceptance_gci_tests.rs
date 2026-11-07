//! Success criterion 3 of the benchmark suite: Coarse, Medium and Fine differ by a ratio of two
//! per direction, a mesh independence sweep over them fills the queue like any other sweep, and
//! the report reads the grid convergence index from three recorded runs, with the published
//! worked examples going through the whole path. The runs are written into the documents as
//! records, as a solve would leave them; no solver and no host is involved.

use std::path::{Path, PathBuf};

use cfdcraft_gci::{Convergence, GciReport, Warning};
use cfdcraft_model::{ResolvedStudy, resolve};
use cfdcraft_ops::gci_ops::{GciRequest, gci_create, gci_report, gci_warning_lines};
use cfdcraft_ops::queue_ops::{AddRequest, queue_add};
use cfdcraft_ops::scenario_ops::effective_of;
use cfdcraft_ops::sweep_ops::sweep_create;
use cfdcraft_ops::{BenchmarkOptions, DocOutcome, ExitClass, GciData, Repro, benchmark_create};
use cfdcraft_runs::{
    Outcome, Output, Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id,
};
use cfdcraft_store::ContentHash;
use cfdcraft_sweep::{Parameter, Sweep, Target, Values};
use cfdcraft_templates::{MeshPreset, TemplateOptions, TimeChoice, build as template};
use cfdcraft_text::Line;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::{CAVITY, codes, edit, new_document, passed, say, scratch};

const QUANTITY: &str = "test.quantity";

/// Each template and the key number its mesh independence study reads.
const TEMPLATES: [(&str, &str); 5] = [
    ("lid-cavity", "bench.ghia.u_centre"),
    ("backward-step", "bench.step.reattachment"),
    ("backward-step-laminar", "bench.armaly.reattachment"),
    ("cylinder", "bench.cylinder.st"),
    ("heated-cavity", "bench.dvd.nu_hot"),
];

/// The three values of the published worked example, coarse to fine.
const NASA: [f64; 3] = [0.96178, 0.96854, 0.97050];
/// Coarse to fine: the middle value overshoots.
const OSCILLATORY: [f64; 3] = [0.9, 1.1, 1.0];
/// Coarse to fine: the values change by less each time but not fast enough to converge.
const NO_ORDER: [f64; 3] = [1.15, 1.1, 1.0];

fn sweep_of(id: &str) -> String {
    format!("gci-{id}")
}

/// The ids of the three points of the sweep of a template.
fn points_of(id: &str) -> Vec<String> {
    let sweep = sweep_of(id);
    (1..=3).map(|n| format!("{sweep}-{n:03}")).collect()
}

/// The study of a template at a mesh preset as the text `benchmark_create` hands back.
fn template_text(id: &str, mesh: &str) -> String {
    let options = BenchmarkOptions {
        mesh: Some(mesh.to_owned()),
        ..BenchmarkOptions::default()
    };
    let (report, text) = benchmark_create(id, &options);
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{id}: {}",
        report.render_text()
    );
    text.unwrap_or_else(|| panic!("{id}: no study came back"))
}

/// A new document of the template at the coarse mesh with its mesh independence sweep stored.
fn with_sweep(id: &str, tag: &str) -> PathBuf {
    let path = new_document(&scratch(tag), "bench", &template_text(id, "coarse"));
    let outcome = gci_create(&path, &GciRequest::new(&sweep_of(id)));
    passed(&outcome.report);
    path
}

/// Append a run made for the variant `scenario` that recorded a result holding `numbers`.
fn record(path: &Path, scenario: &str, numbers: &[(&str, f64)]) -> String {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let (study, tag) = effective_of("gci", &document, Some(scenario)).unwrap();
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let id = next_run_id(&document.artifacts.runs);
    let bytes = format!("output of {id}").into_bytes();
    let hash = ContentHash::of(&bytes);
    let size = bytes.len() as u64;
    let record = RunRecord {
        id: id.clone(),
        rerun_of: None,
        scenario: tag,
        mesh: None,
        result: Some(Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob: hash,
            size,
        }),
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash: ContentHash::of(b"a mesh made earlier"),
            result_key: Some(keys.result),
            settings: study,
            tool,
            host: None,
        },
        checks: Vec::new(),
        key_numbers: numbers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect(),
        comparison: None,
        continued_from: None,
        outcome: Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    };
    document.add_run(record).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
    id
}

/// The report of the mesh independence sweep of a template whose three points recorded `values`
/// (coarse to fine) for `quantity`.
fn report_for(id: &str, quantity: &str, values: [f64; 3], tag: &str) -> DocOutcome {
    let path = with_sweep(id, tag);
    for (point, value) in points_of(id).iter().zip(values) {
        record(&path, point, &[(quantity, value)]);
    }
    gci_report(&path, &sweep_of(id), quantity)
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

/// A template resolved at a preset, steady or transient as the template says or as chosen.
fn resolved_at(id: &str, mesh: MeshPreset, time: Option<TimeChoice>) -> ResolvedStudy {
    let options = TemplateOptions {
        mesh,
        time,
        ..TemplateOptions::default()
    };
    let study = template(id, &options).unwrap_or_else(|e| panic!("{id} at {mesh:?}: {e}"));
    resolve(&study).unwrap_or_else(|e| panic!("{id} at {mesh:?}: {e}"))
}

#[test]
fn sc3_the_presets_nest_and_the_gci_sweep_reports() {
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    for (id, _) in TEMPLATES {
        for time in [None, Some(TimeChoice::Transient)] {
            let studies: Vec<ResolvedStudy> = MeshPreset::ALL
                .iter()
                .map(|mesh| resolved_at(id, *mesh, time))
                .collect();
            let label = format!("{id} {time:?}");

            // Two per direction: four times the cells for each step, exactly.
            let cells: Vec<u64> = studies
                .iter()
                .map(|r| r.cell_count().expect("the mesh has a layout"))
                .collect();
            assert_eq!(cells[1], cells[0] * 4, "{label}");
            assert_eq!(cells[2], cells[0] * 16, "{label}");
            for (r, level) in studies.iter().zip(0_u32..) {
                assert_eq!(r.mesh.level, level, "{label}");
                assert_eq!(r.mesh.cells_x, studies[0].mesh.cells_x << level, "{label}");
                assert_eq!(r.mesh.cells_y, studies[0].mesh.cells_y << level, "{label}");
            }

            // The time step halves with each level where there is one.
            let steps: Vec<Option<f64>> = studies
                .iter()
                .map(ResolvedStudy::effective_delta_t)
                .collect();
            if time.is_some() || id == "cylinder" {
                assert!(steps[0].is_some(), "{label}");
                assert_eq!(steps[1], steps[0].map(|d| d / 2.0), "{label}");
                assert_eq!(steps[2], steps[0].map(|d| d / 4.0), "{label}");
            } else {
                assert_eq!(steps, vec![None::<f64>; 3], "{label}");
            }

            // Three meshes, three keys.
            let keys: Vec<_> = studies
                .iter()
                .map(|r| StageRecipes::from_resolved(r, &tool).keys().mesh)
                .collect();
            assert_ne!(keys[0], keys[1], "{label}");
            assert_ne!(keys[1], keys[2], "{label}");
            assert_ne!(keys[0], keys[2], "{label}");
        }

        // The sweep of the saved study makes three variants whose meshes are the presets.
        let path = with_sweep(id, &format!("ag1-{id}"));
        let document = cfdcraft_doc::open(&path).unwrap().document;
        let stored: Vec<String> = document
            .artifacts
            .scenarios
            .iter()
            .map(|s| s.id.clone())
            .collect();
        assert_eq!(stored, points_of(id), "{id}");
        assert_eq!(document.artifacts.sweeps.len(), 1, "{id}");
        assert_eq!(document.artifacts.sweeps[0].id, sweep_of(id), "{id}");
        for (point, mesh) in points_of(id).iter().zip(MeshPreset::ALL) {
            let (study, _) = effective_of("gci", &document, Some(point)).unwrap();
            let r = resolve(&study).unwrap();
            let preset = resolved_at(id, mesh, None);
            assert_eq!(r.mesh, preset.mesh, "{id} {point}");
            assert_eq!(r.cell_count(), preset.cell_count(), "{id} {point}");
            assert_eq!(
                r.effective_delta_t(),
                preset.effective_delta_t(),
                "{id} {point}"
            );
        }
    }
}

#[test]
fn sc3_the_sweep_fills_the_queue_like_any_sweep() {
    for (id, _) in TEMPLATES {
        let path = with_sweep(id, &format!("ag2-{id}"));
        let request = AddRequest {
            sweep: Some(sweep_of(id)),
            ..AddRequest::default()
        };
        let out = queue_add(&path, &request);
        passed(&out.report);
        assert_eq!(
            say(out.line.as_ref().expect("a sentence")),
            "Added 3 jobs to the queue; 3 are waiting.",
            "{id}"
        );
        let Some(Repro::Queue(data)) = &out.report.repro else {
            panic!("{id}: expected the queue, found {:?}", out.report.repro);
        };
        let ids: Vec<&str> = data.jobs.iter().map(|job| job.id.as_str()).collect();
        assert_eq!(ids, ["q-001", "q-002", "q-003"], "{id}");
        let scenarios: Vec<Option<&str>> = data
            .jobs
            .iter()
            .map(|job| job.scenario.as_deref())
            .collect();
        let points = points_of(id);
        let expected: Vec<Option<&str>> = points.iter().map(|p| Some(p.as_str())).collect();
        assert_eq!(scenarios, expected, "{id}");
        for (job, n) in data.jobs.iter().zip(0..) {
            assert_eq!(job.kind, "solve", "{id}");
            assert_eq!(job.state, "queued", "{id}");
            assert_eq!(job.origin, Some(format!("{} {n}", sweep_of(id))), "{id}");
        }
        assert_eq!(data.active, None, "{id}: no job is running");
        assert!(!data.drain, "{id}: nothing advances the queue");
    }
}

/// The warnings the study of a template adds to any series: wall functions for the turbulent step
/// and a time-dependent quantity for the cylinder.
fn template_warnings(id: &str) -> Vec<Warning> {
    match id {
        "backward-step" => vec![Warning::WallFunction],
        "cylinder" => vec![Warning::Transient],
        _ => Vec::new(),
    }
}

fn template_warning_lines(id: &str, quantity: &str) -> Vec<Line> {
    let quantity = quantity.to_owned();
    match id {
        "backward-step" => vec![Line::GciWarnWallFunction { quantity }],
        "cylinder" => vec![Line::GciWarnTransient { quantity }],
        _ => Vec::new(),
    }
}

#[test]
fn sc3_the_report_reads_three_recorded_runs() {
    for (id, quantity) in TEMPLATES {
        // The quantity is one the template says a mesh independence study reads.
        let info = cfdcraft_templates::list()
            .into_iter()
            .find(|t| t.id == id)
            .expect("listed");
        assert!(
            info.headline_quantities.iter().any(|q| q == quantity),
            "{id}"
        );

        // The published example: a monotone series with the observed order and the index.
        let out = report_for(id, quantity, NASA, &format!("ag3-{id}-nasa"));
        passed(&out.report);
        let data = gci_of(&out);
        assert_eq!(data.quantity, quantity, "{id}");
        assert_eq!(data.levels.len(), 3, "{id}");
        assert_eq!(data.reports.len(), 1, "{id}");
        let report = finest(&out);
        assert_eq!(report.convergence, Convergence::Monotone, "{id}");
        close(report.order, 1.78617, 1e-5);
        close(report.gci_fine, 0.00103083, 1e-5);
        assert_eq!(report.warnings, template_warnings(id), "{id}");
        assert_eq!(
            gci_warning_lines(data),
            template_warning_lines(id, quantity),
            "{id}"
        );
        assert_eq!(
            out.line,
            Some(Line::GciSummary {
                quantity: quantity.to_owned(),
                order: "1.79".to_owned(),
                gci_percent: "0.10".to_owned(),
                class: "monotone".to_owned(),
            }),
            "{id}"
        );

        // An overshooting series is oscillatory and says so.
        let out = report_for(id, quantity, OSCILLATORY, &format!("ag3-{id}-oscillating"));
        passed(&out.report);
        assert_eq!(finest(&out).convergence, Convergence::Oscillatory, "{id}");
        assert!(
            finest(&out).warnings.contains(&Warning::Oscillatory),
            "{id}"
        );
        assert!(
            gci_warning_lines(gci_of(&out)).contains(&Line::GciWarnOscillatory {
                quantity: quantity.to_owned(),
            }),
            "{id}"
        );
        assert_eq!(
            out.line,
            Some(Line::GciSummary {
                quantity: quantity.to_owned(),
                order: "1.00".to_owned(),
                gci_percent: "12.50".to_owned(),
                class: "oscillatory".to_owned(),
            }),
            "{id}"
        );

        // A series that does not converge gives no index and the sentence that says so.
        let out = report_for(id, quantity, NO_ORDER, &format!("ag3-{id}-no-order"));
        passed(&out.report);
        assert_eq!(out.report.exit_class, 0, "{id}");
        assert_eq!(finest(&out).order, None, "{id}");
        assert_eq!(finest(&out).gci_fine, None, "{id}");
        assert_eq!(
            out.line,
            Some(Line::GciOrderNone {
                quantity: quantity.to_owned(),
                class: "not converging".to_owned(),
            }),
            "{id}"
        );
    }
}

#[test]
fn sc3_the_vectors_of_the_research_hold_through_the_whole_path() {
    // Celik and others, Table 1: 18000, 8000 and 4500 cells on one area. The three meshes are
    // three variants of one document that differ in the cells along x (90, 160 and 360 with 50
    // along y), as a user would make them, and the values are those of the paper.
    let study = edit(CAVITY, |study| {
        study["mesh"]["cells_x"] = json!(90);
        study["mesh"]["cells_y"] = json!(50);
    });
    let path = new_document(&scratch("ag4-celik"), "celik", &study);
    let sweep = Sweep {
        id: "celik".to_owned(),
        base: "base".to_owned(),
        parameters: vec![Parameter {
            name: "cells_x".to_owned(),
            target: Target::Count {
                setting: "/mesh/cells_x".to_owned(),
            },
            values: Values::List {
                items: vec![90.0, 160.0, 360.0],
            },
        }],
    };
    passed(&sweep_create(&path, &sweep).report);
    for (point, value) in ["celik-001", "celik-002", "celik-003"]
        .iter()
        .zip([5.863, 5.972, 6.063])
    {
        record(&path, point, &[(QUANTITY, value)]);
    }
    let out = gci_report(&path, "celik", QUANTITY);
    passed(&out.report);
    let counts: Vec<u64> = gci_of(&out).levels.iter().map(|row| row.cells).collect();
    assert_eq!(counts, [4500, 8000, 18000]);
    assert_eq!(gci_of(&out).reports.len(), 1);
    let report = finest(&out);
    assert_eq!(report.convergence, Convergence::Monotone);
    close(Some(report.ratios[0]), 1.5, 1e-9);
    close(Some(report.ratios[1]), 4.0 / 3.0, 1e-9);
    close(report.order, 1.53397, 1e-5);
    close(report.extrapolated, 6.168496, 1e-5);
    close(report.relative_error_fine, 0.0150091, 1e-5);
    close(report.extrapolated_error_fine, 0.0171023, 1e-5);
    close(report.gci_fine, 0.0217499, 1e-5);
    // The prints of the paper.
    assert_eq!(format!("{:.3}", report.order.unwrap()), "1.534");
    assert_eq!(format!("{:.4}", report.extrapolated.unwrap()), "6.1685");
    assert_eq!(format!("{:.2}", report.gci_fine.unwrap() * 100.0), "2.17");
    assert_eq!(
        out.line,
        Some(Line::GciSummary {
            quantity: QUANTITY.to_owned(),
            order: "1.53".to_owned(),
            gci_percent: "2.17".to_owned(),
            class: "monotone".to_owned(),
        })
    );
    // The meshes are refined by different ratios, which the report says; the series is inside the
    // asymptotic range.
    assert!(report.warnings.contains(&Warning::NotRefined));
    assert!(!report.warnings.contains(&Warning::Oscillatory));
    let asymptotic = report.asymptotic_ratio.expect("an asymptotic ratio");
    assert!((asymptotic - 1.0).abs() < 0.1, "{asymptotic}");

    // The worked example of the reference solver's documentation: a ratio of two on each level.
    let out = report_for("lid-cavity", QUANTITY, NASA, "ag4-nasa");
    passed(&out.report);
    let report = finest(&out);
    close(report.order, 1.78617, 1e-5);
    close(report.extrapolated, 0.971300, 1e-5);
    close(report.gci_fine, 0.00103083, 1e-5);
    assert_eq!(format!("{:.3}", report.order.unwrap()), "1.786");
    assert_eq!(format!("{:.3}", report.gci_fine.unwrap() * 100.0), "0.103");
    assert!(report.warnings.is_empty());
}

#[test]
fn sc3_an_unfinished_or_unrefined_sweep_is_refused() {
    // The middle mesh has no run: it is named, for every template.
    for (id, quantity) in TEMPLATES {
        let path = with_sweep(id, &format!("ag5-{id}"));
        let points = points_of(id);
        record(&path, &points[0], &[(quantity, NASA[0])]);
        record(&path, &points[2], &[(quantity, NASA[2])]);
        let out = gci_report(&path, &sweep_of(id), quantity);
        refused_with(&out, "CFD-GCI-001");
        let wanted = say(&Line::GciWaiting {
            sweep: sweep_of(id),
            missing: points[1].clone(),
        });
        assert_eq!(out.report.diagnostics[0].message, wanted, "{id}");
    }

    // Two variants with the same cell count are not a refinement.
    let path = new_document(
        &scratch("ag5-same"),
        "same",
        &template_text("lid-cavity", "coarse"),
    );
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
        record(&path, point, &[(QUANTITY, 1.0)]);
    }
    let out = gci_report(&path, "same-mesh", QUANTITY);
    refused_with(&out, "CFD-GCI-002");
    let wanted = say(&Line::ScenarioRefused {
        reason: "the meshes are not refined: two points have the same cell count".to_owned(),
    });
    assert_eq!(out.report.diagnostics[0].message, wanted);
}
