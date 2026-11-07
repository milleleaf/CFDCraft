//! Success criterion 1 of the benchmark suite, as a user states it: each of the five templates
//! gives a study that compiles to a case the safety scan and the read-back check accept, and a
//! recorded run of it is shown against its published reference with the numbers beside the
//! verdict; a wrong run fails and names the metric; a study that was changed is refused by name.
//! Every result here is a synthetic tree stored in a document; no solver and no host is involved.

use std::path::{Path, PathBuf};

use cfdcraft_backend_esi::Dialect;
use cfdcraft_bench_cylinder::fixture::{Run, tree as cylinder_tree};
use cfdcraft_bench_dvd::fixture::{Tweak, good_files as dvd_files};
use cfdcraft_bench_ghia::{GhiaTable, render_table};
use cfdcraft_bench_step::fixture::good_files as step_files;
use cfdcraft_capture::{Bundle, store_bundle};
use cfdcraft_doc::store_dir;
use cfdcraft_gate::{GateMetric, GateReport, MetricKind};
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use cfdcraft_ops::ghia::{compare_results, load_table, profile_from_raw};
use cfdcraft_ops::{
    BenchmarkOptions, DocOutcome, ExitClass, benchmark_check, benchmark_create, ids, prepare,
    select,
};
use cfdcraft_runs::{
    KeyNumbers, Outcome, Output, Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id,
};
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_text::Line;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::{CAVITY, codes, edit, new_document, scratch};

/// A template, the benchmark that judges it, and the mesh preset it is judged on.
struct Case {
    template: &'static str,
    benchmark: &'static str,
    mesh: &'static str,
    /// A metric that a wrong run of the template must fail, where the metric is known by name.
    failing_metric: Option<&'static str>,
    /// How much the viscosity is multiplied to make a study that is no longer the benchmark.
    viscosity_factor: f64,
}

const CASES: [Case; 5] = [
    Case {
        template: "lid-cavity",
        benchmark: "ghia",
        mesh: "fine",
        failing_metric: Some("u"),
        viscosity_factor: 2.0,
    },
    Case {
        template: "backward-step",
        benchmark: "step",
        mesh: "medium",
        failing_metric: None,
        viscosity_factor: 2.0,
    },
    Case {
        template: "backward-step-laminar",
        benchmark: "armaly",
        mesh: "medium",
        failing_metric: None,
        // At twice the viscosity the laminar step is still inside the range the reference covers.
        viscosity_factor: 10.0,
    },
    Case {
        template: "cylinder",
        benchmark: "cylinder",
        mesh: "medium",
        failing_metric: Some("cLmax"),
        viscosity_factor: 2.0,
    },
    Case {
        template: "heated-cavity",
        benchmark: "dvd",
        mesh: "medium",
        failing_metric: Some("Nu"),
        viscosity_factor: 2.0,
    },
];

/// The study of a template at a mesh preset, as the text `benchmark_create` hands back.
fn created(template: &str, mesh: &str) -> String {
    let options = BenchmarkOptions {
        mesh: Some(mesh.to_owned()),
        ..BenchmarkOptions::default()
    };
    let (report, text) = benchmark_create(template, &options);
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{template}: {}",
        report.render_text()
    );
    text.unwrap_or_else(|| panic!("{template}: no study came back"))
}

fn resolved_of(text: &str) -> ResolvedStudy {
    let study = Study::from_json(text).expect("the text is a study");
    resolve(&study).expect("the study has no findings")
}

/// Both sampled centrelines of the cavity at the reference values, every velocity moved by `shift`
/// lid velocities, written the way the solver writes them.
fn ghia_files(r: &ResolvedStudy, shift: f64) -> Vec<(String, String)> {
    let table = GhiaTable::load().expect("the reference table loads");
    let lid = r.lid_velocity().expect("the cavity has a lid");
    let folder = r.solver.end_time;
    let mut out = Vec::new();
    for (file, profile, component, length) in [
        ("vertical_U.xy", &table.u, 1_usize, r.domain.length_y),
        ("horizontal_U.xy", &table.v, 2, r.domain.length_x),
    ] {
        let mut rows: Vec<_> = profile.coords.iter().zip(&profile.values).collect();
        rows.sort_by(|a, b| a.0.total_cmp(b.0));
        let text: String = rows
            .iter()
            .map(|(c, v)| {
                let mut cols = [0.0; 3];
                cols[component - 1] = (**v + shift) * lid;
                format!(
                    "{:.12} {:.12} {:.12} {:.12}\n",
                    **c * length,
                    cols[0],
                    cols[1],
                    cols[2]
                )
            })
            .collect();
        out.push((format!("postProcessing/centreline/{folder}/{file}"), text));
    }
    out
}

/// The floor line of a step run, with the reattachment moved to half its distance from the step.
/// The line holds four numbers per row; the first row is at the step, where the streamwise
/// velocity is minus the reattachment length in step heights.
fn move_reattachment(file: &mut (String, String)) {
    let rows: Vec<Vec<f64>> = file
        .1
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|token| token.parse::<f64>().expect("a number"))
                .collect()
        })
        .collect();
    let crossing = -rows[0][1];
    assert!(crossing > 0.0, "the floor line starts below zero velocity");
    file.1 = rows
        .iter()
        .map(|row| {
            format!(
                "{:.12} {:.12} {:.12} {:.12}\n",
                row[0],
                row[1] + crossing / 2.0,
                row[2],
                row[3]
            )
        })
        .collect();
}

/// The files of a run that reproduces the published numbers of the benchmark.
fn good_files(case: &Case, r: &ResolvedStudy) -> Vec<(String, String)> {
    match case.benchmark {
        "ghia" => ghia_files(r, 0.0),
        "step" | "armaly" => {
            step_files(r).unwrap_or_else(|| panic!("{}: no step files", case.template))
        }
        "cylinder" => cylinder_tree(r, &Run::good()),
        "dvd" => dvd_files(r, &Tweak::default())
            .unwrap_or_else(|reason| panic!("{}: {reason}", case.template)),
        other => panic!("no files for {other}"),
    }
}

/// The files of a run that is far outside the tolerance of at least one metric.
fn wrong_files(case: &Case, r: &ResolvedStudy) -> Vec<(String, String)> {
    match case.benchmark {
        // Ten times the allowance of two hundredths of a lid velocity.
        "ghia" => ghia_files(r, 0.2),
        "step" | "armaly" => {
            let mut files = good_files(case, r);
            // The floor line is the first file of both step trees.
            move_reattachment(&mut files[0]);
            files
        }
        // A lift peak of 1.3 against 1.0, about ten allowances.
        "cylinder" => cylinder_tree(
            r,
            &Run {
                lift_amplitude: 1.3,
                ..Run::good()
            },
        ),
        // A Nusselt number 20 % high against an allowance of 2 to 3 %.
        "dvd" => dvd_files(
            r,
            &Tweak {
                nu: 1.2,
                ..Tweak::default()
            },
        )
        .unwrap_or_else(|reason| panic!("{}: {reason}", case.template)),
        other => panic!("no files for {other}"),
    }
}

/// Append a run that recorded a result holding `files`, stored beside the document the way a solve
/// leaves one.
fn record_result(path: &Path, files: Vec<(String, String)>) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let study = document.study.clone();
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let entries = files
        .into_iter()
        .map(|(name, text)| (name, text.into_bytes()))
        .collect();
    let store = Store::create(store_dir(path)).unwrap();
    let (blob, size) = store_bundle(&store, &Bundle::from_entries(entries).unwrap()).unwrap();
    let record = RunRecord {
        id: next_run_id(&document.artifacts.runs),
        rerun_of: None,
        scenario: None,
        mesh: None,
        result: Some(Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob,
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
        key_numbers: KeyNumbers::new(),
        comparison: None,
        continued_from: None,
        outcome: Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    };
    document.add_run(record).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
}

/// A new document of the study text with one run holding `files`.
fn document_with_run(tag: &str, text: &str, files: Vec<(String, String)>) -> PathBuf {
    let path = new_document(&scratch(tag), "bench", text);
    record_result(&path, files);
    path
}

/// The word the table prints for a metric.
fn verdict_word(metric: &GateMetric) -> &'static str {
    match (metric.gating, metric.passed) {
        (false, _) => "info",
        (true, true) => "PASS",
        (true, false) => "FAIL",
    }
}

/// What the rendered table leaves out of what it must show for every metric: the computed value,
/// the published range, the tolerance and the verdict. Empty means the table shows all of it.
fn table_problems(gate: &GateReport) -> Vec<String> {
    let text = gate.render_text();
    let mut missing = Vec::new();
    for metric in &gate.metrics {
        let verdict = verdict_word(metric);
        if gate.name == "ghia" {
            let sign = if metric.passed { "<=" } else { ">" };
            let wanted = format!(
                "Ghia {}: {verdict} (max |dev| {:.7} {sign} {})",
                metric.name, metric.max_abs_dev, metric.tolerance
            );
            if !text.contains(&wanted) {
                missing.push(format!("the table lacks the line {wanted}"));
            }
            continue;
        }
        let prefix = format!("{}  ", metric.name);
        let Some(line) = text.lines().find(|line| line.starts_with(&prefix)) else {
            missing.push(format!("{}: the table has no line", metric.name));
            continue;
        };
        let mut wanted = Vec::new();
        match metric.kind {
            MetricKind::Scalar => {
                if let Some(row) = metric.rows.first() {
                    wanted.push(format!("{:.4}", row.computed));
                }
                if let Some([low, high]) = metric.reference_range {
                    wanted.push(format!("{low:.4} to {high:.4}"));
                }
                wanted.push(format!("{:.4}", metric.tolerance));
            }
            MetricKind::Profile => {
                wanted.push(format!("max deviation {:.6}", metric.max_abs_dev));
                wanted.push(format!("tolerance {:.6}", metric.tolerance));
            }
        }
        for piece in &wanted {
            if !line.contains(piece.as_str()) {
                missing.push(format!("{}: the line lacks {piece}: {line}", metric.name));
            }
        }
        if !line.ends_with(verdict) {
            missing.push(format!(
                "{}: the line does not end in {verdict}",
                metric.name
            ));
        }
    }
    missing
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
    assert!(outcome.report.gate.is_none());
}

#[test]
fn sc1_each_template_is_shown_against_its_reference() {
    for case in &CASES {
        let label = case.template;
        let text = created(case.template, case.mesh);
        if let Err(report) = prepare(&text, Dialect::EsiV2512) {
            panic!("{label}: the case was refused: {}", report.render_text());
        }
        let resolved = resolved_of(&text);
        let tag = format!("ab1-{}", case.benchmark);
        let path = document_with_run(&tag, &text, good_files(case, &resolved));

        let done = benchmark_check(&path, None, None, None);
        assert_eq!(
            done.report.class(),
            ExitClass::Passed,
            "{label}: {}",
            done.report.render_text()
        );
        assert_eq!(done.report.exit_class, 0, "{label}");
        let gate = done
            .report
            .gate
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: the report holds no gate"));
        assert_eq!(gate.name, case.benchmark, "{label}");
        assert!(gate.passed, "{label}: {}", gate.render_text());
        assert!(gate.reason.is_none(), "{label}: {:?}", gate.reason);
        assert!(!gate.metrics.is_empty(), "{label}");
        assert!(
            gate.metrics.iter().filter(|m| m.gating).all(|m| m.passed),
            "{label}: {}",
            gate.render_text()
        );
        assert_eq!(table_problems(gate), Vec::<String>::new(), "{label}");
        if gate.name != "ghia" {
            let head = format!("{} verdict: PASS\n", gate.name);
            assert!(gate.render_text().starts_with(&head), "{label}");
        }
        let Some(Line::BenchmarkPassed { benchmark, .. }) = &done.line else {
            panic!(
                "{label}: expected the passing sentence, found {:?}",
                done.line
            );
        };
        assert_eq!(benchmark, case.benchmark, "{label}");
    }
}

#[test]
fn sc1_a_wrong_run_fails_the_gate_and_names_the_metric() {
    for case in &CASES {
        let label = case.template;
        let text = created(case.template, case.mesh);
        let resolved = resolved_of(&text);
        let tag = format!("ab2-{}", case.benchmark);
        let path = document_with_run(&tag, &text, wrong_files(case, &resolved));

        let done = benchmark_check(&path, None, None, None);
        assert_eq!(done.report.class(), ExitClass::GateFailed, "{label}");
        assert_eq!(done.report.exit_class, 1, "{label}");
        assert!(
            done.report.diagnostics.is_empty(),
            "{label}: the run was judged, not refused"
        );
        let gate = done
            .report
            .gate
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: the report holds no gate"));
        assert_eq!(gate.name, case.benchmark, "{label}");
        assert!(!gate.passed, "{label}: {}", gate.render_text());
        assert!(gate.reason.is_none(), "{label}: {:?}", gate.reason);
        let failed: Vec<&str> = gate
            .metrics
            .iter()
            .filter(|m| m.gating && !m.passed)
            .map(|m| m.name.as_str())
            .collect();
        assert!(!failed.is_empty(), "{label}: {}", gate.render_text());
        if let Some(name) = case.failing_metric {
            assert!(failed.contains(&name), "{label}: {failed:?}");
        }
        assert_eq!(table_problems(gate), Vec::<String>::new(), "{label}");
        assert!(gate.render_text().contains("FAIL"), "{label}");

        let Some(Line::BenchmarkFailed {
            benchmark,
            failed: named,
        }) = &done.line
        else {
            panic!(
                "{label}: expected the failing sentence, found {:?}",
                done.line
            );
        };
        assert_eq!(benchmark, case.benchmark, "{label}");
        assert_eq!(named, &failed.join(", "), "{label}");
    }
}

#[test]
fn sc1_a_changed_study_is_refused_not_judged() {
    for case in &CASES {
        let label = case.template;
        let text = created(case.template, case.mesh);
        let changed = edit(&text, |study| {
            let nu = &mut study["fluid"]["kinematic_viscosity"]["value"];
            *nu = (nu.as_f64().expect("a number") * case.viscosity_factor).into();
        });
        let tag = format!("ab3-{}", case.benchmark);
        let stored = vec![("log.simpleFoam".to_owned(), "End\n".to_owned())];
        let path = document_with_run(&tag, &changed, stored);

        // Checked against its own benchmark, the study is refused with what differs.
        let named = benchmark_check(&path, None, Some(case.benchmark), None);
        refused_with(&named, "CFD-BENCH-001");
        let message = &named.report.diagnostics[0].message;
        let start = format!("This study is not the {} benchmark: ", case.benchmark);
        assert!(message.starts_with(&start), "{label}: {message}");
        assert!(message.contains("expected"), "{label}: {message}");
        let lower = message.to_lowercase();
        let words = ["reynolds", "viscosity", "rayleigh"];
        assert!(
            words.iter().any(|word| lower.contains(word)),
            "{label}: {message}"
        );

        // Left to choose, the check refuses it too.
        let chosen = benchmark_check(&path, None, None, None);
        refused_with(&chosen, "CFD-BENCH-001");

        // No benchmark selects the study, and every benchmark says what differs.
        let Err(rejected) = select(&resolved_of(&changed), None) else {
            panic!("{label}: the changed study still selects a benchmark");
        };
        let tried: Vec<&str> = rejected.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(tried, ids().to_vec(), "{label}");
        assert!(
            rejected.iter().all(|(_, found)| !found.is_empty()),
            "{label}"
        );
    }

    // The heated cavity at a Rayleigh number the reference does not tabulate.
    let options = BenchmarkOptions {
        rayleigh: Some(3e4),
        ..BenchmarkOptions::default()
    };
    let (report, text) = benchmark_create("heated-cavity", &options);
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
    let text = text.expect("a study");
    let stored = vec![(
        "log.buoyantBoussinesqSimpleFoam".to_owned(),
        "End\n".to_owned(),
    )];
    let path = document_with_run("ab3-rayleigh", &text, stored);
    let done = benchmark_check(&path, None, Some("dvd"), None);
    refused_with(&done, "CFD-BENCH-001");
    let message = &done.report.diagnostics[0].message;
    assert!(message.contains("Rayleigh number"), "{message}");
}

#[test]
fn sc1_the_cavity_still_validates_as_before() {
    let resolved = resolved_of(CAVITY);
    let files = ghia_files(&resolved, 0.004);

    // The result on disk, judged by the comparison the cavity has always had.
    let dir = scratch("ab4-files");
    for (name, text) in &files {
        let file = dir.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let table = load_table(None).unwrap();
    let direct = compare_results(&dir, &resolved, &table);
    assert_eq!(direct.name, "ghia");
    assert!(direct.passed, "{}", direct.render_text());

    // The same result stored in a document and checked through the benchmark verb.
    let path = document_with_run("ab4-doc", CAVITY, files);
    let done = benchmark_check(&path, None, None, None);
    assert_eq!(
        done.report.class(),
        ExitClass::Passed,
        "{}",
        done.report.render_text()
    );
    // The check adds the flag of the run's convergence; everything else is the direct comparison.
    let mut checked = done.report.gate.clone().expect("the report holds the gate");
    assert!(checked.convergence.take().is_some());
    let gate = &checked;
    assert_eq!(gate, &direct);
    assert_eq!(gate.render_text(), direct.render_text());
    assert_eq!(
        serde_json::to_string(gate).unwrap(),
        serde_json::to_string(&direct).unwrap()
    );
    assert!(matches!(done.line, Some(Line::BenchmarkPassed { .. })));

    // And that text is the reference crate's own table, character for character.
    let folder = resolved.solver.end_time.to_string();
    let lid = resolved.lid_velocity().unwrap();
    let sample = |file: &str, length: f64, component: usize| {
        let raw = std::fs::read_to_string(
            dir.join("postProcessing/centreline")
                .join(&folder)
                .join(file),
        )
        .unwrap();
        profile_from_raw(&raw, length, lid, component).unwrap()
    };
    let u = table
        .compare_u(&sample("vertical_U.xy", resolved.domain.length_y, 1))
        .unwrap();
    let v = table
        .compare_v(&sample("horizontal_U.xy", resolved.domain.length_x, 2))
        .unwrap();
    assert_eq!(gate.render_text(), render_table(&u, &v));
}
