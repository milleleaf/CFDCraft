//! Checking a recorded run against its benchmark from the result stored in its document: the
//! stored result as a set of files, the verdict and the exit class, the refusals, the tolerance
//! file, and the promise that nothing in the document or the store changes.

use std::path::{Path, PathBuf};

use cfdcraft_bench_cylinder::fixture::{Run, tree as cylinder_tree};
use cfdcraft_capture::{Bundle, store_bundle};
use cfdcraft_doc::store_dir;
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{ResolvedStudy, resolve};
use cfdcraft_ops::{BundleFiles, DocOutcome, ExitClass, benchmark_check};
use cfdcraft_runs::{
    KeyNumbers, Outcome, Output, Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id,
};
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_templates::{TemplateOptions, build as template};
use cfdcraft_text::Line;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::{add_mesh_only_run, codes, new_document, passed, say, scratch, snapshot};

fn entries(files: Vec<(String, String)>) -> Bundle {
    let entries = files
        .into_iter()
        .map(|(name, text)| (name, text.into_bytes()))
        .collect();
    Bundle::from_entries(entries).unwrap()
}

/// The cylinder template as a study text and as the study a check resolves.
fn cylinder() -> (ResolvedStudy, String) {
    let study = template("cylinder", &TemplateOptions::default()).unwrap();
    let text = serde_json::to_string_pretty(&study).unwrap();
    (resolve(&study).unwrap(), text)
}

/// Append a run that recorded a result holding `files`, stored in the store beside the document,
/// the way a solve leaves one. Returns the id of the run.
fn record_result(path: &Path, files: Vec<(String, String)>, rerun_of: Option<&str>) -> String {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let study = document.study.clone();
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let store = Store::create(store_dir(path)).unwrap();
    let (blob, size) = store_bundle(&store, &entries(files)).unwrap();
    let id = next_run_id(&document.artifacts.runs);
    let record = RunRecord {
        id: id.clone(),
        rerun_of: rerun_of.map(str::to_owned),
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
    id
}

/// A new document of the cylinder template with one run for each of `runs`, in order.
fn cylinder_document(tag: &str, runs: &[Run]) -> PathBuf {
    let (resolved, text) = cylinder();
    let path = new_document(&scratch(tag), "cylinder", &text);
    for run in runs {
        record_result(&path, cylinder_tree(&resolved, run), None);
    }
    path
}

fn lift(amplitude: f64) -> Run {
    Run {
        lift_amplitude: amplitude,
        ..Run::good()
    }
}

fn file(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
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

fn message(outcome: &DocOutcome) -> &str {
    &outcome.report.diagnostics[0].message
}

#[test]
fn a_stored_result_is_a_result_files() {
    let files = BundleFiles::new(
        entries(vec![
            ("a/b/c.txt".to_owned(), "one".to_owned()),
            ("a/b/d.txt".to_owned(), "two".to_owned()),
            ("a/e.txt".to_owned(), "three".to_owned()),
            ("top.txt".to_owned(), "four".to_owned()),
        ])
        .to_bytes()
        .unwrap(),
    );

    assert_eq!(files.read("a/b/c.txt").as_deref(), Some("one"));
    assert_eq!(files.read("a/./b/c.txt").as_deref(), Some("one"));
    assert_eq!(files.read("top.txt").as_deref(), Some("four"));
    assert_eq!(files.read("a/b"), None, "a folder is not a file");
    assert_eq!(files.read("nothing"), None);
    assert_eq!(files.read(""), None);
    assert_eq!(files.read("../a/b/c.txt"), None);
    assert_eq!(files.read("a/../a/e.txt"), None);
    assert_eq!(files.read("/a/e.txt"), None);

    assert_eq!(files.list("a"), ["b", "e.txt"].map(str::to_owned));
    assert_eq!(files.list("a/b"), ["c.txt", "d.txt"].map(str::to_owned));
    assert_eq!(files.list(""), ["a", "top.txt"].map(str::to_owned));
    assert_eq!(files.list("."), ["a", "top.txt"].map(str::to_owned));
    assert!(files.list("nothing").is_empty());
    assert!(files.list("a/b/c.txt").is_empty(), "a file has no entries");
    assert!(files.list("..").is_empty());
    assert!(files.list("/a").is_empty());

    let broken = BundleFiles::new(vec![1, 2, 3]);
    assert_eq!(broken.read("a"), None);
    assert!(broken.list("").is_empty());
}

#[test]
fn a_recorded_cylinder_run_is_checked() {
    let path = cylinder_document("check-pass", &[Run::good()]);

    let done = benchmark_check(&path, None, None, None);
    passed(&done.report);
    assert_eq!(done.report.operation, "benchmark-check");
    assert_eq!(done.report.exit_class, 0);
    let gate = done
        .report
        .gate
        .as_ref()
        .expect("the report holds the gate");
    assert_eq!(gate.name, "cylinder");
    assert!(gate.passed, "{}", gate.render_text());
    let metrics: Vec<&str> = gate.metrics.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(metrics, ["cDmax", "cLmax", "St", "DeltaP"]);
    let line = done.line.expect("a passing check has its sentence");
    assert_eq!(
        line,
        Line::BenchmarkPassed {
            benchmark: "cylinder".to_owned(),
            metrics: "4".to_owned(),
        }
    );
    assert_eq!(
        say(&line),
        "cylinder: every metric is within its tolerance (4 checked)."
    );
    assert!(done.report.render_text().contains("cylinder"));

    // Asking for the benchmark by name gives the same verdict.
    let named = benchmark_check(&path, Some("run-1"), Some("cylinder"), None);
    passed(&named.report);
    assert_eq!(named.report.gate, done.report.gate);
}

#[test]
fn a_failing_run_fails_the_class() {
    let path = cylinder_document("check-fail", &[lift(1.3)]);

    let done = benchmark_check(&path, None, None, None);
    assert_eq!(done.report.class(), ExitClass::GateFailed);
    assert_eq!(done.report.exit_class, 1);
    assert_eq!(done.report.status, "failed");
    assert!(done.report.diagnostics.is_empty());
    let gate = done.report.gate.as_ref().unwrap();
    assert!(!gate.passed);
    let lift_metric = gate.metrics.iter().find(|m| m.name == "cLmax").unwrap();
    assert!(!lift_metric.passed);
    let Some(Line::BenchmarkFailed { benchmark, failed }) = &done.line else {
        panic!("expected the failed sentence, found {:?}", done.line);
    };
    assert_eq!(benchmark, "cylinder");
    assert!(failed.contains("cLmax"), "{failed}");
    assert!(done.report.render_text().contains("FAILED"));
}

#[test]
fn the_wrong_benchmark_is_named_with_what_differs() {
    let path = cylinder_document("check-wrong", &[Run::good()]);

    let wrong = benchmark_check(&path, None, Some("dvd"), None);
    refused_with(&wrong, "CFD-BENCH-001");
    assert!(
        message(&wrong).starts_with("This study is not the dvd benchmark: "),
        "{}",
        message(&wrong)
    );
    assert!(message(&wrong).contains("expected"), "{}", message(&wrong));

    let unknown = benchmark_check(&path, None, Some("nope"), None);
    refused_with(&unknown, "CFD-BENCH-001");
    assert!(message(&unknown).contains("nope"), "{}", message(&unknown));
    assert!(
        message(&unknown).contains("ghia, step, armaly, cylinder, dvd"),
        "{}",
        message(&unknown)
    );

    // A cavity at another Reynolds number is none of them; the cavity benchmark is the nearest.
    let dir = scratch("check-none");
    let study = repro_support::edit(repro_support::CAVITY, |study| {
        study["fluid"]["kinematic_viscosity"]["value"] = 0.0002.into();
    });
    let cavity = new_document(&dir, "cavity", &study);
    let files = vec![("log.simpleFoam".to_owned(), "End\n".to_owned())];
    record_result(&cavity, files, None);
    let none = benchmark_check(&cavity, None, None, None);
    refused_with(&none, "CFD-BENCH-001");
    assert!(
        message(&none).starts_with("This study is not the ghia benchmark: "),
        "{}",
        message(&none)
    );
}

#[test]
fn a_result_without_the_files_the_comparison_needs() {
    let (resolved, text) = cylinder();
    let path = new_document(&scratch("check-incomplete"), "cylinder", &text);
    let without_forces: Vec<(String, String)> = cylinder_tree(&resolved, &Run::good())
        .into_iter()
        .filter(|(name, _)| !name.contains("/force_"))
        .collect();
    assert_eq!(without_forces.len(), 2, "the two pressure probes remain");
    record_result(&path, without_forces, None);

    let done = benchmark_check(&path, None, None, None);
    assert_eq!(done.report.class(), ExitClass::GateFailed);
    let gate = done.report.gate.as_ref().unwrap();
    assert!(!gate.passed);
    let reason = gate.reason.as_deref().expect("a refusal has its reason");
    assert!(reason.starts_with("CFD-BENCH-002"), "{reason}");
    let Some(Line::BenchmarkResultsIncomplete { benchmark, file }) = &done.line else {
        panic!("expected the incomplete sentence, found {:?}", done.line);
    };
    assert_eq!(benchmark, "cylinder");
    assert!(file.contains("force_1"), "{file}");
}

#[test]
fn the_run_is_chosen_like_the_other_verbs() {
    let (resolved, text) = cylinder();
    let path = new_document(&scratch("check-runs"), "cylinder", &text);

    // No runs at all.
    let none = benchmark_check(&path, None, None, None);
    refused_with(&none, "CFD-REPRO-005");
    let none_named = benchmark_check(&path, Some("run-1"), None, None);
    refused_with(&none_named, "CFD-REPRO-005");

    record_result(&path, cylinder_tree(&resolved, &Run::good()), None);
    record_result(&path, cylinder_tree(&resolved, &lift(1.3)), None);
    // A rerun of the first run, which is not the newest run to be taken by default.
    record_result(&path, cylinder_tree(&resolved, &Run::good()), Some("run-1"));
    add_mesh_only_run(&path);

    // The newest run that is not a rerun and recorded a result is run-2, which fails.
    let newest = benchmark_check(&path, None, None, None);
    assert_eq!(newest.report.class(), ExitClass::GateFailed);
    assert!(
        newest
            .report
            .artifacts
            .iter()
            .any(|a| a.kind == "run" && a.name == "run-2"),
        "{:?}",
        newest.report.artifacts
    );
    // A named run is used, a rerun included.
    for named in ["run-1", "run-3"] {
        let done = benchmark_check(&path, Some(named), None, None);
        passed(&done.report);
    }
    assert_eq!(
        benchmark_check(&path, Some("run-2"), None, None)
            .report
            .class(),
        ExitClass::GateFailed
    );

    for wrong in ["run-9", "../x", "RUN-1", "", "run 1"] {
        let done = benchmark_check(&path, Some(wrong), None, None);
        refused_with(&done, "CFD-REPRO-004");
    }
    let no_result = benchmark_check(&path, Some("run-4"), None, None);
    refused_with(&no_result, "CFD-REPRO-005");
    assert_eq!(
        message(&no_result),
        "run-4 recorded no result, so there is nothing to rerun or compare."
    );
}

#[test]
fn an_override_is_used_and_named() {
    // A lift peak of 1.05 is outside the shipped allowance of 3 % and inside 6 %.
    let path = cylinder_document("check-override", &[lift(1.05)]);
    let dir = scratch("check-override-files");

    let shipped = benchmark_check(&path, None, None, None);
    assert_eq!(shipped.report.class(), ExitClass::GateFailed);

    let wider = file(
        &dir,
        "wider.toml",
        "[tolerance]\ncl_max = 0.06\njustification = \"the mesh is coarse\"\n",
    );
    let done = benchmark_check(&path, None, None, Some(&wider));
    passed(&done.report);
    let gate = done.report.gate.as_ref().unwrap();
    assert!(gate.passed, "{}", gate.render_text());
    let lift_metric = gate.metrics.iter().find(|m| m.name == "cLmax").unwrap();
    assert!(
        (lift_metric.tolerance - 0.06).abs() < 1e-9,
        "{}",
        lift_metric.tolerance
    );
    assert!(
        gate.notes
            .iter()
            .any(|note| note.starts_with("tolerance override used:")),
        "{:?}",
        gate.notes
    );
    let sentence = say(&Line::BenchmarkOverrideUsed {
        benchmark: "cylinder".to_owned(),
    });
    assert!(gate.notes.contains(&sentence), "{:?}", gate.notes);
    assert!(
        gate.notes
            .iter()
            .any(|note| note.contains("the mesh is coarse"))
    );
    assert!(matches!(done.line, Some(Line::BenchmarkPassed { .. })));

    // Too wide, without a reason, missing or too large: refused before anything is compared.
    let too_wide = file(
        &dir,
        "too-wide.toml",
        "[tolerance]\ncd_max = 0.5\njustification = \"a reason\"\n",
    );
    let refused = benchmark_check(&path, None, None, Some(&too_wide));
    refused_with(&refused, "CFD-CLI-001");
    assert!(
        message(&refused).contains("cd_max"),
        "{}",
        message(&refused)
    );
    let bare = file(&dir, "bare.toml", "[tolerance]\ncl_max = 0.05\n");
    let refused = benchmark_check(&path, None, None, Some(&bare));
    refused_with(&refused, "CFD-CLI-001");
    let absent = dir.join("absent.toml");
    let refused = benchmark_check(&path, None, None, Some(&absent));
    refused_with(&refused, "CFD-CLI-001");
    assert!(message(&refused).contains(&absent.display().to_string()));
    let big = file(&dir, "big.toml", &"#".repeat(64 * 1024 + 1));
    let refused = benchmark_check(&path, None, None, Some(&big));
    refused_with(&refused, "CFD-CLI-001");
    assert!(
        message(&refused).contains("64 KiB"),
        "{}",
        message(&refused)
    );
}

#[test]
fn a_result_gone_from_the_store_is_refused() {
    let path = cylinder_document("check-gone", &[Run::good()]);
    let blobs = store_dir(&path).join("blobs");
    for entry in std::fs::read_dir(&blobs).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }

    let done = benchmark_check(&path, None, None, None);
    refused_with(&done, "CFD-REPRO-002");
}

#[test]
fn the_check_reads_and_writes_nothing() {
    let path = cylinder_document("check-readonly", &[Run::good(), lift(1.3)]);
    let dir = scratch("check-readonly-files");
    let wider = file(
        &dir,
        "wider.toml",
        "[tolerance]\ncl_max = 0.06\njustification = \"a reason\"\n",
    );
    let before = snapshot(&path);

    let _ = benchmark_check(&path, Some("run-1"), None, None);
    let _ = benchmark_check(&path, None, None, None);
    let _ = benchmark_check(&path, None, Some("ghia"), None);
    let _ = benchmark_check(&path, None, Some("nope"), None);
    let _ = benchmark_check(&path, Some("run-9"), None, None);
    let _ = benchmark_check(&path, None, None, Some(&wider));

    assert_eq!(snapshot(&path), before);
}

/// Record the convergence assessment of run `run` in the document, as a solve leaves it.
fn assessed(path: &Path, run: &str, assessment: serde_json::Value) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let record = document
        .artifacts
        .runs
        .iter_mut()
        .find(|r| r.id == run)
        .unwrap();
    record.verdict = Some(serde_json::from_value(assessment).unwrap());
    cfdcraft_doc::save(&document, path).unwrap();
}

fn assessment(state: &str, reasons: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "state": state,
        "reasons": reasons,
        "quantities": [],
        "mass": {"status": "ok", "imbalance": 0.001, "reference": 1.0},
        "energy": {"status": "not_applicable", "imbalance": null, "reference": null},
        "residuals_ok": state == "converged",
    })
}

#[test]
fn a_converged_run_is_flagged_converged_and_says_nothing_more() {
    let path = cylinder_document("check-converged", &[Run::good()]);
    assessed(&path, "run-1", assessment("converged", &[]));

    let done = benchmark_check(&path, None, None, None);
    passed(&done.report);
    let gate = done.report.gate.as_ref().unwrap();
    let flag = gate
        .convergence
        .as_ref()
        .expect("the gate carries the flag");
    assert!(flag.converged);
    assert_eq!(flag.reason, "converged");
    assert_eq!(flag.message, "");
    let json = serde_json::to_value(gate).unwrap();
    assert_eq!(json["convergence"]["converged"], true);
    assert!(json["convergence"].get("message").is_none());
}

#[test]
fn an_unconverged_run_is_flagged_and_the_verdict_stays() {
    let reasons = ["not_settled:floor", "residuals_above_limit"];
    let path = cylinder_document("check-unconverged", &[Run::good(), lift(1.3)]);
    assessed(&path, "run-1", assessment("not_converged", &reasons));
    assessed(&path, "run-2", assessment("not_converged", &reasons));
    let sentence = say(&Line::BenchmarkNotConverged {
        benchmark: "cylinder".to_owned(),
        reasons: "not_settled:floor, residuals_above_limit".to_owned(),
    });

    // The run that passes still passes, with exit class 0, and the flag says it is not converged.
    let done = benchmark_check(&path, Some("run-1"), None, None);
    passed(&done.report);
    assert_eq!(done.report.exit_class, 0);
    let gate = done.report.gate.as_ref().unwrap();
    assert!(gate.passed);
    let flag = gate.convergence.as_ref().unwrap();
    assert!(!flag.converged);
    assert_eq!(flag.reason, "not_settled:floor, residuals_above_limit");
    assert_eq!(flag.message, sentence);
    assert!(sentence.contains("NOT CONVERGED"), "{sentence}");

    // The run that fails still fails, with exit class 1, and carries the same flag.
    let failed = benchmark_check(&path, Some("run-2"), None, None);
    assert_eq!(failed.report.exit_class, 1);
    let gate = failed.report.gate.as_ref().unwrap();
    assert!(!gate.passed);
    assert_eq!(gate.convergence.as_ref().unwrap().message, sentence);

    // The flag is part of the machine report.
    let json: serde_json::Value = serde_json::from_str(&done.report.to_json()).unwrap();
    assert_eq!(json["gate"]["convergence"]["converged"], false);
    assert_eq!(
        json["gate"]["convergence"]["reason"],
        "not_settled:floor, residuals_above_limit"
    );
}

#[test]
fn a_run_with_no_assessment_is_flagged_unknown() {
    let path = cylinder_document("check-unassessed", &[Run::good()]);

    let done = benchmark_check(&path, None, None, None);
    passed(&done.report);
    let flag = done
        .report
        .gate
        .as_ref()
        .unwrap()
        .convergence
        .as_ref()
        .unwrap();
    assert!(!flag.converged);
    assert_eq!(flag.reason, "unknown: the run recorded no assessment");
    assert!(flag.message.contains("UNKNOWN"), "{}", flag.message);
    assert!(
        flag.message
            .contains("unknown: the run recorded no assessment"),
        "{}",
        flag.message
    );

    // An assessment that could not judge the run is unknown too, with its cause.
    let path = cylinder_document("check-not-judged", &[Run::good()]);
    assessed(
        &path,
        "run-1",
        assessment("not_judged", &["no_monitored_quantity"]),
    );
    let done = benchmark_check(&path, None, None, None);
    let flag = done
        .report
        .gate
        .as_ref()
        .unwrap()
        .convergence
        .as_ref()
        .unwrap();
    assert!(!flag.converged);
    assert_eq!(flag.reason, "unknown: not judged (no_monitored_quantity)");
}
