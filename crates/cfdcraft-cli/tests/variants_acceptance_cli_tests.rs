//! The variant, sweep and disk criteria end to end through the real binary against a fake ssh and
//! a fake docker. The fake box answers every run with one prepared results archive, so a run
//! records a mesh and a result without any solver. A geometry variant here keeps the same number
//! of cells as the study (64 by 256 against 128 by 128), so the prepared results still pass the
//! evidence judge.
#![cfg(unix)]

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;
#[path = "../../cfdcraft-ops/tests/support/mod.rs"]
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use repro_support::{CAVITY, new_document, scratch, snapshot};
use serde_json::{Value, json};
use support::{SSH_TARGET, events, new_root, workspace_root};

const GROUP: &str = "cli-variants-acceptance";
const LID: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_X: &str = "/mesh/cells_x";
const CELLS_Y: &str = "/mesh/cells_y";

/// A fake box whose results download is a good run of the cavity.
fn fresh(tag: &str) -> PathBuf {
    let root = new_root(GROUP, tag);
    let src = root.join("results-src");
    let spec = FixtureSpec {
        end_time: 5000,
        cells: 16384,
        samples: 64,
        shift: 0.0,
    };
    write_results(&src, &spec).unwrap();
    let made = Command::new("tar")
        .arg("-C")
        .arg(&src)
        .arg("-cf")
        .arg(root.join("results.tar"))
        .arg(".")
        .status()
        .unwrap();
    assert!(made.success());
    root
}

/// The built binary with the fakes first on its PATH, run from `target/` so its socket folder is
/// short enough for a unix socket.
fn cli(root: &Path, args: &[String]) -> Output {
    let path = std::env::join_paths(
        std::iter::once(root.join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .env("PATH", path)
        .env("CFC_TEST_ROOT", root)
        .env("CFC_TEST_MODE", "ok")
        .env("CFC_TEST_RESULTS_TAR", root.join("results.tar"))
        .stdin(Stdio::null())
        .current_dir(workspace_root().join("target"))
        .output()
        .unwrap()
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// A command line from words.
fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

/// `solve <document>` followed by the key and the hosts file of the fake box and `extra`.
fn solve(root: &Path, doc: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = words(&["solve", &text(doc)]);
    args.extend([
        "--key".to_owned(),
        text(&root.join("dummy-key")),
        "--hosts".to_owned(),
        text(&root.join("hosts.toml")),
    ]);
    args.extend(extra.iter().map(|word| (*word).to_owned()));
    args
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

/// The data a verb printed on stdout.
fn data(o: &Output) -> Value {
    serde_json::from_str(&out(o)).expect("stdout is the JSON data")
}

/// The one report a verb printed with `--json`.
fn report(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is exactly one JSON document")
}

/// How many containers the fake docker was asked to start.
fn containers(root: &Path) -> usize {
    events(root).iter().filter(|e| *e == "run").count()
}

/// The remote work folder the test hosts file configures.
fn work_dir(root: &Path) -> String {
    let hosts = fs::read_to_string(root.join("hosts.toml")).unwrap();
    let line = hosts.lines().find(|l| l.starts_with("work_dir")).unwrap();
    line.split('"').nth(1).unwrap().to_owned()
}

/// A verb that must pass: its status is 0.
fn ok(root: &Path, args: &[String]) -> Output {
    let o = cli(root, args);
    assert_eq!(code(&o), 0, "{args:?}: {}", err(&o));
    o
}

fn parameter(name: &str, kind: &str, setting: &str, items: &[f64]) -> Value {
    json!({
        "name": name,
        "target": { "kind": kind, "setting": setting },
        "values": { "mode": "list", "items": items },
    })
}

/// Three lid speeds by two cell counts, six points.
fn grid() -> Value {
    json!({
        "id": "grid",
        "base": "base",
        "parameters": [
            parameter("lid_speed", "number", LID, &[0.1, 0.2, 0.3]),
            parameter("cells_x", "count", CELLS_X, &[20.0, 40.0]),
        ],
    })
}

/// A sweep file in `dir`.
fn spec_file(dir: &Path, name: &str, spec: &Value) -> PathBuf {
    let file = dir.join(name);
    fs::write(&file, spec.to_string()).unwrap();
    file
}

#[test]
fn sc1_sc2_through_the_binary() {
    let root = fresh("sc1-sc2");
    let doc = new_document(&scratch("acc-sc1-sc2"), "cavity", CAVITY);
    let d = text(&doc);

    // The study first, so the variants have a mesh to find.
    let first = ok(&root, &solve(&root, &doc, &[]));
    assert!(
        out(&first).contains("Recorded run-1: mesh, result."),
        "{}",
        out(&first)
    );
    assert_eq!(containers(&root), 1, "{:?}", events(&root));

    // A setting-only variant and a geometry variant of the same number of cells.
    let lid = format!("{LID}=0.2");
    let made = ok(
        &root,
        &words(&["scenario", "set", &d, "fast-lid", "--set", &lid]),
    );
    assert!(
        out(&made).contains("Created variant fast-lid of base."),
        "{}",
        out(&made)
    );
    let (x, y) = (format!("{CELLS_X}=64"), format!("{CELLS_Y}=256"));
    ok(
        &root,
        &words(&["scenario", "set", &d, "fine", "--set", &x, "--set", &y]),
    );
    let listed = data(&ok(&root, &words(&["scenario", "list", &d])));
    let kinds: Vec<(&str, &str)> = listed["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["id"].as_str().unwrap(), row["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("base", "base"),
            ("fast-lid", "setting"),
            ("fine", "geometry")
        ]
    );
    assert_eq!(listed["scenarios"][1]["mesh_shared_with"], "base");
    assert_eq!(listed["scenarios"][2]["mesh_shared_with"], Value::Null);

    // The diff reads in study terms.
    let said = ok(&root, &words(&["scenario", "diff", &d, "base", "fine"]));
    assert!(
        out(&said).contains("fine differs from base: "),
        "{}",
        out(&said)
    );
    let diff = report(&ok(
        &root,
        &words(&["scenario", "diff", &d, "base", "fine", "--json"]),
    ));
    let reasons: Vec<&str> = diff["repro"]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reason| reason.as_str().unwrap())
        .collect();
    assert!(reasons.contains(&"cells in x 128 → 64"), "{reasons:?}");
    assert!(reasons.contains(&"cells in y 128 → 256"), "{reasons:?}");

    // The setting-only variant records a result on the study's mesh; the other makes its own.
    let fast = ok(&root, &solve(&root, &doc, &["--scenario", "fast-lid"]));
    assert!(
        out(&fast).contains("Recorded run-2: result."),
        "{}",
        out(&fast)
    );
    let fine = ok(&root, &solve(&root, &doc, &["--scenario", "fine"]));
    assert!(
        out(&fine).contains("Recorded run-3: mesh, result."),
        "{}",
        out(&fine)
    );
    assert_eq!(containers(&root), 3, "{:?}", events(&root));

    // The runs carry the variant they were made for.
    let runs = data(&ok(&root, &words(&["runs", &d])));
    assert_eq!(runs["runs"][0]["scenario"], Value::Null);
    assert_eq!(runs["runs"][1]["scenario"], "fast-lid");
    assert_eq!(runs["runs"][2]["scenario"], "fine");
    assert_eq!(runs["runs"][1]["recorded"], json!(["result"]));
    assert_eq!(runs["runs"][2]["recorded"], json!(["mesh", "result"]));

    // Every variant now reads as fresh, on the mesh it stands on.
    for (scenario, mesh_run, result_run) in
        [("fast-lid", "run-1", "run-2"), ("fine", "run-3", "run-3")]
    {
        let status = report(&ok(
            &root,
            &words(&["status", &d, "--scenario", scenario, "--json"]),
        ));
        assert_eq!(status["repro"]["mesh"]["state"], "fresh", "{scenario}");
        assert_eq!(status["repro"]["mesh"]["run"], mesh_run, "{scenario}");
        assert_eq!(status["repro"]["result"]["state"], "fresh", "{scenario}");
        assert_eq!(status["repro"]["result"]["run"], result_run, "{scenario}");
    }

    // A second solve of each starts no container.
    let asks: [&[&str]; 3] = [&["--scenario", "fast-lid"], &["--scenario", "fine"], &[]];
    for extra in asks {
        let again = ok(&root, &solve(&root, &doc, extra));
        assert!(
            out(&again).contains("Nothing to run: the mesh and the result are up to date."),
            "{extra:?}: {}",
            out(&again)
        );
    }
    assert_eq!(containers(&root), 3, "{:?}", events(&root));
}

#[test]
fn sc3_through_the_binary() {
    let root = fresh("sc3");
    let dir = scratch("acc-sc3");
    let doc = new_document(&dir, "cavity", CAVITY);
    let d = text(&doc);
    let good = spec_file(&dir, "grid.json", &grid());
    let zero = json!({
        "id": "cells",
        "base": "base",
        "parameters": [parameter("cells_x", "count", CELLS_X, &[20.0, 0.0, 40.0])],
    });
    let zero = spec_file(&dir, "zero.json", &zero);
    let big = json!({
        "id": "big",
        "base": "base",
        "parameters": [{
            "name": "cells_x",
            "target": { "kind": "count", "setting": CELLS_X },
            "values": { "mode": "steps", "start": 20.0, "stop": 220.0, "count": 201 },
        }],
    });
    let big = spec_file(&dir, "big.json", &big);

    // A sound sweep is stored and passes its check.
    let made = ok(&root, &words(&["sweep", "create", &d, &text(&good)]));
    assert!(
        out(&made).contains("Created sweep grid: 6 variants."),
        "{}",
        out(&made)
    );
    let checked = data(&ok(&root, &words(&["sweep", "preflight", &d, "grid"])));
    assert_eq!(checked["failed"], 0);
    assert_eq!(checked["distinct_meshes"], 2);

    // A sweep with a point that cannot be built exits 1 and names the point.
    ok(&root, &words(&["sweep", "create", &d, &text(&zero)]));
    let failed = cli(&root, &words(&["sweep", "preflight", &d, "cells"]));
    assert_eq!(code(&failed), 1, "{}", err(&failed));
    assert!(err(&failed).contains("cells-002"), "{}", err(&failed));
    assert!(
        err(&failed).contains("at least one cell"),
        "{}",
        err(&failed)
    );
    assert!(out(&failed).is_empty(), "{}", out(&failed));

    // A sweep over the limit is refused by name, and nothing is stored for it.
    let before = snapshot(&doc);
    let refused = cli(&root, &words(&["sweep", "plan", &d, &text(&big)]));
    assert_eq!(code(&refused), 2, "{}", err(&refused));
    assert!(
        err(&refused).contains("Sweep big would make 201 points, more than the 200 allowed."),
        "{}",
        err(&refused)
    );
    assert_eq!(snapshot(&doc), before);

    // None of that started a container.
    assert_eq!(containers(&root), 0, "{:?}", events(&root));
    assert!(events(&root).is_empty(), "{:?}", events(&root));
}

#[test]
fn sc5_through_the_binary() {
    let root = fresh("sc5");
    let doc = new_document(&scratch("acc-sc5"), "cavity", CAVITY);
    let d = text(&doc);
    ok(&root, &solve(&root, &doc, &[]));
    assert_eq!(containers(&root), 1, "{:?}", events(&root));

    // Disk use is shown per variant.
    let used = data(&ok(&root, &words(&["disk", &d])));
    assert_eq!(used["scenarios"][0]["scenario"], "base");
    assert!(used["stored_bytes"].as_u64().unwrap() > 0);
    assert_eq!(used["pruned_bytes"], 0);

    // A pinned study keeps its results: even an applied prune removes nothing.
    ok(&root, &words(&["scenario", "label", &d, "base", "--pin"]));
    let kept = report(&ok(&root, &words(&["prune", &d, "--apply", "--json"])));
    assert_eq!(kept["repro"]["applied"], true);
    assert_eq!(kept["repro"]["plan"]["remove"], json!([]));
    assert!(kept["repro"]["plan"]["kept_pinned"].as_u64().unwrap() > 0);
    ok(&root, &words(&["scenario", "label", &d, "base", "--unpin"]));

    // Pruning is a dry run first.
    let before = snapshot(&doc);
    let dry = ok(&root, &words(&["prune", &d]));
    assert!(out(&dry).contains("Nothing was removed"), "{}", out(&dry));
    assert_eq!(snapshot(&doc), before);

    // Then it is applied.
    let done = ok(&root, &words(&["prune", &d, "--apply"]));
    assert!(out(&done).contains("Removed"), "{}", out(&done));
    assert_ne!(snapshot(&doc), before);
    let used = data(&ok(&root, &words(&["disk", &d])));
    assert!(used["pruned_bytes"].as_u64().unwrap() > 0);

    // The stages read as pruned, a solve leaves them alone, and a request makes them again.
    let status = report(&ok(&root, &words(&["status", &d, "--json"])));
    assert_eq!(status["repro"]["mesh"]["state"], "pruned");
    assert_eq!(status["repro"]["result"]["state"], "pruned");
    let idle = ok(&root, &solve(&root, &doc, &[]));
    assert!(
        out(&idle).contains("Nothing to run: the mesh and the result are up to date."),
        "{}",
        out(&idle)
    );
    assert_eq!(containers(&root), 1, "{:?}", events(&root));

    let remade = ok(&root, &solve(&root, &doc, &["--remake-pruned"]));
    assert!(
        out(&remade).contains("Recorded run-2: mesh, result."),
        "{}",
        out(&remade)
    );
    assert_eq!(containers(&root), 2, "{:?}", events(&root));
    let status = report(&ok(&root, &words(&["status", &d, "--json"])));
    assert_eq!(status["repro"]["mesh"]["state"], "fresh");
    assert_eq!(status["repro"]["result"]["state"], "fresh");

    // Every run record is still there.
    let runs = data(&ok(&root, &words(&["runs", &d])));
    assert_eq!(runs["runs"].as_array().unwrap().len(), 2);
}

#[test]
fn every_new_verb_prints_one_json_report_with_json() {
    let root = fresh("json");
    let dir = scratch("acc-json");
    let doc = new_document(&dir, "cavity", CAVITY);
    let d = text(&doc);
    let spec = text(&spec_file(&dir, "grid.json", &grid()));
    ok(&root, &solve(&root, &doc, &[]));
    let lid = format!("{LID}=0.2");
    let key = text(&root.join("dummy-key"));

    // Each verb of the family, and the verbs that gained a variant, with --json. The first group
    // names the document only as the caller gave it, the second names no path at all.
    let echoing = [
        words(&["status", &d, "--json", "--scenario", "base"]),
        solve(&root, &doc, &["--scenario", "base", "--json"]),
    ];
    let quiet = [
        words(&["scenario", "set", &d, "fast-lid", "--set", &lid, "--json"]),
        words(&["scenario", "list", &d, "--json"]),
        words(&["scenario", "show", &d, "fast-lid", "--json"]),
        words(&["scenario", "diff", &d, "base", "fast-lid", "--json"]),
        words(&["scenario", "label", &d, "fast-lid", "--pin", "--json"]),
        words(&["sweep", "plan", &d, &spec, "--json"]),
        words(&["sweep", "create", &d, &spec, "--json"]),
        words(&["sweep", "preflight", &d, "grid", "--json"]),
        words(&["disk", &d, "--json"]),
        words(&["prune", &d, "--json"]),
    ];
    let folder = text(doc.parent().unwrap());
    for (at, args) in echoing.iter().chain(quiet.iter()).enumerate() {
        let o = ok(&root, args);
        assert!(err(&o).is_empty(), "{args:?}: {}", err(&o));
        let printed = report(&o);
        assert!(printed.is_object(), "{args:?}");
        assert_eq!(printed["exit_class"], 0, "{args:?}");
        assert!(printed["operation"].is_string(), "{args:?}");
        let said = out(&o);
        for host_text in [SSH_TARGET.to_owned(), key.clone(), work_dir(&root)] {
            assert!(!said.contains(&host_text), "{args:?} names {host_text}");
        }
        if at >= echoing.len() {
            assert!(!said.contains(&folder), "{args:?}: {said}");
            assert!(!said.contains("cavity.cfdcraft"), "{args:?}: {said}");
        }
    }
    // A solve with nothing to run starts nothing, so nothing reached the host after the first.
    assert_eq!(containers(&root), 1, "{:?}", events(&root));
}
