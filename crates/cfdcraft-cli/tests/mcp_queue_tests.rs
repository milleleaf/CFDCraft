//! The queue over `cfdcraft mcp` as a real process: a solve is queued and answered with a job id, the
//! background runner is not owned by the server, and the new tools reach the queue and the live
//! view. The server runs without the attached-jobs setting here. Where a test must not have a
//! runner do anything, the hosts file is removed after the handshake, so a runner started by the
//! server ends at once and the job stays waiting.
#![cfg(unix)]

#[path = "../../cfdcraft-ops/tests/support/mod.rs"]
mod support;

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_ops::{CaseRun, CaseRunner, Report, save_document, solve_document};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::{Duration, Instant},
};
use support::*;

const GROUP: &str = "mcpq";

/// A running `cfdcraft mcp` and a client for it, as small as the tests here need.
struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    seen: HashMap<u64, Value>,
    next: u64,
}

fn fresh(tag: &str) -> PathBuf {
    assert!(
        tag.len() <= 9,
        "test tag too long for CI checkout paths: {tag}"
    );
    new_short_root(GROUP, tag)
}

fn document_file(root: &Path, name: &str) -> PathBuf {
    root.join("ws/documents").join(format!("{name}.cfdcraft"))
}

/// Save the cavity as the document `name` in the workspace the server will use.
fn put_document(root: &Path, name: &str) -> PathBuf {
    let path = document_file(root, name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let saved = save_document(CAVITY, &path, false);
    assert_eq!(saved.report.exit_class, 0, "{}", saved.report.render_text());
    path
}

impl Server {
    /// Start the server with `extra` arguments and complete the handshake.
    fn start(root: &Path, extra: &[&str]) -> Self {
        let path = std::env::join_paths(
            std::iter::once(root.join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let p = |n: &str| root.join(n).to_string_lossy().into_owned();
        let mut child = Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
            .arg("mcp")
            .args(["--workspace", &p("ws"), "--key", &p("dummy-key")])
            .args(["--hosts", &p("hosts.toml")])
            .args(extra)
            .env("PATH", path)
            .env("CFC_TEST_ROOT", root)
            .env("CFC_TEST_MODE", "ok")
            .current_dir(workspace_root().join("target"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // visible in a failing test's output: it says why a server stopped
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut s = Self {
            child,
            stdin,
            lines,
            seen: HashMap::new(),
            next: 1,
        };
        let init = s.call(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "0"}
            }),
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
        s.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        s
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(v) = self.seen.remove(&id) {
                return v;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    let v: Value = serde_json::from_str(&line)
                        .unwrap_or_else(|_| panic!("stdout carried a non-protocol line: {line:?}"));
                    if let Some(got) = v["id"].as_u64() {
                        self.seen.insert(got, v);
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no response with id {id}"),
                Err(RecvTimeoutError::Disconnected) => panic!("the server closed stdout"),
            }
        }
    }

    /// Call a tool and return the whole response, whether it is a result or an error.
    fn raw_tool(&mut self, name: &str, arguments: Value) -> Value {
        self.call("tools/call", json!({"name": name, "arguments": arguments}))
    }

    /// Call a tool and return its structured content; the call itself must not be a protocol error.
    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let r = self.raw_tool(name, arguments);
        assert!(r.get("error").is_none(), "{name}: {r}");
        assert_ne!(r["result"]["isError"], true, "{name}: {r}");
        r["result"]["structuredContent"].clone()
    }

    /// Close standard input and wait for the process to exit.
    fn close_and_wait(&mut self) -> std::process::ExitStatus {
        self.stdin = None;
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "the server did not exit in time"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A workspace holding the cavity as the document `cavity`, and a server on it whose runner cannot
/// start: the hosts file is gone once the handshake is done.
fn queue_server(tag: &str) -> (PathBuf, Server) {
    let root = fresh(tag);
    put_document(&root, "cavity");
    let s = Server::start(&root, &[]);
    std::fs::remove_file(root.join("hosts.toml")).unwrap();
    (root, s)
}

/// `queue.json` of the document `cavity`, as it is on disk.
fn queue_state(root: &Path) -> Value {
    let file = cfdcraft_doc::store_dir(&document_file(root, "cavity")).join("queue.json");
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Nothing a tool said names the workspace folder, the key, the hosts file or the ssh target.
fn assert_nothing_private(root: &Path, answer: &Value) {
    let text = answer.to_string();
    let folder = root.to_string_lossy();
    for needle in [folder.as_ref(), "dummy-key", "hosts.toml", SSH_TARGET] {
        assert!(!text.contains(needle), "{needle} is in {text}");
    }
}

/// Runs that write the results the checks want, for a document that is solved before the server
/// starts.
struct FixtureRunner {
    dir: PathBuf,
}

impl CaseRunner for FixtureRunner {
    fn run_case(&mut self, _study_json: &str) -> Result<CaseRun, Box<Report>> {
        let spec = FixtureSpec {
            end_time: 5000,
            cells: 16384,
            samples: 64,
            shift: 0.0,
        };
        write_results(&self.dir, &spec).unwrap();
        let checks = json!([{"name": "evidence", "passed": true, "detail": "passed"}]);
        Ok(CaseRun {
            results_dir: self.dir.clone(),
            checks: serde_json::from_value(checks).unwrap(),
            host: Some("box".to_owned()),
            np: None,
            stopped_at: None,
        })
    }
}

const NOTHING_TO_ADD: &str =
    "Nothing to add: everything asked for is up to date or already queued.";

#[test]
fn start_document_job_queues_a_solve_and_returns_a_job_id() {
    let (root, mut s) = queue_server("qstart");
    let r = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(r["status"], "started", "{r}");
    assert_eq!(r["run_id"], "q-001", "{r}");

    let status = s.tool("run_status", json!({"run_id": "q-001", "name": "cavity"}));
    assert_eq!(status["status"], "queued", "{status}");
    let by_search = s.tool("run_status", json!({"run_id": "q-001"}));
    assert_eq!(
        by_search["status"], "queued",
        "one document has it: {by_search}"
    );
    let result = s.tool("get_result", json!({"run_id": "q-001", "name": "cavity"}));
    assert_eq!(result["status"], "running", "not finished: {result}");
    assert!(result.get("report").is_none() && result.get("job").is_none());

    let queue = queue_state(&root);
    assert_eq!(queue["jobs"].as_array().unwrap().len(), 1, "{queue}");
    assert_eq!(queue["jobs"][0]["id"], "q-001");
    assert_eq!(queue["jobs"][0]["state"], "queued");
}

#[test]
fn a_second_start_adds_a_second_job_and_nothing_to_run_is_still_answered() {
    let (root, mut s) = queue_server("qtwo");
    let made = s.tool(
        "scenario_set",
        json!({
            "name": "cavity",
            "scenario": "fast",
            "overrides": [{"op": "set", "setting": "/boundary_conditions/lid/velocity/value", "value": 0.2}]
        }),
    );
    assert_eq!(made["exit_class"], 0, "{made}");
    let first = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(first["run_id"], "q-001", "{first}");
    let second = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve", "scenario": "fast"}),
    );
    assert_eq!(second["status"], "started", "{second}");
    assert_eq!(second["run_id"], "q-002", "{second}");

    // The same job asked for again while it waits is not added twice.
    let again = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve", "scenario": "fast"}),
    );
    assert_eq!(again["status"], "nothing_to_run", "{again}");
    assert_eq!(again["message"], NOTHING_TO_ADD, "{again}");
    assert!(again.get("run_id").is_none(), "{again}");
    assert_eq!(queue_state(&root)["jobs"].as_array().unwrap().len(), 2);
}

#[test]
fn a_solve_of_an_up_to_date_document_queues_nothing() {
    let root = fresh("qutd");
    let path = put_document(&root, "cavity");
    let mut runner = FixtureRunner {
        dir: root.join("fixture"),
    };
    let solved = solve_document(&path, &mut runner);
    assert_eq!(
        solved.report.exit_class,
        0,
        "{}",
        solved.report.render_text()
    );
    let mut s = Server::start(&root, &[]);
    std::fs::remove_file(root.join("hosts.toml")).unwrap();

    let r = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(r["status"], "nothing_to_run", "{r}");
    assert_eq!(r["message"], NOTHING_TO_ADD, "{r}");
    assert!(r.get("run_id").is_none(), "{r}");
    assert!(
        !cfdcraft_doc::store_dir(&path).join("queue.json").exists(),
        "nothing was queued"
    );
}

#[test]
fn rerun_and_updates_keep_the_attached_path() {
    let root = fresh("qrerun");
    put_document(&root, "cavity");
    // The hosts file stays: an update runs at once on the (fake) compute host.
    let mut s = Server::start(&root, &[]);

    let refused = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "rerun", "run": "run-9"}),
    );
    assert_eq!(refused["status"], "refused", "{refused}");
    assert_eq!(refused["report"]["exit_class"], 2, "{refused}");
    assert!(refused.get("run_id").is_none(), "{refused}");

    let started = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "update_mesh"}),
    );
    assert_eq!(started["status"], "started", "{started}");
    let id = started["run_id"].as_str().unwrap().to_owned();
    assert!(!id.starts_with("q-"), "a registry run id: {id}");
    wait_until("the update to end", || {
        s.tool("run_status", json!({"run_id": id}))["status"] == "finished"
    });
    assert!(
        !cfdcraft_doc::store_dir(&document_file(&root, "cavity"))
            .join("queue.json")
            .exists(),
        "nothing was queued"
    );
}

#[test]
fn attached_jobs_restores_the_earlier_behaviour() {
    let root = fresh("qattach");
    put_document(&root, "cavity");
    let mut s = Server::start(&root, &["--attached-jobs"]);

    hold_lock(&root, now_epoch(), 3600);
    let busy = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(busy["status"], "busy", "{busy}");
    assert_eq!(busy["holder"]["run_id"], "other-run-1", "{busy}");
    assert!(busy.get("run_id").is_none(), "{busy}");

    std::fs::remove_dir_all(lock_dir(&root)).unwrap();
    let started = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(started["status"], "started", "{started}");
    let id = started["run_id"].as_str().unwrap().to_owned();
    assert!(!id.starts_with("q-"), "a registry run id: {id}");
    wait_until("the solve to end", || {
        s.tool("run_status", json!({"run_id": id}))["status"] == "finished"
    });
    assert!(
        !cfdcraft_doc::store_dir(&document_file(&root, "cavity"))
            .join("queue.json")
            .exists(),
        "nothing was queued"
    );
}

#[test]
fn a_queued_job_survives_the_server_going_away() {
    let (root, mut s) = queue_server("qsurvive");
    let r = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(r["run_id"], "q-001", "{r}");
    assert!(s.close_and_wait().success());

    let queue = queue_state(&root);
    let jobs = queue["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1, "{queue}");
    assert_eq!(jobs[0]["state"], "queued", "no cancel was written: {queue}");
    assert!(jobs[0].get("stop").is_none(), "{queue}");
}

#[test]
fn the_start_answer_names_no_path_or_host() {
    let (root, mut s) = queue_server("qprivate");
    for arguments in [
        json!({"name": "cavity", "job": "solve"}),
        json!({"name": "cavity", "job": "solve"}),
        json!({"name": "absent", "job": "solve"}),
        json!({"name": "cavity", "job": "solve", "scenario": "nope"}),
    ] {
        let r = s.tool("start_document_job", arguments);
        assert_nothing_private(&root, &r);
    }
    let status = s.tool("run_status", json!({"run_id": "q-001", "name": "cavity"}));
    assert_nothing_private(&root, &status);
}

// ---- the seven tools ----

const LID_SETTING: &str = "/boundary_conditions/lid/velocity/value";
const CELLS_SETTING: &str = "/mesh/cells_x";

/// The codes of the findings of a report.
fn codes(report: &Value) -> Vec<&str> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect()
}

/// The jobs a queue report lists, as (id, state) pairs in queue order.
fn jobs_of(report: &Value) -> Vec<(String, String)> {
    report["repro"]["jobs"]
        .as_array()
        .unwrap_or_else(|| panic!("a queue report: {report}"))
        .iter()
        .map(|j| {
            (
                j["id"].as_str().unwrap().to_owned(),
                j["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn live_dir(root: &Path, job: &str) -> PathBuf {
    cfdcraft_doc::store_dir(&document_file(root, "cavity"))
        .join("live")
        .join(job)
}

/// Change `queue.json` of the document `cavity` the way a runner would, while no runner lives.
fn edit_queue(root: &Path, change: impl FnOnce(&mut Value)) {
    let file = cfdcraft_doc::store_dir(&document_file(root, "cavity")).join("queue.json");
    let mut queue = queue_state(root);
    change(&mut queue);
    std::fs::write(file, serde_json::to_string_pretty(&queue).unwrap()).unwrap();
}

/// The job looks as a runner leaves a job that is running on the host.
fn make_running(root: &Path, job: &str) {
    edit_queue(root, |queue| {
        let found = queue["jobs"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|j| j["id"] == job)
            .unwrap();
        found["state"] = json!("running");
        found["run_id"] = json!("cfdcraft-cavity-1-2");
    });
}

/// What a runner has copied of a job: its specification, 60 iterations of log with one line that
/// names a host path and a run, and the files of one probe.
fn write_live(root: &Path, job: &str) {
    let live = live_dir(root, job);
    let probe = live.join("postProcessing/probe_1/0");
    std::fs::create_dir_all(&probe).unwrap();
    let spec = json!({
        "spec": {
            "probes": [{"fo": "probe_1", "label": "centre", "field": "Velocity"}],
            "lines": [],
            "surfaces": [],
            "patches": [],
            "solves_energy": false,
            "residual_limits": null,
            "transient": false
        },
        "settings": {}
    });
    std::fs::write(live.join("spec.json"), spec.to_string()).unwrap();
    let mut log = String::new();
    for step in 1..=60 {
        log.push_str(&format!("Time = {step}\n\n"));
    }
    log.push_str("Reading /home/user/secret/path from cfdcraft-cavity-1-2 done\n");
    std::fs::write(live.join("log.simpleFoam"), log).unwrap();
    let mut rows = "# Probe 0 (0.05 0.05 0.025)\n#       Probe 0\n#          Time\n".to_owned();
    for step in 1..=300 {
        let ux = 0.01 + 1e-5 / f64::from(step);
        rows.push_str(&format!("{step} ({ux:.12} -0.020000000000 0)\n"));
    }
    std::fs::write(probe.join("U"), rows).unwrap();
}

/// A server with a job running and its live view copied, and nothing that can reach a host.
fn running_server(tag: &str) -> (PathBuf, Server) {
    let (root, mut s) = queue_server(tag);
    let r = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(r["run_id"], "q-001", "{r}");
    make_running(&root, "q-001");
    write_live(&root, "q-001");
    (root, s)
}

#[test]
fn queue_list_returns_the_jobs() {
    let (root, mut s) = queue_server("qlist");
    s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    let r = s.tool("queue_list", json!({"name": "cavity"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    assert_eq!(r["repro"]["kind"], "queue", "{r}");
    let job = &r["repro"]["jobs"][0];
    assert_eq!(job["id"], "q-001", "{r}");
    assert_eq!(job["state"], "queued");
    assert_eq!(job["kind"], "solve");
    for key in ["scenario", "run", "code", "progress_line"] {
        assert!(job.get(key).is_some_and(Value::is_null), "{key}: {r}");
    }
    assert_eq!(r["repro"]["drain"], false, "no runner lives: {r}");

    let unknown = s.tool("queue_list", json!({"name": "absent"}));
    assert_eq!(unknown["exit_class"], 2, "{unknown}");
    assert_nothing_private(&root, &unknown);
}

#[test]
fn queue_add_adds_a_variant_a_sweep_and_a_continue() {
    let (_root, mut s) = queue_server("qadd");
    let made = s.tool(
        "scenario_set",
        json!({
            "name": "cavity",
            "scenario": "fast",
            "overrides": [{"op": "set", "setting": LID_SETTING, "value": 0.2}]
        }),
    );
    assert_eq!(made["exit_class"], 0, "{made}");
    let one = s.tool("queue_add", json!({"name": "cavity", "scenario": "fast"}));
    assert_eq!(one["exit_class"], 0, "{one}");
    assert_eq!(jobs_of(&one), [("q-001".to_owned(), "queued".to_owned())]);
    assert_eq!(one["repro"]["jobs"][0]["scenario"], "fast");

    let sweep = json!({
        "id": "cells",
        "base": "base",
        "parameters": [{
            "name": "cells_x",
            "target": {"kind": "count", "setting": CELLS_SETTING},
            "values": {"mode": "list", "items": [20, 40]}
        }]
    });
    let stored = s.tool("sweep_create", json!({"name": "cavity", "sweep": sweep}));
    assert_eq!(stored["exit_class"], 0, "{stored}");
    let points = s.tool("queue_add", json!({"name": "cavity", "sweep": "cells"}));
    assert_eq!(points["exit_class"], 0, "{points}");
    assert_eq!(jobs_of(&points).len(), 3, "{points}");
    assert!(points["repro"]["jobs"][2]["origin"].is_string(), "{points}");

    for arguments in [
        json!({"name": "cavity", "scenario": "fast", "sweep": "cells"}),
        json!({"name": "cavity", "run": "run-1", "scenario": "fast"}),
    ] {
        let both = s.tool("queue_add", arguments.clone());
        assert_eq!(both["exit_class"], 2, "{arguments}: {both}");
        assert_eq!(codes(&both), ["CFD-QUEUE-001"], "{both}");
    }
    let none = s.tool("queue_add", json!({"name": "cavity", "run": "run-1"}));
    assert_eq!(none["exit_class"], 2, "no run to continue: {none}");
    let after = s.tool("queue_list", json!({"name": "cavity"}));
    assert_eq!(jobs_of(&after).len(), 3, "a refusal adds nothing: {after}");
}

#[test]
fn queue_reorder_and_cancel_work_on_waiting_jobs() {
    let (root, mut s) = queue_server("qmove");
    let made = s.tool(
        "scenario_set",
        json!({
            "name": "cavity",
            "scenario": "fast",
            "overrides": [{"op": "set", "setting": LID_SETTING, "value": 0.2}]
        }),
    );
    assert_eq!(made["exit_class"], 0, "{made}");
    s.tool("queue_add", json!({"name": "cavity"}));
    s.tool("queue_add", json!({"name": "cavity", "scenario": "fast"}));

    let moved = s.tool(
        "queue_reorder",
        json!({"name": "cavity", "job": "q-002", "position": 1}),
    );
    assert_eq!(moved["exit_class"], 0, "{moved}");
    let order: Vec<String> = jobs_of(&moved).into_iter().map(|(id, _)| id).collect();
    assert_eq!(order, ["q-002", "q-001"], "{moved}");

    let cancelled = s.tool("queue_cancel", json!({"name": "cavity", "job": "q-001"}));
    assert_eq!(cancelled["exit_class"], 0, "{cancelled}");
    assert!(
        jobs_of(&cancelled).contains(&("q-001".to_owned(), "cancelled".to_owned())),
        "{cancelled}"
    );
    let again = s.tool("queue_cancel", json!({"name": "cavity", "job": "q-001"}));
    assert_eq!(codes(&again), ["CFD-QUEUE-003"], "{again}");

    make_running(&root, "q-002");
    let running = s.tool(
        "queue_reorder",
        json!({"name": "cavity", "job": "q-002", "position": 1}),
    );
    assert_eq!(running["exit_class"], 2, "{running}");
    assert_eq!(codes(&running), ["CFD-QUEUE-003"], "{running}");
    let unknown = s.tool(
        "queue_reorder",
        json!({"name": "cavity", "job": "q-009", "position": 1}),
    );
    assert_eq!(codes(&unknown), ["CFD-QUEUE-002"], "{unknown}");
}

#[test]
fn run_stop_asks_a_running_job_to_write_and_stop() {
    let (root, mut s) = running_server("qstop");
    let r = s.tool("run_stop", json!({"name": "cavity", "job": "q-001"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    let queue = queue_state(&root);
    assert_eq!(queue["jobs"][0]["stop"]["mode"], "write_now", "{queue}");
    assert_eq!(queue["jobs"][0]["stop"]["cause"], "user", "{queue}");
    assert_eq!(
        queue["jobs"][0]["state"], "running",
        "the runner carries it out"
    );

    // A job that is only waiting cannot be asked.
    s.tool(
        "scenario_set",
        json!({
            "name": "cavity",
            "scenario": "fast",
            "overrides": [{"op": "set", "setting": LID_SETTING, "value": 0.2}]
        }),
    );
    // The active job holds the one place, so the second job waits.
    let waiting = s.tool("queue_add", json!({"name": "cavity", "scenario": "fast"}));
    assert_eq!(jobs_of(&waiting).len(), 2, "{waiting}");
    let r = s.tool("run_stop", json!({"name": "cavity", "job": "q-002"}));
    assert_eq!(r["exit_class"], 2, "{r}");
    assert_eq!(codes(&r), ["CFD-QUEUE-003"], "{r}");
}

#[test]
fn run_monitors_returns_the_table_for_a_job_or_a_recorded_run() {
    let (root, mut s) = running_server("qmon");
    let r = s.tool("run_monitors", json!({"name": "cavity", "job": "q-001"}));
    assert_eq!(r["status"], "ok", "{r}");
    assert_eq!(r["job"], "q-001");
    assert_eq!(r["state"], "running");
    assert!(r["iterations"].as_u64().unwrap() > 0, "{r}");
    let rows = r["rows"].as_array().unwrap();
    let names: Vec<&str> = rows
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"probe_1.Ux") && names.contains(&"probe_1.Uy"),
        "{r}"
    );
    let known = [
        "settled",
        "drifting",
        "oscillating",
        "too_early",
        "too_few_samples",
        "error",
    ];
    for row in rows {
        assert!(known.contains(&row["state"].as_str().unwrap()), "{row}");
        for key in ["last", "tail_mean", "spread"] {
            assert!(row[key].is_number(), "{key}: {row}");
        }
        assert!(
            row["label"].as_str().unwrap().starts_with("centre"),
            "{row}"
        );
    }
    let verdict = r["verdict"]["state"].as_str().unwrap();
    assert!(
        ["converged", "not_converged", "not_judged"].contains(&verdict),
        "{r}"
    );
    assert!(r["mass_balance"]["state"].is_string() && r["energy_balance"]["state"].is_string());

    // With neither job nor run the active job is shown.
    let active = s.tool("run_monitors", json!({"name": "cavity"}));
    assert_eq!(active["job"], "q-001", "{active}");
    assert!(ssh_log(&root).is_empty(), "nothing reached the host");
    assert_nothing_private(&root, &r);
}

#[test]
fn a_recorded_run_has_its_table_made_again_from_its_stored_result() {
    let root = fresh("qrecord");
    let path = put_document(&root, "cavity");
    let mut runner = FixtureRunner {
        dir: root.join("fixture"),
    };
    let solved = solve_document(&path, &mut runner);
    assert_eq!(
        solved.report.exit_class,
        0,
        "{}",
        solved.report.render_text()
    );
    let mut s = Server::start(&root, &[]);
    std::fs::remove_file(root.join("hosts.toml")).unwrap();

    let r = s.tool("run_monitors", json!({"name": "cavity", "run": "run-1"}));
    assert_eq!(r["status"], "ok", "{r}");
    assert_eq!(r["run"], "run-1");
    assert!(r.get("job").is_none(), "{r}");
    let verdict = r["verdict"]["state"].as_str().unwrap();
    assert!(
        ["converged", "not_converged", "not_judged"].contains(&verdict),
        "{r}"
    );
    assert!(
        r["recorded_verdict"].is_object(),
        "the verdict kept with the run: {r}"
    );

    let unknown = s.tool("run_monitors", json!({"name": "cavity", "run": "run-9"}));
    assert_eq!(unknown["status"], "refused", "{unknown}");
    assert_eq!(unknown["code"], "CFD-REPRO-004", "{unknown}");
    assert!(ssh_log(&root).is_empty(), "nothing reached the host");
    assert_nothing_private(&root, &r);
}

#[test]
fn run_log_returns_a_redacted_tail_and_the_next_offset() {
    let (root, mut s) = running_server("qlog");
    let first = s.tool(
        "run_log",
        json!({"name": "cavity", "job": "q-001", "offset": 0, "lines": 4}),
    );
    assert_eq!(first["status"], "ok", "{first}");
    assert_eq!(
        // four lines, the last one empty: `lines()` would drop it
        first["text"].as_str().unwrap().split('\n').count(),
        4,
        "{first}"
    );
    assert!(first["next_offset"].as_u64().unwrap() > 0, "{first}");

    let (mut offset, mut all) = (0, String::new());
    loop {
        let piece = s.tool(
            "run_log",
            json!({"name": "cavity", "job": "q-001", "offset": offset, "lines": 50}),
        );
        let next = piece["next_offset"].as_u64().unwrap();
        if next == offset {
            break;
        }
        all.push_str(piece["text"].as_str().unwrap());
        all.push('\n');
        offset = next;
    }
    assert!(all.contains("Time = 60"), "{all}");
    assert!(all.contains("Reading <path> from <run> done"), "{all}");
    assert!(
        !all.contains("/home/user") && !all.contains("cfdcraft-cavity"),
        "{all}"
    );
    assert!(ssh_log(&root).is_empty(), "nothing reached the host");
}

#[test]
fn every_new_tool_validates_its_names() {
    let (root, mut s) = running_server("qnames");
    for (tool, arguments, code) in [
        (
            "run_log",
            json!({"name": "a/b", "job": "q-001"}),
            "CFD-WS-001",
        ),
        (
            "run_log",
            json!({"name": "cavity", "job": "../x"}),
            "CFD-QUEUE-002",
        ),
        (
            "run_log",
            json!({"name": "cavity", "job": "q-001", "lines": 0}),
            "CFD-QUEUE-001",
        ),
        (
            "run_log",
            json!({"name": "cavity", "job": "q-001", "lines": 201}),
            "CFD-QUEUE-001",
        ),
        ("run_monitors", json!({"name": "a/b"}), "CFD-WS-001"),
        (
            "run_monitors",
            json!({"name": "cavity", "job": "../x"}),
            "CFD-QUEUE-002",
        ),
        (
            "run_monitors",
            json!({"name": "cavity", "run": "../x"}),
            "CFD-REPRO-004",
        ),
        (
            "run_monitors",
            json!({"name": "cavity", "job": "q-001", "run": "run-1"}),
            "CFD-QUEUE-001",
        ),
    ] {
        let r = s.tool(tool, arguments.clone());
        assert_eq!(r["status"], "refused", "{tool} {arguments}: {r}");
        assert_eq!(r["code"], code, "{tool} {arguments}: {r}");
        assert_nothing_private(&root, &r);
    }
    for (tool, arguments, code) in [
        ("queue_list", json!({"name": "../x"}), "CFD-WS-001"),
        ("queue_add", json!({"name": "a/b"}), "CFD-WS-001"),
        (
            "queue_reorder",
            json!({"name": "cavity", "job": "../x", "position": 1}),
            "CFD-QUEUE-002",
        ),
        (
            "queue_cancel",
            json!({"name": "a/b", "job": "q-001"}),
            "CFD-WS-001",
        ),
        (
            "run_stop",
            json!({"name": "cavity", "job": "../x"}),
            "CFD-QUEUE-002",
        ),
    ] {
        let r = s.tool(tool, arguments.clone());
        assert_eq!(r["exit_class"], 2, "{tool} {arguments}: {r}");
        assert_eq!(codes(&r), [code], "{tool} {arguments}: {r}");
    }
}

#[test]
fn outputs_are_closed_json_with_no_host_text() {
    let (root, mut s) = running_server("qclosed");
    for (tool, arguments) in [
        ("queue_list", json!({"name": "cavity"})),
        ("queue_add", json!({"name": "cavity"})),
        (
            "queue_reorder",
            json!({"name": "cavity", "job": "q-001", "position": 1}),
        ),
        ("queue_cancel", json!({"name": "cavity", "job": "q-009"})),
        ("run_stop", json!({"name": "cavity", "job": "q-001"})),
        ("run_monitors", json!({"name": "cavity", "job": "q-001"})),
        ("run_log", json!({"name": "cavity", "job": "q-001"})),
    ] {
        let r = s.tool(tool, arguments.clone());
        assert_nothing_private(&root, &r);
        // No input of any of these names a path, a key, a host or a command.
        for extra in ["path", "key", "host", "command", "dictionary"] {
            let mut bad = arguments.clone();
            bad[extra] = json!("x");
            let refused = s.raw_tool(tool, bad);
            assert!(
                refused.get("error").is_some() || refused["result"]["isError"] == true,
                "{tool} accepted `{extra}`: {refused}"
            );
        }
    }
    let listed = s.call("tools/list", json!({}));
    for tool in listed["result"]["tools"].as_array().unwrap() {
        assert!(tool["outputSchema"].is_object(), "{}", tool["name"]);
    }
}

#[test]
fn the_new_tools_are_used_without_the_attached_setting() {
    let (_root, mut s) = queue_server("qdefault");
    let listed = s.call("tools/list", json!({}));
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 43, "{names:?}");
    for tool in [
        "queue_list",
        "queue_add",
        "queue_reorder",
        "queue_cancel",
        "run_stop",
        "run_monitors",
        "run_log",
    ] {
        assert!(names.contains(&tool), "{tool} is advertised");
    }
    let added = s.tool("queue_add", json!({"name": "cavity"}));
    assert_eq!(jobs_of(&added), [("q-001".to_owned(), "queued".to_owned())]);
}
