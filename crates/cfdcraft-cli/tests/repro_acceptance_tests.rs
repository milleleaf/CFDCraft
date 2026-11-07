//! The acceptance claims of reproducible results, through the real binary against a fake ssh and
//! a fake docker. The fake host answers every run with one prepared results archive, so a run
//! records a mesh and a result without any solver. The same claims are proved on the operations
//! in `crates/cfdcraft-ops/tests/acceptance_tests.rs`.
//!
//! | Claim | Test |
//! |---|---|
//! | 1 and 3. A second solve starts nothing; an edit and a solve make a new run and keep the old provenance | `sc1_sc3_solve_edit_solve_through_the_binary` |
//! | 2. The lid speed stales the result and says why | `sc1_sc3_solve_edit_solve_through_the_binary` |
//! | 4. A rerun reproduces; updates follow the chosen order; a bad order is refused | `sc4_rerun_and_update_through_the_binary` |
//! | 5. Every verb prints one JSON report | `sc5_every_verb_prints_one_json_report` |
//! | The host is named nowhere in the output | `nothing_about_the_host_is_in_any_json_output` |
#![cfg(unix)]

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;
#[path = "../../cfdcraft-ops/tests/support/mod.rs"]
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_ops::save_document;
use repro_support::{FixtureRunner, edit, new_document, passed, scratch, with_lid_speed};
use serde_json::Value;
use support::{SSH_TARGET, events, new_root, workspace_root};

const GROUP: &str = "cli-repro-acceptance";

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

/// `<verb> <documents>` followed by the key and the hosts file of the fake box and `extra`.
fn on_box(root: &Path, verb: &str, docs: &[&Path], extra: &[&str]) -> Vec<String> {
    let mut args = vec![verb.to_owned()];
    args.extend(docs.iter().map(|doc| text(doc)));
    args.extend([
        "--key".to_owned(),
        text(&root.join("dummy-key")),
        "--hosts".to_owned(),
        text(&root.join("hosts.toml")),
    ]);
    args.extend(extra.iter().map(|word| (*word).to_owned()));
    args
}

/// A command line without the key and the hosts file.
fn plain(verb: &str, docs: &[&Path], extra: &[&str]) -> Vec<String> {
    let mut args = vec![verb.to_owned()];
    args.extend(docs.iter().map(|doc| text(doc)));
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

/// Standard output as exactly one JSON document.
fn one_json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is exactly one JSON document")
}

/// A new document made from the cavity, called `name` in `dir`.
fn cavity_document(dir: &Path, name: &str) -> PathBuf {
    new_document(dir, name, repro_support::CAVITY)
}

/// The cavity with another mesh of the same number of cells, so the prepared results still pass.
fn with_mesh(cells_x: u64, cells_y: u64) -> String {
    edit(repro_support::CAVITY, |study| {
        study["mesh"]["cells_x"] = Value::from(cells_x);
        study["mesh"]["cells_y"] = Value::from(cells_y);
    })
}

/// A document solved without a host, then given a mesh of other cells so both stages are out of
/// date.
fn solved_then_remeshed(dir: &Path, name: &str) -> PathBuf {
    let doc = cavity_document(dir, name);
    let mut runner = FixtureRunner::new(&dir.join(format!("runs-{name}")));
    passed(&cfdcraft_ops::solve_document(&doc, &mut runner).report);
    passed(&save_document(&with_mesh(64, 256), &doc, true).report);
    doc
}

#[test]
fn sc1_sc3_solve_edit_solve_through_the_binary() {
    let root = fresh("solve-edit");
    let dir = scratch("accept-solve-edit");
    let doc = cavity_document(&dir, "cavity");

    let first = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&first), 0, "{}", err(&first));
    assert!(
        out(&first).contains("Recorded run-1: mesh, result."),
        "{}",
        out(&first)
    );
    assert_eq!(containers(&root), 1, "{:?}", events(&root));

    let second = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&second), 0, "{}", err(&second));
    assert!(
        out(&second).contains("Nothing to run: the mesh and the result are up to date."),
        "{}",
        out(&second)
    );
    assert_eq!(containers(&root), 1, "{:?}", events(&root));

    let provenance = plain("provenance", &[&doc], &["--run", "run-1", "--json"]);
    let before = cli(&root, &provenance);
    assert_eq!(code(&before), 0, "{}", err(&before));

    // Edit the study with the binary: a faster lid.
    let study = dir.join("lid.json");
    fs::write(&study, with_lid_speed(0.2)).unwrap();
    let saved = cli(
        &root,
        &[
            "save".to_owned(),
            text(&study),
            text(&doc),
            "--replace".to_owned(),
        ],
    );
    assert_eq!(code(&saved), 0, "{}", err(&saved));

    let stale = cli(&root, &plain("status", &[&doc], &[]));
    assert_eq!(code(&stale), 0, "{}", err(&stale));
    assert!(
        err(&stale).contains("result stale because lid velocity 0.1 → 0.2 m/s"),
        "{}",
        err(&stale)
    );
    assert!(!err(&stale).contains("mesh stale"), "{}", err(&stale));

    let third = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&third), 0, "{}", err(&third));
    assert!(
        out(&third).contains("Recorded run-2: result."),
        "{}",
        out(&third)
    );
    assert_eq!(containers(&root), 2, "{:?}", events(&root));

    let listed = cli(&root, &plain("runs", &[&doc], &["--json"]));
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let ids: Vec<Value> = one_json(&listed)["repro"]["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|run| run["id"].clone())
        .collect();
    assert_eq!(ids, ["run-1", "run-2"]);

    // The first run's provenance is what it was before the edit.
    let after = cli(&root, &provenance);
    assert_eq!(code(&after), 0, "{}", err(&after));
    assert_eq!(one_json(&after)["repro"], one_json(&before)["repro"]);
}

#[test]
fn sc4_rerun_and_update_through_the_binary() {
    // A rerun from the provenance reproduces the original.
    let root = fresh("rerun");
    let doc = cavity_document(&scratch("accept-rerun"), "cavity");
    let first = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&first), 0, "{}", err(&first));
    let again = cli(&root, &on_box(&root, "rerun", &[&doc], &["--run", "run-1"]));
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        out(&again).contains("run-2 reproduced run-1: every checked quantity is identical."),
        "{}",
        out(&again)
    );
    assert_eq!(containers(&root), 2, "{:?}", events(&root));

    // Stale items are updated in the order given.
    let root = fresh("update");
    let dir = scratch("accept-update");
    let a = solved_then_remeshed(&dir, "a");
    let b = solved_then_remeshed(&dir, "b");
    let order = [
        "--order", "b:mesh", "--order", "a:mesh", "--order", "b:result", "--order", "a:result",
    ];
    let done = cli(&root, &on_box(&root, "update", &[&a, &b], &order));
    assert_eq!(code(&done), 0, "{}", err(&done));
    let said = out(&done);
    let mut at = 0;
    for sentence in [
        "Updated b:mesh in run-2.",
        "Updated a:mesh in run-2.",
        "Updated b:result in run-3.",
        "Updated a:result in run-3.",
    ] {
        let found = said[at..]
            .find(sentence)
            .unwrap_or_else(|| panic!("{sentence:?} is missing or out of order in {said}"));
        at += found + sentence.len();
    }
    assert_eq!(containers(&root), 2, "{:?}", events(&root));

    // An order that breaks a dependency is refused and starts nothing.
    let c = solved_then_remeshed(&dir, "c");
    let d = solved_then_remeshed(&dir, "d");
    let before = (fs::read(&c).unwrap(), fs::read(&d).unwrap());
    let backwards = [
        "--order", "c:result", "--order", "c:mesh", "--order", "d:mesh", "--order", "d:result",
    ];
    let refused = cli(&root, &on_box(&root, "update", &[&c, &d], &backwards));
    assert_eq!(code(&refused), 2, "{}", err(&refused));
    assert!(
        err(&refused).contains("c:result cannot come before c:mesh"),
        "{}",
        err(&refused)
    );
    assert_eq!(containers(&root), 2, "{:?}", events(&root));
    assert_eq!((fs::read(&c).unwrap(), fs::read(&d).unwrap()), before);
}

/// Every verb of the reproducibility flow, called with `--json` on one document in a fake box:
/// the label of the call and what the binary answered.
fn every_verb_as_json(tag: &str) -> (PathBuf, Vec<(&'static str, Output)>) {
    let root = fresh(tag);
    let dir = scratch(&format!("accept-{tag}"));
    let doc = cavity_document(&dir, "cavity");
    let study = dir.join("cavity.json");
    fs::write(&study, repro_support::CAVITY).unwrap();
    let mut calls = Vec::new();

    let solve = on_box(&root, "solve", &[&doc], &["--json"]);
    calls.push(("solve", cli(&root, &solve)));
    calls.push(("solve again", cli(&root, &solve)));
    let rerun = on_box(&root, "rerun", &[&doc], &["--run", "run-1", "--json"]);
    calls.push(("rerun", cli(&root, &rerun)));
    calls.push(("status", cli(&root, &plain("status", &[&doc], &["--json"]))));
    calls.push(("runs", cli(&root, &plain("runs", &[&doc], &["--json"]))));
    calls.push(("result", cli(&root, &plain("result", &[&doc], &["--json"]))));
    let provenance = plain("provenance", &[&doc], &["--json"]);
    calls.push(("provenance", cli(&root, &provenance)));
    let compare = plain("compare", &[&doc], &["run-1", "run-2", "--json"]);
    calls.push(("compare", cli(&root, &compare)));
    calls.push(("keys", cli(&root, &plain("keys", &[&study], &["--json"]))));

    // With a stale result there is something for an update to plan.
    passed(&save_document(&with_lid_speed(0.2), &doc, true).report);
    let plan = plain("update", &[&doc], &["--plan", "--json"]);
    calls.push(("update --plan", cli(&root, &plan)));
    (root, calls)
}

#[test]
fn sc5_every_verb_prints_one_json_report() {
    let (_, calls) = every_verb_as_json("json");
    let verbs: Vec<&str> = calls.iter().map(|(verb, _)| *verb).collect();
    assert_eq!(
        verbs,
        [
            "solve",
            "solve again",
            "rerun",
            "status",
            "runs",
            "result",
            "provenance",
            "compare",
            "keys",
            "update --plan"
        ]
    );

    for (verb, called) in &calls {
        assert_eq!(code(called), 0, "{verb}: {}", err(called));
        assert!(err(called).is_empty(), "{verb}: {}", err(called));
        let report = one_json(called);
        assert_eq!(report["exit_class"], code(called), "{verb}");
    }

    let by_name = |name: &str| {
        let (_, called) = calls.iter().find(|(verb, _)| *verb == name).unwrap();
        one_json(called)
    };
    let result = by_name("result");
    assert_eq!(result["repro"]["kind"], "result");
    assert_eq!(result["repro"]["checks"][0]["name"], "evidence");
    assert_eq!(result["repro"]["checks"][0]["passed"], true);
    assert!(result["repro"]["key_numbers"]["solver.iterations"].is_number());

    let provenance = by_name("provenance");
    assert_eq!(provenance["repro"]["kind"], "provenance");
    assert!(
        provenance["repro"]["provenance"]["tool"]["image_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(provenance["repro"]["provenance"]["host"], "box");

    assert_eq!(by_name("solve")["repro"]["kind"], "run");
    assert_eq!(
        by_name("rerun")["repro"]["comparison"]["verdict"],
        "reproduced"
    );
    assert_eq!(by_name("runs")["repro"]["kind"], "runs");
    assert_eq!(by_name("status")["repro"]["kind"], "status");
    assert_eq!(by_name("compare")["repro"]["kind"], "comparison");
    assert_eq!(
        by_name("update --plan")["repro"]["pending"],
        serde_json::json!(["cavity:result"])
    );
    assert!(by_name("keys")["artifacts"].is_array());
}

#[test]
fn nothing_about_the_host_is_in_any_json_output() {
    let (root, calls) = every_verb_as_json("leak");
    let everything: String = calls.iter().map(|(_, called)| out(called)).collect();
    assert!(everything.contains("\"repro\""), "{everything}");
    for host_text in [
        SSH_TARGET.to_owned(),
        text(&root.join("dummy-key")),
        work_dir(&root),
    ] {
        assert!(
            !everything.contains(&host_text),
            "the output names {host_text}"
        );
    }
}
