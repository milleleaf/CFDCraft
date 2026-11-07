//! The parity table over the runs a document recorded, and the admission check that reads it.
//! The runs are synthetic: the cylinder benchmark's result trees, stored under different backends
//! and images, with the key numbers and convergence verdicts the test gives them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdcraft_bench_cylinder::fixture::{Run, tree as cylinder_tree};
use cfdcraft_capture::{Bundle, store_bundle};
use cfdcraft_compare::Tolerance;
use cfdcraft_doc::store_dir;
use cfdcraft_model::{ResolvedStudy, Severity, resolve};
use cfdcraft_ops::backends::GOLDEN_COMPILE_PASSED;
use cfdcraft_ops::parity_ops::check_admission_with;
use cfdcraft_ops::repro::ParityData;
use cfdcraft_ops::{
    BundleFiles, ExitClass, Report, Repro, build as build_benchmark, check_admission, parity_table,
};
use cfdcraft_parity::{
    GateResult, Level, Levels, ParityInput, ParityReport, RecordedColumn, build, render_table,
};
use cfdcraft_runs::{
    KeyNumbers, Outcome, Output, Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id,
};
use cfdcraft_settle::{Balance, BalanceStatus, Verdict, VerdictState};
use cfdcraft_store::{ContentHash, Store};
use cfdcraft_templates::{TemplateOptions, build as template};
use cfdcraft_text::{Line, Text};

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::{codes, new_document, scratch};

const BASELINE: &str = "esi-v2512";

/// The recorded numbers of a good run, as the benchmark names them.
const BASE: [(&str, f64); 5] = [
    ("bench.cylinder.st", 0.30),
    ("bench.cylinder.cd_max", 3.24),
    ("bench.cylinder.cl_max", 1.0),
    ("bench.cylinder.delta_p", 2.48),
    ("bench.cylinder.cycles", 8.0),
];

fn say(line: &Line) -> String {
    Text::english().render(line)
}

fn entries(files: Vec<(String, String)>) -> Bundle {
    let entries = files
        .into_iter()
        .map(|(name, text)| (name, text.into_bytes()))
        .collect();
    Bundle::from_entries(entries).unwrap()
}

/// The cylinder template as a study text and as the study a run resolves.
fn cylinder() -> (ResolvedStudy, String) {
    let study = template("cylinder", &TemplateOptions::default()).unwrap();
    let text = serde_json::to_string_pretty(&study).unwrap();
    (resolve(&study).unwrap(), text)
}

fn balance() -> Balance {
    Balance {
        status: BalanceStatus::NotApplicable,
        imbalance: None,
        reference: None,
    }
}

fn verdict(state: VerdictState) -> Verdict {
    Verdict {
        state,
        reasons: Vec::new(),
        quantities: Vec::new(),
        mass: balance(),
        energy: balance(),
        residuals_ok: true,
    }
}

/// What a recorded run is made of: where it ran, what it left and what it recorded.
struct Recorded<'a> {
    dialect: &'a str,
    digest: &'a str,
    run: Run,
    numbers: Vec<(&'a str, f64)>,
    state: Option<VerdictState>,
}

impl<'a> Recorded<'a> {
    /// A good run, converged, with the base numbers.
    fn good(dialect: &'a str, digest: &'a str) -> Self {
        Self {
            dialect,
            digest,
            run: Run::good(),
            numbers: BASE.to_vec(),
            state: Some(VerdictState::Converged),
        }
    }

    fn number(mut self, name: &'a str, value: f64) -> Self {
        for entry in &mut self.numbers {
            if entry.0 == name {
                entry.1 = value;
            }
        }
        self
    }

    fn without(mut self, name: &str) -> Self {
        self.numbers.retain(|(n, _)| *n != name);
        self
    }

    fn state(mut self, state: Option<VerdictState>) -> Self {
        self.state = state;
        self
    }

    fn run(mut self, run: Run) -> Self {
        self.run = run;
        self
    }
}

/// Append a run that recorded a result, stored in the store beside the document the way a solve
/// leaves one. Returns the id of the run.
fn record(path: &Path, recorded: &Recorded<'_>) -> String {
    let (resolved, _) = cylinder();
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let study = document.study.clone();
    let tool = ToolIdentity::new(recorded.digest, recorded.dialect, None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let store = Store::create(store_dir(path)).unwrap();
    let files = entries(cylinder_tree(&resolved, &recorded.run));
    let (blob, size) = store_bundle(&store, &files).unwrap();
    let id = next_run_id(&document.artifacts.runs);
    let numbers: KeyNumbers = recorded
        .numbers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), *value))
        .collect();
    let run = RunRecord {
        id: id.clone(),
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
        key_numbers: numbers,
        comparison: None,
        continued_from: None,
        outcome: Outcome::Complete,
        checkpoint: None,
        verdict: recorded.state.map(verdict),
        np: None,
    };
    document.add_run(run).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
    id
}

/// A new document of the cylinder template with the given runs recorded in order.
fn document(tag: &str, runs: &[Recorded<'_>]) -> PathBuf {
    let (_, text) = cylinder();
    let path = new_document(&scratch(tag), "cylinder", &text);
    for run in runs {
        record(&path, run);
    }
    path
}

fn data(report: &Report) -> &ParityData {
    match &report.repro {
        Some(Repro::Parity(data)) => data,
        other => panic!("expected the data of a parity table, found {other:?}"),
    }
}

fn severity_of<'a>(report: &'a Report, code: &str) -> Option<&'a Severity> {
    report
        .diagnostics
        .iter()
        .find(|d| d.code == code)
        .map(|d| &d.severity)
}

#[test]
fn two_recorded_runs_give_a_parity_table() {
    let path = document(
        "parity-two",
        &[
            Recorded::good(BASELINE, "sha256:aa"),
            Recorded::good("esi-v2606", "sha256:bb"),
        ],
    );
    let report = parity_table(&path, "cylinder", None);
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
    let parity = &data(&report).parity;

    assert_eq!(parity.benchmark, "cylinder");
    assert_eq!(parity.baseline.backend, BASELINE);
    assert_eq!(parity.baseline.image, "sha256:aa");
    assert_eq!(parity.baseline.run, "run-1");
    assert_eq!(parity.columns.len(), 1);
    let column = &parity.columns[0];
    assert_eq!(column.backend, "esi-v2606");
    assert_eq!(column.image, "sha256:bb");
    assert_eq!(column.run, "run-2");
    assert!(!column.preview);
    assert_eq!(column.gate, GateResult::Pass);
    assert!(column.converged);
    assert_eq!(column.levels.oracle, Level::Pass);
    assert_eq!(column.levels.parity, Level::Pass);
    // The golden compile of this backend has not passed, so nothing is admitted yet.
    assert_eq!(column.levels.golden, Level::NotRun);

    // One row for each quantity the benchmark's gate judges; the cycle count is not one of them.
    let names: Vec<&str> = column.rows.iter().map(|r| r.quantity.as_str()).collect();
    assert_eq!(
        names,
        [
            "bench.cylinder.cd_max",
            "bench.cylinder.cl_max",
            "bench.cylinder.delta_p",
            "bench.cylinder.st",
        ]
    );

    // The tolerance of each row is the one the benchmark's own gate uses for it.
    let (resolved, _) = cylinder();
    let gate = build_benchmark("cylinder", None).unwrap().compare(
        &BundleFiles::new(
            entries(cylinder_tree(&resolved, &Run::good()))
                .to_bytes()
                .unwrap(),
        ),
        &resolved,
    );
    let gate_tolerance = |metric: &str| {
        gate.metrics
            .iter()
            .find(|m| m.name == metric)
            .map(|m| Tolerance::new(m.tolerance, 0.0).unwrap())
            .unwrap()
    };
    for (quantity, metric) in [
        ("bench.cylinder.cd_max", "cDmax"),
        ("bench.cylinder.cl_max", "cLmax"),
        ("bench.cylinder.delta_p", "DeltaP"),
        ("bench.cylinder.st", "St"),
    ] {
        let row = column.rows.iter().find(|r| r.quantity == quantity).unwrap();
        assert_eq!(row.tolerance, gate_tolerance(metric), "{quantity}");
    }

    // The text carries the header sentence and the fixed-format table.
    let text = &data(&report).text;
    let header = say(&Line::ParityTableHeader {
        benchmark: "cylinder".to_owned(),
        case: "cylinder".to_owned(),
        baseline: BASELINE.to_owned(),
    });
    assert!(text.starts_with(&header), "{text}");
    assert!(text.contains(&render_table(parity)), "{text}");
    assert!(text.contains("esi-v2606 value"), "{text}");
}

#[test]
fn runs_are_grouped_by_backend_and_image() {
    let path = document(
        "parity-groups",
        &[
            Recorded::good(BASELINE, "sha256:aa"),
            Recorded::good("esi-v2606", "sha256:bb"),
            Recorded::good("esi-v2606", "sha256:cc"),
            Recorded::good("esi-v2606", "sha256:bb").number("bench.cylinder.st", 0.301),
        ],
    );
    let report = parity_table(&path, "cylinder", Some("the cylinder at medium"));
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
    let parity = &data(&report).parity;
    assert_eq!(parity.case, "the cylinder at medium");
    assert_eq!(parity.columns.len(), 2);

    let by_image: BTreeMap<&str, (&str, f64)> = parity
        .columns
        .iter()
        .map(|c| {
            assert_eq!(c.backend, "esi-v2606");
            let st = c
                .rows
                .iter()
                .find(|r| r.quantity == "bench.cylinder.st")
                .unwrap()
                .value;
            (c.image.as_str(), (c.run.as_str(), st))
        })
        .collect();
    // The newest run of each group is the one used.
    assert_eq!(by_image["sha256:bb"].0, "run-4");
    assert!((by_image["sha256:bb"].1 - 0.301).abs() < 1e-12);
    assert_eq!(by_image["sha256:cc"].0, "run-3");
    assert!((by_image["sha256:cc"].1 - 0.30).abs() < 1e-12);
}

#[test]
fn a_missing_run_missing_baseline_or_missing_quantity_is_a_named_diagnostic() {
    // Only the baseline: nothing to set against it.
    let only_baseline = document("parity-only-base", &[Recorded::good(BASELINE, "sha256:aa")]);
    let report = parity_table(&only_baseline, "cylinder", None);
    assert_eq!(codes(&report), ["CFD-PARITY-001"]);
    assert_eq!(report.class(), ExitClass::StudyInvalid);
    assert!(
        report.diagnostics[0].message.contains("cylinder"),
        "{}",
        report.diagnostics[0].message
    );

    // Only another backend: no baseline to compare with, and the column is still shown.
    let no_baseline = document(
        "parity-no-base",
        &[Recorded::good("esi-v2606", "sha256:bb")],
    );
    let report = parity_table(&no_baseline, "cylinder", None);
    assert_eq!(codes(&report), ["CFD-PARITY-002"]);
    assert_eq!(data(&report).parity.columns.len(), 1);
    assert_eq!(data(&report).parity.columns[0].levels.parity, Level::NotRun);

    // No run at all.
    let empty = document("parity-empty", &[]);
    let report = parity_table(&empty, "cylinder", None);
    assert_eq!(codes(&report), ["CFD-PARITY-001", "CFD-PARITY-002"]);

    // A quantity missing in a column.
    let missing = document(
        "parity-missing",
        &[
            Recorded::good(BASELINE, "sha256:aa"),
            Recorded::good("esi-v2606", "sha256:bb").without("bench.cylinder.st"),
        ],
    );
    let report = parity_table(&missing, "cylinder", None);
    assert_eq!(codes(&report), ["CFD-PARITY-004"]);
    assert!(
        report.diagnostics[0].message.contains("bench.cylinder.st"),
        "{}",
        report.diagnostics[0].message
    );
    assert!(report.diagnostics[0].message.contains("esi-v2606"));

    // A benchmark that does not exist is refused by name.
    let report = parity_table(&missing, "no-such-benchmark", None);
    assert_eq!(codes(&report), ["CFD-BENCH-001"]);
    assert!(report.repro.is_none());
}

#[test]
fn a_failed_or_unconverged_column_is_a_warning_and_is_shown() {
    let path = document(
        "parity-warnings",
        &[
            Recorded::good(BASELINE, "sha256:aa"),
            Recorded::good("esi-v2606", "sha256:bb").state(Some(VerdictState::NotConverged)),
            // A lift far above the published range fails the gate.
            Recorded::good("esi-v2606", "sha256:cc").run(Run {
                lift_amplitude: 1.3,
                ..Run::good()
            }),
        ],
    );
    let report = parity_table(&path, "cylinder", None);
    // Warnings never change the class.
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
    assert_eq!(codes(&report), ["CFD-PARITY-003", "CFD-PARITY-003"]);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|d| d.severity == Severity::Warning)
    );

    let parity = &data(&report).parity;
    let bb = parity
        .columns
        .iter()
        .find(|c| c.image == "sha256:bb")
        .unwrap();
    assert!(!bb.converged);
    assert_eq!(bb.rows.len(), 4, "the column is shown with its rows");
    let not_converged = say(&Line::ParityNotConverged {
        backend: "esi-v2606".to_owned(),
    });
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.message == not_converged),
        "{}",
        report.render_text()
    );
    assert!(data(&report).text.contains(&not_converged));

    let cc = parity
        .columns
        .iter()
        .find(|c| c.image == "sha256:cc")
        .unwrap();
    assert_eq!(cc.gate, GateResult::Fail);
    assert!(cc.converged);
    assert_eq!(cc.rows.len(), 4);

    // Neither column counts, so no level of the table has run.
    assert_eq!(parity.levels, Levels::not_run());
    assert!(!parity.admitted());
}

#[test]
fn a_preview_column_carries_the_label_and_the_missing_claim() {
    let path = document(
        "parity-preview",
        &[
            Recorded::good(BASELINE, "sha256:aa"),
            Recorded::good("fieldcraft-lbm-0.7.5", "sha256:dd"),
        ],
    );
    let report = parity_table(&path, "cylinder", None);
    let parity = &data(&report).parity;
    assert_eq!(parity.columns.len(), 1);
    let column = &parity.columns[0];
    assert_eq!(column.backend, "fieldcraft-lbm-0.7.5");
    assert!(column.preview);

    let preview = say(&Line::BackendPreview);
    assert!(data(&report).text.contains(&preview));
    assert_eq!(
        severity_of(&report, "CFD-BACKEND-009"),
        Some(&Severity::Warning)
    );
    assert_eq!(
        severity_of(&report, "CFD-BACKEND-011"),
        Some(&Severity::Warning)
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == "CFD-BACKEND-009" && d.message == preview)
    );
    // A preview column never counts.
    assert!(!parity.admitted());
}

fn key_numbers(list: &[(&str, f64)]) -> KeyNumbers {
    list.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
}

fn recorded_column(
    backend: &str,
    gate_passed: bool,
    converged: Option<bool>,
    list: &[(&str, f64)],
) -> RecordedColumn {
    RecordedColumn {
        backend: backend.to_owned(),
        image: "sha256:11".to_owned(),
        run: "run-2".to_owned(),
        preview: false,
        gate_passed,
        converged,
        numbers: key_numbers(list),
    }
}

/// A table of one column of the backend `backend` against a baseline of two quantities, built by
/// the parity crate from the recorded numbers given.
fn table_of(
    backend: &str,
    gate_passed: bool,
    converged: Option<bool>,
    numbers: &[(&str, f64)],
) -> ParityReport {
    let tolerance = Tolerance::new(0.1, 0.0).unwrap();
    let input = ParityInput {
        benchmark: "cylinder".to_owned(),
        case: "case".to_owned(),
        baseline: recorded_column(BASELINE, true, Some(true), &[("q1", 1.0), ("q2", 2.0)]),
        columns: vec![recorded_column(backend, gate_passed, converged, numbers)],
        tolerances: BTreeMap::from([("q1".to_owned(), tolerance), ("q2".to_owned(), tolerance)]),
        golden: BTreeMap::from([(backend.to_owned(), Level::Pass)]),
    };
    build(&input).0
}

#[test]
fn admission_needs_all_three_levels() {
    let both = [("q1", 1.0), ("q2", 2.0)];

    // Everything passed, and the backend's golden compile is on the list.
    let good = table_of("esi-v2606", true, Some(true), &both);
    assert_eq!(good.columns[0].levels.parity, Level::Pass);
    assert!(check_admission_with(&good, "esi-v2606", &["esi-v2606"]).is_ok());

    // The list as it stands does not hold this backend: the golden level is named.
    let found = check_admission(&good, "esi-v2606").unwrap_err();
    assert_eq!(found.code, "CFD-PARITY-006");
    assert!(
        found.message.contains("golden compile"),
        "{}",
        found.message
    );
    assert!(!GOLDEN_COMPILE_PASSED.contains(&"esi-v2606"));

    // The gate failed: the oracle level is named.
    let failed = table_of("esi-v2606", false, Some(true), &both);
    let found = check_admission_with(&failed, "esi-v2606", &["esi-v2606"]).unwrap_err();
    assert_eq!(found.code, "CFD-PARITY-006");
    assert!(
        found.message.contains("reference oracle"),
        "{}",
        found.message
    );

    // The run is not converged, or has no assessment.
    for converged in [Some(false), None] {
        let table = table_of("esi-v2606", true, converged, &both);
        let found = check_admission_with(&table, "esi-v2606", &["esi-v2606"]).unwrap_err();
        assert_eq!(found.code, "CFD-PARITY-006");
        assert!(found.message.contains("convergence"), "{}", found.message);
    }

    // A quantity outside its tolerance.
    let off = table_of("esi-v2606", true, Some(true), &[("q1", 1.5), ("q2", 2.0)]);
    let found = check_admission_with(&off, "esi-v2606", &["esi-v2606"]).unwrap_err();
    assert_eq!(found.code, "CFD-PARITY-005");
    assert!(found.message.contains("q1"), "{}", found.message);

    // A quantity missing from the column: the parity level did not pass.
    let short = table_of("esi-v2606", true, Some(true), &[("q1", 1.0)]);
    let found = check_admission_with(&short, "esi-v2606", &["esi-v2606"]).unwrap_err();
    assert_eq!(found.code, "CFD-PARITY-006");
    assert!(found.message.contains("parity"), "{}", found.message);

    // A backend the table has no column for.
    let found = check_admission_with(&good, "su2-8.6.0", &["su2-8.6.0"]).unwrap_err();
    assert_eq!(found.code, "CFD-PARITY-001");
}
