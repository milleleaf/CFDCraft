use cfdcraft_bench_ghia::{Comparison, Row, render_table};
use cfdcraft_gate::{
    Benchmark, GateMetric, GateReport, GateRow, MetricKind, Mismatch, ResultFiles, bound_override,
};
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use std::collections::BTreeMap;

fn comparison(station: f64, computed: f64, reference: f64, dev: f64, ok: bool) -> Comparison {
    Comparison {
        max_abs_dev: dev,
        tolerance: 0.02,
        passed: ok,
        rows: vec![Row {
            station,
            computed,
            reference,
            abs_dev: dev,
        }],
    }
}

fn ghia_report() -> (GateReport, Comparison, Comparison) {
    let u = comparison(0.5, 0.25, 0.24, 0.01, true);
    let v = comparison(0.25, -0.5, -0.5, 0.0, true);
    let report = GateReport::from_metrics(
        "ghia",
        vec![
            GateMetric::from_comparison("u", &u),
            GateMetric::from_comparison("v", &v),
        ],
    );
    (report, u, v)
}

#[test]
fn a_ghia_report_serialises_as_before() {
    let (report, _, _) = ghia_report();
    let expected = concat!(
        r#"{"name":"ghia","passed":true,"metrics":["#,
        r#"{"name":"u","max_abs_dev":0.01,"tolerance":0.02,"passed":true,"rows":["#,
        r#"{"station":0.5,"computed":0.25,"reference":0.24,"abs_dev":0.01}]},"#,
        r#"{"name":"v","max_abs_dev":0.0,"tolerance":0.02,"passed":true,"rows":["#,
        r#"{"station":0.25,"computed":-0.5,"reference":-0.5,"abs_dev":0.0}]}]}"#,
    );
    assert_eq!(serde_json::to_string(&report).unwrap(), expected);
}

#[test]
fn a_failed_report_carries_its_reason() {
    let report = GateReport::failed_named("ghia", "no sampled output");
    assert!(!report.passed && report.metrics.is_empty());
    assert_eq!(report.reason.as_deref(), Some("no sampled output"));
    let text = serde_json::to_string(&report).unwrap();
    assert!(text.contains(r#""reason":"no sampled output""#));
    let again: GateReport = serde_json::from_str(&text).unwrap();
    assert_eq!(again, report);
}

#[test]
fn new_members_are_skipped_at_their_default() {
    let (report, _, _) = ghia_report();
    let text = serde_json::to_string(&report).unwrap();
    for member in [
        "kind",
        "reference_range",
        "unit",
        "gating",
        "notes",
        "convergence",
    ] {
        assert!(!text.contains(member), "{member} should be skipped");
    }
    // An old stored report without the new members still reads, with the defaults.
    let again: GateReport = serde_json::from_str(&text).unwrap();
    assert_eq!(again, report);
    assert!(again.metrics.iter().all(|m| m.gating));
    assert!(again.metrics.iter().all(|m| m.kind == MetricKind::Profile));

    let mut scalar = GateMetric::scalar("cDmax", 3.2301, [3.22, 3.24], 0.0648, Some("1"));
    scalar.gating = false;
    let mut report = GateReport::from_metrics("cylinder", vec![scalar]);
    report.notes.push("a note".to_owned());
    let text = serde_json::to_string(&report).unwrap();
    for member in [
        r#""kind":"scalar""#,
        r#""reference_range":[3.22,3.24]"#,
        r#""unit":"1""#,
        r#""gating":false"#,
        r#""notes":["a note"]"#,
    ] {
        assert!(text.contains(member), "{member} missing in {text}");
    }
    let again: GateReport = serde_json::from_str(&text).unwrap();
    assert_eq!(again, report);
}

#[test]
fn passed_means_every_gating_metric_passed() {
    let ok = GateMetric::scalar("a", 1.0, [0.9, 1.1], 0.01, None);
    let bad = GateMetric::scalar("b", 2.0, [0.9, 1.1], 0.01, None);
    assert!(ok.passed && !bad.passed);

    let mut info = bad.clone();
    info.gating = false;
    assert!(GateReport::from_metrics("x", vec![ok.clone(), info.clone()]).passed);
    assert!(!GateReport::from_metrics("x", vec![ok.clone(), bad]).passed);
    // Nothing gating means nothing was checked.
    assert!(!GateReport::from_metrics("x", vec![info]).passed);
    assert!(!GateReport::from_metrics("x", Vec::new()).passed);
}

#[test]
fn a_scalar_passes_inside_the_range_widened_by_the_tolerance() {
    // 0.125 is a power of two, so the edges 0.875 and 2.125 are exact in floating point (2.1 - 2.0
    // is 0.10000000000000009, which sits just outside a tolerance of 0.1).
    let at = |v: f64| GateMetric::scalar("m", v, [1.0, 2.0], 0.125, None);
    assert!(at(1.5).passed && at(0.875).passed && at(2.125).passed);
    assert!(!at(0.85).passed && !at(2.2).passed);
    assert!(!at(f64::NAN).passed && !at(f64::INFINITY).passed);
    let m = at(2.05);
    assert!((m.max_abs_dev - 0.05).abs() < 1e-12);
    let [row]: [GateRow; 1] = m.rows.try_into().unwrap();
    assert_eq!((row.station, row.computed, row.reference), (0.0, 2.05, 1.5));
    assert!((row.abs_dev - 0.55).abs() < 1e-12);
}

#[test]
fn the_ghia_text_is_unchanged() {
    let (report, u, v) = ghia_report();
    assert_eq!(report.render_text(), render_table(&u, &v));

    let failed = GateReport::failed_named("ghia", "no sampled output");
    assert_eq!(
        failed.render_text(),
        "Ghia verdict: FAILED\n  no sampled output\n"
    );
    let incomplete = GateReport::from_metrics("ghia", vec![GateMetric::from_comparison("u", &u)]);
    assert!(incomplete.render_text().contains("incomplete"));
}

/// Files in memory, with the refusals every implementation owes.
struct Files(BTreeMap<String, String>);

fn safe(rel: &str) -> bool {
    !rel.starts_with('/') && !rel.split('/').any(|part| part == "..")
}

impl ResultFiles for Files {
    fn read(&self, rel: &str) -> Option<String> {
        safe(rel).then(|| self.0.get(rel).cloned()).flatten()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        if !safe(rel_dir) {
            return Vec::new();
        }
        let dir = rel_dir.trim_end_matches('/');
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut names: Vec<String> = self
            .0
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

fn files(entries: &[(&str, &str)]) -> Files {
    Files(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    )
}

fn study() -> ResolvedStudy {
    let text = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
    resolve(&Study::from_json(text).unwrap()).unwrap()
}

struct Fake;

impl Fake {
    fn value(files: &dyn ResultFiles) -> Option<f64> {
        files.read("out/value.txt")?.trim().parse().ok()
    }
}

impl Benchmark for Fake {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn title(&self) -> &'static str {
        "A fake benchmark"
    }
    fn reference(&self) -> &'static str {
        "nowhere"
    }
    fn matches(&self, _study: &ResolvedStudy) -> Result<(), Vec<Mismatch>> {
        Ok(())
    }
    fn compare(&self, files: &dyn ResultFiles, _study: &ResolvedStudy) -> GateReport {
        match Self::value(files) {
            Some(v) => GateReport::from_metrics(
                "fake",
                vec![GateMetric::scalar("x", v, [1.0, 2.0], 0.1, None)],
            ),
            None => GateReport::failed_named("fake", "out/value.txt is missing"),
        }
    }
    fn quantities(&self, files: &dyn ResultFiles, _study: &ResolvedStudy) -> Vec<(String, f64)> {
        Self::value(files)
            .map(|v| vec![("bench.fake.x".to_owned(), v)])
            .unwrap_or_default()
    }
}

#[test]
fn a_benchmark_object_can_be_called_through_the_trait() {
    let benchmark: &dyn Benchmark = &Fake;
    let study = study();
    assert_eq!(benchmark.id(), "fake");
    assert!(benchmark.matches(&study).is_ok());

    let good = files(&[("out/value.txt", "1.5\n")]);
    let report = benchmark.compare(&good, &study);
    assert!(report.passed && report.name == "fake");
    assert_eq!(
        benchmark.quantities(&good, &study),
        vec![("bench.fake.x".to_owned(), 1.5)]
    );

    let empty = files(&[]);
    assert!(!benchmark.compare(&empty, &study).passed);
    assert!(benchmark.quantities(&empty, &study).is_empty());
}

#[test]
fn a_folder_and_a_stored_result_are_the_same_to_a_comparer() {
    let f = files(&[
        ("a/one.txt", "1"),
        ("a/two.txt", "2"),
        ("a/sub/three.txt", "3"),
        ("top.txt", "4"),
    ]);
    assert_eq!(f.read("a/one.txt").as_deref(), Some("1"));
    assert_eq!(f.read("a/missing.txt"), None);
    assert_eq!(f.read("../a/one.txt"), None);
    assert_eq!(f.read("a/../a/one.txt"), None);
    assert_eq!(f.read("/a/one.txt"), None);
    assert_eq!(f.list("a"), ["one.txt", "sub", "two.txt"]);
    assert_eq!(f.list(""), ["a", "top.txt"]);
    assert!(f.list("nothing").is_empty());
    assert!(f.list("../a").is_empty());
}

#[test]
fn a_mismatch_names_what_differs() {
    let m = Mismatch {
        what: "reynolds number".to_owned(),
        found: "200".to_owned(),
        expected: "100".to_owned(),
    };
    assert_eq!(m.to_string(), "reynolds number is 200, expected 100");
    assert_eq!(
        serde_json::to_string(&m).unwrap(),
        r#"{"what":"reynolds number","found":"200","expected":"100"}"#
    );
}

#[test]
fn the_override_helper_bounds_a_value() {
    assert_eq!(bound_override("u_max_abs", 0.03, 0.02, 3.0), Ok(0.03));
    for bad in [0.07, -0.01, 0.0, f64::NAN, f64::INFINITY] {
        let err = bound_override("u_max_abs", bad, 0.02, 3.0).unwrap_err();
        assert!(err.contains("u_max_abs"), "{err}");
    }
}

#[test]
fn the_override_is_named_in_the_report() {
    let mut report = GateReport::from_metrics(
        "fake",
        vec![GateMetric::scalar("x", 9.0, [1.0, 2.0], 0.1, None)],
    );
    assert!(!report.passed);
    report.note_override("x tolerance 0.1 instead of 0.05: coarse mesh");
    assert!(!report.passed);
    assert_eq!(
        report.notes,
        ["tolerance override used: x tolerance 0.1 instead of 0.05: coarse mesh"]
    );
}

#[test]
fn the_convergence_flag_round_trips_and_is_skipped_when_absent() {
    use cfdcraft_gate::GateConvergence;
    let mut report = GateReport::from_metrics(
        "cylinder",
        vec![GateMetric::scalar("a", 1.0, [0.9, 1.1], 0.01, None)],
    );
    assert!(report.convergence.is_none());
    report.convergence = Some(GateConvergence {
        converged: false,
        reason: "residuals_above_limit".to_owned(),
        message: "a sentence".to_owned(),
    });
    let text = serde_json::to_string(&report).unwrap();
    assert!(
        text.contains(
            r#""convergence":{"converged":false,"reason":"residuals_above_limit","message":"a sentence"}"#
        ),
        "{text}"
    );
    let again: GateReport = serde_json::from_str(&text).unwrap();
    assert_eq!(again, report);
    // The text of the table does not carry the flag: the caller prints its sentence.
    assert!(!report.render_text().contains("a sentence"));
}
