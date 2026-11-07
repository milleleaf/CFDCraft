//! The verbs for mesh independence, run through the real binary. The runs of each point are
//! written into the document as records, so no test needs a compute host and none of these verbs
//! starts a run.

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cfdcraft_model::resolve;
use cfdcraft_ops::scenario_ops::effective_of;
use cfdcraft_runs::{Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id};
use cfdcraft_store::ContentHash;
use repro_support::{CAVITY, edit, new_document, scratch};
use serde_json::{Value, json};

const SWEEP: &str = "gci-cavity";
const QUANTITY: &str = "test.quantity";
const POINTS: [&str; 3] = ["gci-cavity-001", "gci-cavity-002", "gci-cavity-003"];

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

/// A new document of the cavity with 32 cells along each side.
fn new_cavity(tag: &str) -> PathBuf {
    let study = edit(CAVITY, |study| {
        study["mesh"]["cells_x"] = json!(32);
        study["mesh"]["cells_y"] = json!(32);
    });
    new_document(&scratch(tag), "cavity", &study)
}

/// Append a run made for the variant `scenario` that recorded a result holding `value` for the
/// quantity.
fn record(path: &Path, scenario: &str, value: f64) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let (study, tag) = effective_of("gci", &document, Some(scenario)).unwrap();
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let id = next_run_id(&document.artifacts.runs);
    let bytes = format!("output of {id}").into_bytes();
    let record = RunRecord {
        id,
        rerun_of: None,
        scenario: tag,
        mesh: None,
        result: Some(cfdcraft_runs::Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob: ContentHash::of(&bytes),
            size: bytes.len() as u64,
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
        key_numbers: [(QUANTITY.to_owned(), value)].into_iter().collect(),
        comparison: None,
        continued_from: None,
        outcome: cfdcraft_runs::Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    };
    document.add_run(record).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
}

/// A document with the level sweep made by the command and one run for each point.
fn finished(tag: &str, values: [f64; 3]) -> PathBuf {
    let path = new_cavity(tag);
    let made = run(&["gci", "create", s(&path), "--sweep", SWEEP]);
    assert_eq!(code(&made), 0, "{}", err(&made));
    for (point, value) in POINTS.iter().zip(values) {
        record(&path, point, value);
    }
    path
}

#[test]
fn gci_create_prints_one_sentence() {
    let path = new_cavity("gci-cli-create");
    let o = run(&[
        "gci",
        "create",
        s(&path),
        "--sweep",
        SWEEP,
        "--scenario",
        "base",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Created the mesh independence sweep gci-cavity with 3 meshes."),
        "{}",
        out(&o)
    );

    let path = new_cavity("gci-cli-four");
    let o = run(&[
        "gci",
        "create",
        s(&path),
        "--sweep",
        SWEEP,
        "--levels",
        "0,1,2,3",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("with 4 meshes."), "{}", out(&o));

    for levels in ["2,1,0", "0,9", "0,1"] {
        let path = new_cavity("gci-cli-refused");
        let before = std::fs::read(&path).unwrap();
        let o = run(&[
            "gci",
            "create",
            s(&path),
            "--sweep",
            SWEEP,
            "--levels",
            levels,
        ]);
        assert_eq!(code(&o), 2, "{levels}: {}", err(&o));
        assert!(err(&o).contains("CFD-SWP-002"), "{levels}: {}", err(&o));
        assert_eq!(std::fs::read(&path).unwrap(), before, "{levels}");
    }

    let path = new_cavity("gci-cli-words");
    let o = run(&[
        "gci",
        "create",
        s(&path),
        "--sweep",
        SWEEP,
        "--levels",
        "a,b,c",
    ]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("whole numbers"), "{}", err(&o));
}

#[test]
fn gci_report_prints_the_summary_and_the_warnings() {
    let path = finished("gci-cli-report", [0.96178, 0.96854, 0.97050]);
    let o = run(&[
        "gci",
        "report",
        s(&path),
        "--sweep",
        SWEEP,
        "--quantity",
        QUANTITY,
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains(
            "Mesh independence of test.quantity: observed order 1.79, GCI of the fine mesh 0.10 % (monotone)."
        ),
        "{}",
        out(&o)
    );
    assert!(!err(&o).contains("oscillating"), "{}", err(&o));

    let o = run(&[
        "gci",
        "report",
        s(&path),
        "--sweep",
        SWEEP,
        "--quantity",
        QUANTITY,
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(err(&o).is_empty(), "{}", err(&o));
    let report: Value = serde_json::from_str(&out(&o)).expect("stdout is one JSON report");
    assert_eq!(report["repro"]["kind"], "gci");
    assert_eq!(report["repro"]["sweep"], SWEEP);
    assert_eq!(report["repro"]["reports"][0]["convergence"], "monotone");

    // Coarse to fine 0.9, 1.1, 1.0 changes sign, so the warning goes to standard error.
    let wavy = finished("gci-cli-wavy", [0.9, 1.1, 1.0]);
    let o = run(&[
        "gci",
        "report",
        s(&wavy),
        "--sweep",
        SWEEP,
        "--quantity",
        QUANTITY,
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("(oscillatory)."), "{}", out(&o));
    assert!(
        err(&o).contains("test.quantity converges in an oscillating way"),
        "{}",
        err(&o)
    );
}

#[test]
fn an_unfinished_sweep_exits_with_the_refusal_class() {
    let path = new_cavity("gci-cli-unfinished");
    let made = run(&["gci", "create", s(&path), "--sweep", SWEEP]);
    assert_eq!(code(&made), 0, "{}", err(&made));
    record(&path, POINTS[0], 0.96178);
    record(&path, POINTS[2], 0.97050);

    let o = run(&[
        "gci",
        "report",
        s(&path),
        "--sweep",
        SWEEP,
        "--quantity",
        QUANTITY,
    ]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(
        err(&o).contains("The mesh independence sweep gci-cavity is not finished: gci-cavity-002 have no recorded result."),
        "{}",
        err(&o)
    );
    assert!(err(&o).contains("CFD-GCI-001"), "{}", err(&o));
}
