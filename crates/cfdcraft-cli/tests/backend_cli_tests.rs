//! The verbs for backends, import and parity, and the backend choice on `compile` and `run`, run
//! through the real binary. Nothing here needs a compute host: the runs the parity table reads are
//! written into a document as records, and a run that is refused for its backend never reaches a
//! host.

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cfdcraft_bench_cylinder::fixture::{Run, tree as cylinder_tree};
use cfdcraft_capture::{Bundle, store_bundle};
use cfdcraft_doc::store_dir;
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use cfdcraft_runs::{
    KeyNumbers, Outcome, Output as Stored, Provenance, RunRecord, StageRecipes, ToolIdentity,
    next_run_id,
};
use cfdcraft_settle::{Balance, BalanceStatus, Verdict, VerdictState};
use cfdcraft_store::{ContentHash, Store};
use repro_support::{CAVITY, new_document, scratch};
use serde_json::Value;

/// The ids of the registry, in the order the list prints them.
const BACKENDS: [&str; 6] = [
    "esi-v2512",
    "esi-v2606",
    "of14-20261008",
    "su2-8.6.0",
    "codesaturne-9.2.0",
    "fieldcraft-lbm-0.7.5",
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

/// The codes of the findings of a JSON report, with their severities.
fn findings(report: &Value) -> Vec<(String, String)> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["code"].as_str().unwrap().to_owned(),
                d["severity"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// Every file under `dir` with its bytes, by name relative to `dir`.
fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, found);
            } else {
                let name = path.strip_prefix(base).unwrap().to_string_lossy();
                found.insert(name.replace('\\', "/"), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(dir, dir, &mut found);
    found
}

/// The cavity study as a file in a new folder.
fn cavity_file(tag: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(tag);
    let study = dir.join("cavity.json");
    std::fs::write(&study, CAVITY).unwrap();
    (dir, study)
}

// ---- backend list ----

#[test]
fn backend_list_prints_every_backend() {
    let o = run(&["backend", "list"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    let lines: Vec<&str> = text.lines().collect();
    let rows: Vec<usize> = BACKENDS
        .iter()
        .map(|id| {
            lines
                .iter()
                .position(|l| l.starts_with(&format!("{id} (")))
                .unwrap_or_else(|| panic!("{id} has no row in {text}"))
        })
        .collect();
    assert!(rows.windows(2).all(|w| w[0] < w[1]), "{rows:?}: {text}");
    assert_eq!(
        lines.iter().filter(|l| l.contains("): ")).count(),
        6,
        "one row for each backend: {text}"
    );
    assert!(lines[rows[0]].contains(": default, image "), "{text}");
    let lattice = rows[5];
    assert!(
        lines[lattice + 1].starts_with("Preview result"),
        "the preview line follows the lattice row: {text}"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|l| l.starts_with("Preview result"))
            .count(),
        1,
        "only the lattice backend is a preview: {text}"
    );

    let o = run(&["backend", "list", "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(err(&o).is_empty(), "{}", err(&o));
    let report = json(&o);
    assert_eq!(report["operation"], "backend-list");
    assert_eq!(report["repro"]["kind"], "backends");
    let ids: Vec<&str> = report["repro"]["backends"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, BACKENDS);
    assert_eq!(report["repro"]["backends"][5]["preview"], true);
}

// ---- import-case ----

/// A dictionary file: the `FoamFile` header for `object`, then `body`.
fn dict(object: &str, body: &str) -> String {
    format!(
        "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class dictionary;\n    object {object};\n}}\n\n{body}\n"
    )
}

fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A small clean case in the layout of a current Foundation release.
fn write_clean_case(root: &Path) {
    put(
        root,
        "system/controlDict",
        &dict(
            "controlDict",
            "solver incompressibleFluid;\nstartFrom startTime;\nstartTime 0;\nstopAt endTime;\nendTime 0.5;\ndeltaT 0.005;",
        ),
    );
    put(
        root,
        "system/fvSchemes",
        &dict("fvSchemes", "ddtSchemes\n{\n    default steadyState;\n}"),
    );
    put(
        root,
        "constant/physicalProperties",
        &dict("physicalProperties", "viscosityModel constant;\nnu 0.01;"),
    );
    put(
        root,
        "constant/momentumTransport",
        &dict(
            "momentumTransport",
            "simulationType RAS;\nRAS\n{\n    model kEpsilon;\n    turbulence on;\n}",
        ),
    );
    put(
        root,
        "0/U",
        &dict(
            "U",
            "dimensions [0 1 -1 0 0 0 0];\ninternalField uniform (0 0 0);\nboundaryField\n{\n    movingWall\n    {\n        type fixedValue;\n        value uniform (1 0 0);\n    }\n}",
        ),
    );
}

#[test]
fn import_case_imports_and_prints_the_report() {
    let dir = scratch("backend-cli-import");
    let source = dir.join("source").join("tutorial-case");
    write_clean_case(&source);
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let target = ws.join("mycase.cfdcase");

    let o = run(&["import-case", s(&source), "--out", s(&target)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    assert!(text.contains("Imported mycase.cfdcase: "), "{text}");
    assert!(text.contains(" files copied, 0 refused."), "{text}");
    assert!(
        text.contains("Detected case dialect: foundation-14."),
        "{text}"
    );
    assert!(
        text.contains("The original in mycase.cfdcase is read-only; edit the working copy."),
        "{text}"
    );
    assert!(target.join("import.json").is_file());
    assert!(target.join("original/system/controlDict").is_file());
    assert!(target.join("work/system/controlDict").is_file());
    let shown = format!("{text}{}", err(&o));
    assert!(
        !shown.contains(s(&dir)),
        "no directory outside the workspace is named: {shown}"
    );

    // The same report as JSON, for a second name.
    let second = ws.join("again.cfdcase");
    let o = run(&["import-case", s(&source), "--out", s(&second), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(err(&o).is_empty(), "{}", err(&o));
    let report = json(&o);
    assert_eq!(report["repro"]["kind"], "import");
    assert_eq!(report["repro"]["dialect"], "foundation-14");
    assert!(!out(&o).contains(s(&dir)), "{}", out(&o));

    // A target that is not a `.cfdcase` name is refused and nothing is written.
    let bad = ws.join("plain-name");
    let o = run(&["import-case", s(&source), "--out", s(&bad)]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("CFD-CLI-001"), "{}", err(&o));
    assert!(!bad.exists());

    // A case that holds code-executing entries is refused whole, one line for each.
    let unsafe_source = dir.join("source").join("unsafe-case");
    write_clean_case(&unsafe_source);
    put(
        &unsafe_source,
        "system/extraDict",
        "a 1;\nb\n#codeStream\n{\n}\n",
    );
    put(&unsafe_source, "0/otherDict", "a 1;\ntype coded;\n");
    let refused = ws.join("refused.cfdcase");
    let o = run(&["import-case", s(&unsafe_source), "--out", s(&refused)]);
    assert_ne!(code(&o), 0, "{}", err(&o));
    let lines = err(&o);
    assert_eq!(lines.matches("CFD-IMPORT-002").count(), 2, "{lines}");
    assert!(lines.contains("system/extraDict"), "{lines}");
    assert!(lines.contains("0/otherDict"), "{lines}");
    assert!(out(&o).is_empty(), "{}", out(&o));
    assert!(!refused.exists(), "a refused case writes nothing");
    assert!(
        !lines.contains(s(&dir)),
        "no directory outside the workspace is named: {lines}"
    );
}

// ---- parity ----

/// Append a run that recorded a result of the cylinder study, stored beside the document the way a
/// solve leaves one, made by the tools `dialect` and `digest` name and recorded with `numbers`.
fn record(
    path: &Path,
    resolved: &ResolvedStudy,
    dialect: &str,
    digest: &str,
    numbers: &[(&str, f64)],
) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let study = document.study.clone();
    let tool = ToolIdentity::new(digest, dialect, None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let store = Store::create(store_dir(path)).unwrap();
    let entries = cylinder_tree(resolved, &Run::good())
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
        key_numbers: numbers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect::<KeyNumbers>(),
        comparison: None,
        continued_from: None,
        outcome: Outcome::Complete,
        checkpoint: None,
        verdict: Some(Verdict {
            state: VerdictState::Converged,
            reasons: Vec::new(),
            quantities: Vec::new(),
            mass: not_applicable(),
            energy: not_applicable(),
            residuals_ok: true,
        }),
        np: None,
    };
    document.add_run(record).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
}

fn not_applicable() -> Balance {
    Balance {
        status: BalanceStatus::NotApplicable,
        imbalance: None,
        reference: None,
    }
}

/// The recorded numbers of a good cylinder run, as the benchmark names them.
const BASE: [(&str, f64); 5] = [
    ("bench.cylinder.st", 0.30),
    ("bench.cylinder.cd_max", 3.24),
    ("bench.cylinder.cl_max", 1.0),
    ("bench.cylinder.delta_p", 2.48),
    ("bench.cylinder.cycles", 8.0),
];

/// A document of the cylinder study the command writes, with one run under the default backend and
/// one under the second Foundation-line image.
fn cylinder_document(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let study = dir.join("cylinder.json");
    let made = run(&["benchmark", "create", "cylinder", "--out", s(&study)]);
    assert_eq!(code(&made), 0, "{}", err(&made));
    let text = std::fs::read_to_string(&study).unwrap();
    let resolved = resolve(&Study::from_json(&text).unwrap()).unwrap();
    let path = new_document(&dir, "cylinder", &text);
    record(&path, &resolved, "esi-v2512", "sha256:aa", &BASE);
    record(&path, &resolved, "esi-v2606", "sha256:bb", &BASE);
    path
}

#[test]
fn parity_prints_the_table_for_a_benchmark() {
    let doc = cylinder_document("backend-cli-parity");
    let o = run(&["parity", "--benchmark", "cylinder", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let text = out(&o);
    assert!(
        text.starts_with("Parity of cylinder, cylinder, against esi-v2512."),
        "{text}"
    );
    assert!(text.contains("benchmark: cylinder"), "{text}");
    assert!(text.contains("baseline: esi-v2512 PASS"), "{text}");
    assert!(text.contains("bench.cylinder.cd_max"), "{text}");
    assert!(
        text.contains("Levels: golden compile "),
        "the levels sentence is printed: {text}"
    );
    assert!(
        text.contains("esi-v2606 (image sha256:bb): gate PASS, converged true."),
        "{text}"
    );
    assert!(text.contains("esi-v2606 is converged."), "{text}");

    // The candidate notice is a finding on standard error, never part of the table.
    let o = run(&["parity", "--benchmark", "cylinder", s(&doc), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(err(&o).is_empty(), "{}", err(&o));
    let report = json(&o);
    assert_eq!(report["repro"]["kind"], "parity");
    assert_eq!(report["repro"]["parity"]["benchmark"], "cylinder");
    assert_eq!(
        report["repro"]["parity"]["columns"][0]["backend"],
        "esi-v2606"
    );

    // A benchmark nobody has is refused and named.
    let o = run(&["parity", "--benchmark", "no-such-benchmark", s(&doc)]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("no-such-benchmark"), "{}", err(&o));
    assert!(out(&o).is_empty(), "{}", out(&o));
}

// ---- the backend choice on compile and run ----

#[test]
fn compile_with_a_backend_writes_that_backends_case() {
    let (dir, study) = cavity_file("backend-cli-compile");

    let su2 = dir.join("su2-case");
    let o = run(&["compile", "--backend", "su2-8.6.0", s(&study), s(&su2)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let names: Vec<String> = files_under(&su2).into_keys().collect();
    assert_eq!(names, ["config.cfg", "mesh.su2"]);

    // Without the flag the output is that of the default backend, byte for byte.
    let plain = dir.join("plain-case");
    let o = run(&["compile", s(&study), s(&plain)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let named = dir.join("named-case");
    let o = run(&["compile", "--backend", "esi-v2512", s(&study), s(&named)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let plain_files = files_under(&plain);
    assert!(plain_files.contains_key("system/controlDict"));
    assert_eq!(plain_files, files_under(&named));

    // An id no backend has is named and nothing is written.
    let none = dir.join("none-case");
    let o = run(&[
        "compile",
        "--backend",
        "nope",
        s(&study),
        s(&none),
        "--json",
    ]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(
        findings(&json(&o))
            .iter()
            .any(|(c, sev)| c == "CFD-BACKEND-001" && sev == "Error"),
        "{}",
        out(&o)
    );
    assert!(!none.exists());
}

#[test]
fn a_study_a_backend_cannot_run_is_a_diagnostic_not_a_partial_case() {
    let dir = scratch("backend-cli-refused");
    let study = dir.join("step.json");
    let made = run(&[
        "benchmark",
        "create",
        "backward-step",
        "--turbulence",
        "realizable-ke",
        "--out",
        s(&study),
    ]);
    assert_eq!(code(&made), 0, "{}", err(&made));

    let case = dir.join("lbm-case");
    let o = run(&[
        "compile",
        "--backend",
        "fieldcraft-lbm-0.7.5",
        s(&study),
        s(&case),
        "--json",
    ]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(
        findings(&json(&o))
            .iter()
            .any(|(c, sev)| c == "CFD-BACKEND-002" && sev == "Error"),
        "{}",
        out(&o)
    );
    assert!(!case.exists(), "a refused study writes no case");
}

#[test]
fn run_with_an_unpinned_backend_is_refused_before_anything_is_uploaded() {
    let (dir, study) = cavity_file("backend-cli-unpinned");
    // The key and the hosts file do not exist: a run that got as far as the host would say so.
    let key = dir.join("no-such-key");
    let hosts = dir.join("no-such-hosts.toml");
    let o = run(&[
        "run",
        s(&study),
        "--key",
        s(&key),
        "--hosts",
        s(&hosts),
        "--backend",
        "su2-8.6.0",
    ]);
    assert_ne!(code(&o), 0, "{}", out(&o));
    let text = err(&o);
    assert!(text.contains("CFD-BACKEND-010"), "{text}");
    assert!(!text.contains("no-such-key"), "{text}");
    assert!(!text.contains("no-such-hosts"), "{text}");

    let o = run(&[
        "run",
        s(&study),
        "--key",
        s(&key),
        "--hosts",
        s(&hosts),
        "--backend",
        "nope",
    ]);
    assert_eq!(code(&o), 2, "{}", out(&o));
    assert!(err(&o).contains("CFD-BACKEND-001"), "{}", err(&o));
}

#[test]
fn compile_report_labels_preview_and_candidate() {
    let (dir, study) = cavity_file("backend-cli-labels");

    let case = dir.join("lbm-case");
    let o = run(&[
        "compile",
        "--backend",
        "fieldcraft-lbm-0.7.5",
        s(&study),
        s(&case),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let found = findings(&json(&o));
    for wanted in ["CFD-BACKEND-009", "CFD-BACKEND-008"] {
        assert!(
            found.iter().any(|(c, sev)| c == wanted && sev == "Warning"),
            "{wanted} is a warning: {found:?}"
        );
    }
    assert!(case.is_dir());

    // A candidate that is not a preview carries the candidate warning only.
    let su2 = dir.join("su2-case");
    let o = run(&[
        "compile",
        "--backend",
        "su2-8.6.0",
        s(&study),
        s(&su2),
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let codes: Vec<String> = findings(&json(&o)).into_iter().map(|(c, _)| c).collect();
    assert!(codes.contains(&"CFD-BACKEND-008".to_owned()), "{codes:?}");
    assert!(!codes.contains(&"CFD-BACKEND-009".to_owned()), "{codes:?}");

    // The default backend carries neither.
    let plain = dir.join("plain-case");
    let o = run(&["compile", s(&study), s(&plain), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let codes: Vec<String> = findings(&json(&o)).into_iter().map(|(c, _)| c).collect();
    assert!(
        !codes.iter().any(|c| c.starts_with("CFD-BACKEND-")),
        "{codes:?}"
    );
}
