//! The verbs that run a study file's case on the compute host, run through the real binary
//! against a fake ssh and a fake docker. The fake host answers every run with one prepared
//! results archive, so a run records a mesh and a result without any solver.
#![cfg(unix)]

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;
#[path = "../../cfdcraft-ops/tests/support/mod.rs"]
mod support;

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_ops::save_document;
use repro_support::{FixtureRunner, edit, new_document, passed, scratch, with_lid_speed};
use serde_json::Value;
use support::{SSH_TARGET, events, hold_lock, new_root, now_epoch, workspace_root};
use zip::ZipArchive;

const GROUP: &str = "cli-repro-host";

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
fn solve_records_a_run_and_a_second_solve_starts_nothing() {
    let root = fresh("solve");
    let doc = cavity_document(&scratch("host-solve"), "cavity");

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
}

#[test]
fn a_lid_change_makes_status_stale_and_solve_runs_once_more() {
    let root = fresh("lid");
    let doc = cavity_document(&scratch("host-lid"), "cavity");
    let first = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&first), 0, "{}", err(&first));

    passed(&save_document(&with_lid_speed(0.2), &doc, true).report);
    let stale = cli(&root, &plain("status", &[&doc], &[]));
    assert_eq!(code(&stale), 0, "{}", err(&stale));
    assert!(
        err(&stale).contains("result stale because lid velocity 0.1 → 0.2 m/s"),
        "{}",
        err(&stale)
    );

    let again = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(
        out(&again).contains("Recorded run-2: result."),
        "{}",
        out(&again)
    );
    assert_eq!(containers(&root), 2, "{:?}", events(&root));

    let fresh_again = cli(&root, &plain("status", &[&doc], &[]));
    assert_eq!(code(&fresh_again), 0, "{}", err(&fresh_again));
    assert!(
        out(&fresh_again).contains("Nothing is out of date."),
        "{}",
        out(&fresh_again)
    );
}

#[test]
fn rerun_compares_with_the_original() {
    let root = fresh("rerun");
    let doc = cavity_document(&scratch("host-rerun"), "cavity");
    let first = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&first), 0, "{}", err(&first));

    let again = cli(&root, &on_box(&root, "rerun", &[&doc], &["--run", "run-1"]));
    assert_eq!(code(&again), 0, "{}", err(&again));
    assert!(out(&again).contains("reproduced run-1"), "{}", out(&again));
    assert_eq!(containers(&root), 2, "{:?}", events(&root));

    let listed = cli(&root, &plain("runs", &[&doc], &[]));
    assert_eq!(code(&listed), 0, "{}", err(&listed));
    let data: Value = serde_json::from_str(&out(&listed)).unwrap();
    assert_eq!(data["runs"][1]["id"], "run-2");
    assert_eq!(data["runs"][1]["rerun_of"], "run-1");
}

#[test]
fn update_runs_each_document_once_and_plan_needs_no_host() {
    let root = fresh("update");
    let dir = scratch("host-update");
    let a = solved_then_remeshed(&dir, "a");
    let b = solved_then_remeshed(&dir, "b");
    let items = ["a:mesh", "a:result", "b:mesh", "b:result"];

    let shown = cli(&root, &plain("update", &[&a, &b], &["--plan"]));
    assert_eq!(code(&shown), 0, "{}", err(&shown));
    let plan: Value = serde_json::from_str(&out(&shown)).unwrap();
    assert_eq!(plan["pending"], serde_json::json!(items));
    assert_eq!(plan["order"], serde_json::json!(items));
    assert_eq!(containers(&root), 0, "{:?}", events(&root));

    let done = cli(&root, &on_box(&root, "update", &[&a, &b], &[]));
    assert_eq!(code(&done), 0, "{}", err(&done));
    let said = out(&done);
    for sentence in [
        "Updated a:mesh in run-2.",
        "Updated a:result in run-3.",
        "Updated b:mesh in run-2.",
        "Updated b:result in run-3.",
    ] {
        assert!(said.contains(sentence), "{said}");
    }
    assert_eq!(containers(&root), 2, "{:?}", events(&root));
    for doc in [&a, &b] {
        let status = cli(&root, &plain("status", &[doc], &[]));
        assert_eq!(code(&status), 0, "{}", err(&status));
        assert!(
            out(&status).contains("Nothing is out of date."),
            "{}",
            out(&status)
        );
    }
}

#[test]
fn update_takes_items_and_an_order() {
    let root = fresh("items");
    let dir = scratch("host-items");
    let a = solved_then_remeshed(&dir, "a");
    let b = solved_then_remeshed(&dir, "b");

    let only = cli(
        &root,
        &on_box(&root, "update", &[&a], &["--item", "a:mesh"]),
    );
    assert_eq!(code(&only), 0, "{}", err(&only));
    assert!(
        out(&only).contains("Updated a:mesh in run-2."),
        "{}",
        out(&only)
    );
    assert_eq!(containers(&root), 1, "{:?}", events(&root));
    let waiting = cli(&root, &plain("update", &[&a], &["--plan"]));
    let plan: Value = serde_json::from_str(&out(&waiting)).unwrap();
    assert_eq!(plan["pending"], serde_json::json!(["a:result"]));

    let before = fs::read(&b).unwrap();
    let backwards = cli(
        &root,
        &on_box(
            &root,
            "update",
            &[&b],
            &[
                "--item", "b:mesh", "--item", "b:result", "--order", "b:result", "--order",
                "b:mesh",
            ],
        ),
    );
    assert_eq!(code(&backwards), 2, "{}", err(&backwards));
    assert!(
        err(&backwards).contains("cannot come before"),
        "{}",
        err(&backwards)
    );
    assert_eq!(containers(&root), 1, "{:?}", events(&root));
    assert_eq!(fs::read(&b).unwrap(), before);
}

#[test]
fn a_busy_host_is_class_4_and_records_nothing() {
    let root = fresh("busy");
    let doc = cavity_document(&scratch("host-busy"), "cavity");
    let before = fs::read(&doc).unwrap();
    hold_lock(&root, now_epoch(), 3600);

    let o = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&o), 4, "{}", err(&o));
    assert_eq!(fs::read(&doc).unwrap(), before);
    assert_eq!(containers(&root), 0, "{:?}", events(&root));
}

#[test]
fn update_without_a_key_needs_plan() {
    let root = fresh("nokey");
    let dir = scratch("host-nokey");
    let a = solved_then_remeshed(&dir, "a");

    let o = cli(&root, &plain("update", &[&a], &[]));
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("error:"), "{}", err(&o));
    assert_eq!(containers(&root), 0, "{:?}", events(&root));
}

#[test]
fn the_document_holds_only_the_profile_name_of_the_host() {
    let root = fresh("label");
    let doc = cavity_document(&scratch("host-label"), "cavity");
    let o = cli(&root, &on_box(&root, "solve", &[&doc], &[]));
    assert_eq!(code(&o), 0, "{}", err(&o));

    let mut archive = ZipArchive::new(Cursor::new(fs::read(&doc).unwrap())).unwrap();
    let mut index = String::new();
    archive
        .by_name("artifacts.json")
        .unwrap()
        .read_to_string(&mut index)
        .unwrap();
    assert!(index.contains("\"host\": \"box\""), "{index}");
    for host_text in [
        SSH_TARGET.to_owned(),
        text(&root.join("dummy-key")),
        work_dir(&root),
    ] {
        assert!(
            !index.contains(&host_text),
            "the document names {host_text}"
        );
    }
}
