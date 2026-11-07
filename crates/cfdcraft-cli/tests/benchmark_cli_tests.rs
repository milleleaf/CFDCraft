//! The benchmark verbs, run through the real binary: list the templates, write a study from one
//! with the options chosen, and check a recorded run against its published reference. Nothing
//! here needs a compute host: the runs are written into documents as stored results.

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cfdcraft_bench_cylinder::fixture::{Run, tree as cylinder_tree};
use cfdcraft_capture::{Bundle, store_bundle};
use cfdcraft_doc::store_dir;
use cfdcraft_model::{InitMode, ResolvedStudy, ResolvedTime, Study, TurbulenceModel, resolve};
use cfdcraft_runs::{
    KeyNumbers, Outcome, Output as Stored, Provenance, RunRecord, StageRecipes, ToolIdentity,
    next_run_id,
};
use cfdcraft_store::{ContentHash, Store};
use repro_support::{new_document, scratch};
use serde_json::Value;

const TEMPLATES: [&str; 5] = [
    "lid-cavity",
    "backward-step",
    "backward-step-laminar",
    "cylinder",
    "heated-cavity",
];

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .output()
        .unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("the binary ended by itself")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn json(o: &Output) -> Value {
    serde_json::from_str(&out(o)).expect("standard output is one JSON report")
}

/// The study file at `path` as the model resolves it.
fn resolved_at(path: &Path) -> ResolvedStudy {
    let text = std::fs::read_to_string(path).unwrap();
    let study = Study::from_json(&text).expect("the file is a study file");
    resolve(&study).expect("the study has no findings")
}

/// Write the study of `id` with the extra `args` to the new file `name` in `dir`, and resolve it.
fn made(dir: &Path, name: &str, id: &str, extra: &[&str]) -> ResolvedStudy {
    let path = dir.join(name);
    let mut args = vec!["benchmark", "create", id, "--out", s(&path)];
    args.extend_from_slice(extra);
    let o = run(&args);
    assert_eq!(code(&o), 0, "{id} {extra:?}: {}", err(&o));
    resolved_at(&path)
}

#[test]
fn list_prints_the_five_templates() {
    let o = run(&["benchmark", "list"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5, "{text}");
    for (line, id) in lines.iter().zip(TEMPLATES) {
        assert!(line.starts_with(&format!("{id}: ")), "{line}");
        assert!(line.ends_with(")."), "{line}");
    }

    let o = run(&["benchmark", "list", "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let report = json(&o);
    assert_eq!(report["repro"]["kind"], "benchmarks");
    let entries = report["repro"]["benchmarks"].as_array().unwrap();
    assert_eq!(entries.len(), 5);
    for (entry, id) in entries.iter().zip(TEMPLATES) {
        assert_eq!(entry["id"], id);
    }
}

#[test]
fn create_writes_a_study_that_compiles() {
    let dir = scratch("bench-cli-create");
    for id in TEMPLATES {
        let path = dir.join(format!("{id}.json"));
        let o = run(&["benchmark", "create", id, "--out", s(&path)]);
        assert_eq!(code(&o), 0, "{id}: {}", err(&o));
        let sentence = format!("Wrote the {id} study at the medium mesh to {}.", s(&path));
        assert!(out(&o).contains(&sentence), "{id}: {}", out(&o));
        assert!(path.is_file(), "{id}");

        let case = dir.join(format!("{id}-case"));
        let compiled = run(&["compile", s(&path), s(&case)]);
        assert_eq!(code(&compiled), 0, "{id}: {}", err(&compiled));
    }
}

#[test]
fn the_options_reach_the_study() {
    let dir = scratch("bench-cli-options");

    let step = made(
        &dir,
        "fine.json",
        "backward-step",
        &[
            "--mesh",
            "fine",
            "--turbulence",
            "rng-k-epsilon",
            "--init",
            "potential",
        ],
    );
    assert_eq!(step.mesh.level, 2);
    let physics = step.physics.expect("the options are physics");
    assert_eq!(physics.turbulence.model, TurbulenceModel::RngKEpsilon);
    assert_eq!(physics.init, InitMode::Potential);
    assert!(matches!(physics.time, ResolvedTime::Steady));

    let slow = made(
        &dir,
        "transient.json",
        "backward-step",
        &["--time", "transient", "--delta-t", "0.00002"],
    );
    let physics = slow.physics.expect("a transient run is physics");
    let ResolvedTime::Transient { delta_t } = physics.time else {
        panic!("expected a transient run, found {:?}", physics.time);
    };
    assert!((delta_t - 0.00002).abs() < 1e-12, "{delta_t}");

    // A higher Rayleigh number is a thinner fluid: the viscosity falls by the square root of the
    // ratio, 10 to the power of one half for a step from 1e5 to 1e6.
    let default = made(&dir, "ra-default.json", "heated-cavity", &[]);
    let hot = made(
        &dir,
        "ra-high.json",
        "heated-cavity",
        &["--rayleigh", "1000000"],
    );
    let ratio = hot.fluid.kinematic_viscosity / default.fluid.kinematic_viscosity;
    assert!((ratio - 10f64.powf(-0.5)).abs() < 1e-6, "{ratio}");
}

#[test]
fn an_existing_file_is_never_overwritten() {
    let dir = scratch("bench-cli-overwrite");
    let path = dir.join("study.json");
    let first = run(&["benchmark", "create", "lid-cavity", "--out", s(&path)]);
    assert_eq!(code(&first), 0, "{}", err(&first));
    let before = std::fs::read(&path).unwrap();

    let again = run(&["benchmark", "create", "cylinder", "--out", s(&path)]);
    assert_eq!(code(&again), 2, "{}", err(&again));
    assert!(err(&again).contains("already exists"), "{}", err(&again));
    assert!(!out(&again).contains("Wrote"), "{}", out(&again));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn bad_words_and_options_that_do_not_apply() {
    let dir = scratch("bench-cli-refused");
    let cases: [(&str, &[&str], &str); 7] = [
        ("lid-cavity", &["--mesh", "huge"], "mesh"),
        ("lid-cavity", &["--turbulence", "k-epsilon"], "turbulence"),
        ("lid-cavity", &["--time", "weekly"], "time"),
        ("lid-cavity", &["--init", "random"], "init"),
        ("cylinder", &["--rayleigh", "1e5"], "rayleigh"),
        ("backward-step", &["--delta-t", "0.001"], "delta_t"),
        ("nope", &[], "nope"),
    ];
    for (n, (id, extra, named)) in cases.into_iter().enumerate() {
        let path: PathBuf = dir.join(format!("never-{n}.json"));
        let mut args = vec!["benchmark", "create", id, "--out", s(&path)];
        args.extend_from_slice(extra);
        let o = run(&args);
        assert_eq!(code(&o), 2, "{id} {extra:?}: {}", err(&o));
        assert!(err(&o).contains(named), "{id} {extra:?}: {}", err(&o));
        assert!(out(&o).is_empty(), "{id} {extra:?}: {}", out(&o));
        assert!(
            !path.exists(),
            "{id} {extra:?}: a refused study is not written"
        );
    }
}

#[test]
fn json_mode_prints_only_the_report() {
    let dir = scratch("bench-cli-json");
    let path = dir.join("study.json");
    let o = run(&[
        "benchmark",
        "create",
        "heated-cavity",
        "--out",
        s(&path),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(err(&o).is_empty(), "{}", err(&o));
    let report = json(&o);
    assert_eq!(report["operation"], "benchmark-create");
    assert_eq!(report["exit_class"], 0);
    assert!(path.is_file());

    // A refusal, and an existing file, are one report each too.
    for id in ["nope", "heated-cavity"] {
        let o = run(&["benchmark", "create", id, "--out", s(&path), "--json"]);
        assert_eq!(code(&o), 2, "{id}: {}", err(&o));
        assert!(err(&o).is_empty(), "{id}: {}", err(&o));
        assert_eq!(json(&o)["exit_class"], 2, "{id}");
    }

    let o = run(&["benchmark", "list", "--json"]);
    assert!(err(&o).is_empty(), "{}", err(&o));
    assert_eq!(json(&o)["operation"], "benchmark-list");
}

/// Append a run that recorded a result holding `files`, stored in the store beside the document
/// the way a solve leaves one.
fn record_result(path: &Path, files: Vec<(String, String)>) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let study = document.study.clone();
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let store = Store::create(store_dir(path)).unwrap();
    let entries = files
        .into_iter()
        .map(|(name, text)| (name, text.into_bytes()))
        .collect();
    let (blob, size) = store_bundle(&store, &Bundle::from_entries(entries).unwrap()).unwrap();
    let record = RunRecord {
        id: next_run_id(&document.artifacts.runs),
        rerun_of: None,
        scenario: None,
        mesh: None,
        result: Some(Stored {
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

/// A new document of the cylinder study the command writes, with one stored result for each of
/// `runs`, in order.
fn cylinder_document(tag: &str, runs: &[Run]) -> PathBuf {
    let dir = scratch(tag);
    let study = dir.join("cylinder.json");
    let made = run(&["benchmark", "create", "cylinder", "--out", s(&study)]);
    assert_eq!(code(&made), 0, "{}", err(&made));
    let text = std::fs::read_to_string(&study).unwrap();
    let path = new_document(&dir, "cylinder", &text);
    let resolved = resolved_at(&study);
    for one in runs {
        record_result(&path, cylinder_tree(&resolved, one));
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

/// The code of the one finding of a refused check, from its JSON report.
fn refusal_code(args: &[&str]) -> String {
    let o = run(args);
    assert_eq!(code(&o), 2, "{args:?}: {}", err(&o));
    assert!(err(&o).is_empty(), "{args:?}: {}", err(&o));
    let report = json(&o);
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        1,
        "{args:?}"
    );
    report["diagnostics"][0]["code"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_document_with_no_runs_is_refused_with_its_code() {
    let path = cylinder_document("bench-cli-noruns", &[]);
    let o = run(&["benchmark", "check", s(&path)]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(
        err(&o).contains("This file has no runs yet."),
        "{}",
        err(&o)
    );
    assert!(out(&o).is_empty(), "{}", out(&o));
    assert_eq!(
        refusal_code(&["benchmark", "check", s(&path), "--json"]),
        "CFD-REPRO-005"
    );
}

#[test]
fn an_unknown_run_or_benchmark_is_refused() {
    let path = cylinder_document("bench-cli-unknown", &[Run::good()]);
    let doc = s(&path);

    let o = run(&["benchmark", "check", doc, "--run", "run-9"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(
        err(&o).contains("There is no run run-9 in this file."),
        "{}",
        err(&o)
    );
    // A run is a plain name: a path is not one.
    for wrong in ["run-9", "../x"] {
        let args = ["benchmark", "check", doc, "--run", wrong, "--json"];
        assert_eq!(refusal_code(&args), "CFD-REPRO-004", "{wrong}");
    }

    let o = run(&["benchmark", "check", doc, "--benchmark", "nope"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("nope"), "{}", err(&o));
    assert!(
        err(&o).contains("ghia, step, armaly, cylinder, dvd"),
        "the ids that exist are named: {}",
        err(&o)
    );
    for wrong in ["nope", "../x", "dvd"] {
        let args = ["benchmark", "check", doc, "--benchmark", wrong, "--json"];
        assert_eq!(refusal_code(&args), "CFD-BENCH-001", "{wrong}");
    }
    let o = run(&["benchmark", "check", doc, "--benchmark", "dvd"]);
    assert!(
        err(&o).contains("This study is not the dvd benchmark: "),
        "{}",
        err(&o)
    );

    // The same run and benchmark by name are fine.
    let o = run(&[
        "benchmark",
        "check",
        doc,
        "--run",
        "run-1",
        "--benchmark",
        "cylinder",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
}

#[test]
fn the_gate_table_goes_to_standard_output_and_the_class_follows() {
    let path = cylinder_document("bench-cli-gate", &[Run::good(), lift(1.3)]);
    let doc = s(&path);

    let o = run(&["benchmark", "check", doc, "--run", "run-1"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    assert!(text.contains("cylinder verdict: PASS"), "{text}");
    for metric in ["cDmax  ", "cLmax  ", "St  ", "DeltaP  "] {
        assert!(
            text.lines().any(|line| line.starts_with(metric)),
            "{metric}: {text}"
        );
    }
    let sentence = "cylinder: every metric is within its tolerance (4 checked).";
    let table_at = text.find("cylinder verdict: PASS").unwrap();
    let sentence_at = text.find(sentence).expect(sentence);
    assert!(table_at < sentence_at, "{text}");
    assert!(
        !err(&o).contains("verdict"),
        "the table is printed once: {}",
        err(&o)
    );

    // The newest run is the failing one: the table says so, and the class is the gate's.
    let o = run(&["benchmark", "check", doc]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    let text = out(&o);
    assert!(text.contains("cylinder verdict: FAILED"), "{text}");
    assert!(text.contains("cylinder: outside tolerance on"), "{text}");
    assert!(
        text.lines()
            .any(|line| line.starts_with("cLmax  ") && line.ends_with("FAIL")),
        "{text}"
    );
    assert!(!text.contains("every metric is within"), "{text}");

    let o = run(&["benchmark", "check", doc, "--json"]);
    assert_eq!(code(&o), 1);
    assert!(err(&o).is_empty(), "{}", err(&o));
    let report = json(&o);
    assert_eq!(report["exit_class"], 1);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["gate"]["passed"], false);
    assert_eq!(report["gate"]["name"], "cylinder");
}

#[test]
fn an_override_file_is_bounded_and_named() {
    // A lift peak of 1.05 is outside the shipped allowance of 3 % and inside 6 %.
    let path = cylinder_document("bench-cli-override", &[lift(1.05)]);
    let doc = s(&path);
    let dir = scratch("bench-cli-override-files");

    let shipped = run(&["benchmark", "check", doc]);
    assert_eq!(code(&shipped), 1, "{}", err(&shipped));

    let wider = file(
        &dir,
        "wider.toml",
        "[tolerance]\ncl_max = 0.06\njustification = \"the mesh is coarse\"\n",
    );
    let o = run(&["benchmark", "check", doc, "--tolerance-override", s(&wider)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    assert!(text.contains("tolerance override used:"), "{text}");
    assert!(text.contains("the mesh is coarse"), "{text}");

    let too_wide = file(
        &dir,
        "too-wide.toml",
        "[tolerance]\ncd_max = 0.5\njustification = \"a reason\"\n",
    );
    let o = run(&[
        "benchmark",
        "check",
        doc,
        "--tolerance-override",
        s(&too_wide),
    ]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("cd_max"), "{}", err(&o));
    assert!(out(&o).is_empty(), "{}", out(&o));

    let bare = file(&dir, "bare.toml", "[tolerance]\ncl_max = 0.05\n");
    let absent = dir.join("absent.toml");
    let big = file(&dir, "big.toml", &"#".repeat(64 * 1024 + 1));
    for (named, file) in [("bare", &bare), ("absent", &absent), ("big", &big)] {
        let args = [
            "benchmark",
            "check",
            doc,
            "--tolerance-override",
            s(file),
            "--json",
        ];
        assert_eq!(refusal_code(&args), "CFD-CLI-001", "{named}");
    }
    let o = run(&[
        "benchmark",
        "check",
        doc,
        "--tolerance-override",
        s(&absent),
    ]);
    assert!(err(&o).contains(s(&absent)), "{}", err(&o));
    let o = run(&["benchmark", "check", doc, "--tolerance-override", s(&big)]);
    assert!(err(&o).contains("64 KiB"), "{}", err(&o));
}

#[test]
fn validate_help_names_the_benchmarks() {
    let o = run(&["validate", "--help"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let help = out(&o);
    assert!(
        help.contains("published reference its study selects"),
        "{help}"
    );
    assert!(!help.contains("Re=100 cavity and compare both"), "{help}");

    let o = run(&["benchmark", "check", "--help"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let help = out(&o);
    for option in ["--run", "--benchmark", "--tolerance-override", "--json"] {
        assert!(help.contains(option), "{option}: {help}");
    }
}

/// Record an assessment for run `run` of the document, as a solve leaves it.
fn assess(path: &Path, run: &str, state: &str, reasons: &[&str]) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let record = document
        .artifacts
        .runs
        .iter_mut()
        .find(|r| r.id == run)
        .unwrap();
    record.verdict = Some(
        serde_json::from_value(serde_json::json!({
            "state": state,
            "reasons": reasons,
            "quantities": [],
            "mass": {"status": "ok", "imbalance": 0.001, "reference": 1.0},
            "energy": {"status": "not_applicable", "imbalance": null, "reference": null},
            "residuals_ok": state == "converged",
        }))
        .unwrap(),
    );
    cfdcraft_doc::save(&document, path).unwrap();
}

#[test]
fn an_unconverged_or_unassessed_run_is_flagged_before_the_table_and_the_class_stays() {
    let path = cylinder_document("bench-cli-converge", &[Run::good(), Run::good(), lift(1.3)]);
    let doc = s(&path);
    assess(&path, "run-1", "converged", &[]);
    assess(
        &path,
        "run-2",
        "not_converged",
        &["not_settled:floor", "residuals_above_limit"],
    );
    assess(&path, "run-3", "not_converged", &["mass_imbalance"]);

    // Converged: no warning, exit 0.
    let o = run(&["benchmark", "check", doc, "--run", "run-1"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(!out(&o).contains("Warning"), "{}", out(&o));

    // Not converged: the warning is the first line, the verdict is still PASS, exit 0.
    let o = run(&["benchmark", "check", doc, "--run", "run-2"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    assert!(
        text.starts_with(
            "Warning: the run behind this cylinder verdict is NOT CONVERGED (not_settled:floor, residuals_above_limit)."
        ),
        "{text}"
    );
    assert!(text.contains("cylinder verdict: PASS"), "{text}");

    // Not converged and failing: still exit 1.
    let o = run(&["benchmark", "check", doc, "--run", "run-3"]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(out(&o).starts_with("Warning: "), "{}", out(&o));
    assert!(out(&o).contains("cylinder verdict: FAILED"), "{}", out(&o));

    // JSON: the flag is in the gate.
    let o = run(&["benchmark", "check", doc, "--run", "run-2", "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let gate = &json(&o)["gate"];
    assert_eq!(gate["passed"], true);
    assert_eq!(gate["convergence"]["converged"], false);
    assert_eq!(
        gate["convergence"]["reason"],
        "not_settled:floor, residuals_above_limit"
    );

    // No assessment: unknown.
    let bare = cylinder_document("bench-cli-unassessed", &[Run::good()]);
    let o = run(&["benchmark", "check", s(&bare), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let gate = &json(&o)["gate"];
    assert_eq!(gate["convergence"]["converged"], false);
    assert_eq!(
        gate["convergence"]["reason"],
        "unknown: the run recorded no assessment"
    );
    let o = run(&["benchmark", "check", s(&bare)]);
    assert!(
        out(&o).starts_with("Warning: whether the run"),
        "{}",
        out(&o)
    );
}
