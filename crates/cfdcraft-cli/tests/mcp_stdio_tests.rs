//! `cfdcraft mcp` as a real process: the protocol handshake, the typed operations, and what the
//! server does when its client goes away. The compute host is a fake ssh and a fake docker.
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

const GROUP: &str = "mcp";
const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

/// A running `cfdcraft mcp` and a client for it. Responses are matched by id because the server
/// may answer out of order; every line the server wrote is kept so the test can check that
/// standard output carried protocol messages only.
struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    seen: HashMap<u64, Value>,
    raw: Vec<String>,
    next: u64,
}

fn spawn(root: &Path, docker_mode: &str, extra: &[&str]) -> Command {
    let path = std::env::join_paths(
        std::iter::once(root.join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_cfdcraft"));
    c.arg("mcp")
        .args(extra)
        .env("PATH", path)
        .env("CFC_TEST_ROOT", root)
        .env("CFC_TEST_MODE", docker_mode)
        .current_dir(workspace_root().join("target"));
    c
}

/// The standard arguments: the workspace under the fake box, its key and its hosts file.
fn args(root: &Path) -> Vec<String> {
    let p = |n: &str| root.join(n).to_string_lossy().into_owned();
    vec![
        "--workspace".into(),
        p("ws"),
        "--key".into(),
        p("dummy-key"),
        "--hosts".into(),
        p("hosts.toml"),
    ]
}

impl Server {
    /// Start the server and complete the handshake.
    fn start(root: &Path, docker_mode: &str) -> Self {
        Self::start_with_env(root, docker_mode, &[])
    }

    /// As `start`, with extra environment variables for the server (and so for the fake ssh).
    fn start_with_env(root: &Path, docker_mode: &str, env: &[(&str, &str)]) -> Self {
        let a = args(root);
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        let mut child = spawn(root, docker_mode, &a)
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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
            raw: Vec::new(),
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

    /// Send a request and wait for the response with its id.
    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        self.wait_for(id, Duration::from_secs(30))
    }

    /// Wait for the response with `id`; anything else that arrives meanwhile is kept by id.
    fn wait_for(&mut self, id: u64, limit: Duration) -> Value {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(v) = self.seen.remove(&id) {
                return v;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    let v: Value = serde_json::from_str(&line)
                        .unwrap_or_else(|_| panic!("stdout carried a non-protocol line: {line:?}"));
                    assert_eq!(v["jsonrpc"], "2.0", "{line}");
                    self.raw.push(line);
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
        let r = self.call("tools/call", json!({"name": name, "arguments": arguments}));
        assert!(r.get("error").is_none(), "{name}: {r}");
        assert_ne!(r["result"]["isError"], true, "{name}: {r}");
        r["result"]["structuredContent"].clone()
    }

    fn tool_names(&mut self) -> Vec<String> {
        let r = self.call("tools/list", json!({}));
        let mut names: Vec<String> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        names.sort();
        names
    }

    /// Close standard input and wait for the process to exit.
    fn close_and_wait(&mut self, limit: Duration) -> std::process::ExitStatus {
        self.stdin = None;
        self.wait_exit(limit)
    }

    fn wait_exit(&mut self, limit: Duration) -> std::process::ExitStatus {
        let start = Instant::now();
        loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                return s;
            }
            assert!(start.elapsed() < limit, "the server did not exit in time");
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

/// The server refuses a workspace path whose longest socket would reach 100 bytes. The root here is
/// `<checkout>/target/mcp/<tag>`, plus `/ws` and 39 bytes of socket path. A CI checkout path is up
/// to 37 bytes, so a tag over nine bytes would pass on a laptop and fail there.
fn fresh(tag: &str) -> PathBuf {
    assert!(
        tag.len() <= 9,
        "test tag too long for CI checkout paths: {tag}"
    );
    new_short_root(GROUP, tag)
}

fn value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn the_handshake_completes_and_tools_are_listed_with_closed_schemas() {
    let root = fresh("hs");
    let mut s = Server::start(&root, "ok");
    assert_eq!(
        s.tool_names(),
        [
            "compare_runs",
            "compile",
            "create_study",
            "document_status",
            "export",
            "get_provenance",
            "get_result",
            "get_run_result",
            "list_runs",
            "open_document",
            "pack_document",
            "plan_update",
            "run_status",
            "save_document",
            "start_document_job",
            "start_run",
            "unpack_document",
            "validate_study"
        ]
    );
    let r = s.call("tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().unwrap();
    for t in tools {
        let name = t["name"].as_str().unwrap();
        assert_eq!(
            t["inputSchema"]["additionalProperties"], false,
            "{name}: unknown arguments must be refused"
        );
        assert!(t["outputSchema"].is_object(), "{name} declares its result");
    }
    assert!(s.close_and_wait(Duration::from_secs(20)).success());
    for line in &s.raw {
        assert!(serde_json::from_str::<Value>(line).is_ok(), "{line}");
    }
}

#[test]
fn a_study_is_created_and_validated_by_name() {
    let root = fresh("cv");
    let mut s = Server::start(&root, "ok");
    let r = s.tool(
        "create_study",
        json!({"name": "cavity", "study": value(CAVITY)}),
    );
    assert_eq!(r["exit_class"], 0, "{r}");
    assert!(is_hex64(r["study_hash"].as_str().unwrap()), "{r}");
    assert!(root.join("ws/studies/cavity.json").is_file());
    let v = s.tool("validate_study", json!({"name": "cavity"}));
    assert_eq!(v["exit_class"], 0, "{v}");
    assert_eq!(v["study_hash"], r["study_hash"]);
    assert!(ssh_log(&root).is_empty(), "nothing reached the host");
}

#[test]
fn an_invalid_study_is_explained_with_a_code_and_not_stored() {
    let root = fresh("bad");
    let mut s = Server::start(&root, "ok");
    let mut study = value(CHANNEL);
    study["boundary_conditions"]
        .as_object_mut()
        .unwrap()
        .remove("outlet")
        .unwrap();
    let r = s.tool("create_study", json!({"name": "channel", "study": study}));
    assert_eq!(r["exit_class"], 2, "{r}");
    let d = r["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "CFD-BC-001")
        .unwrap_or_else(|| panic!("CFD-BC-001 is reported: {r}"));
    assert_eq!(d["pointer"], "/boundary_conditions", "{d}");
    assert!(!root.join("ws/studies/channel.json").exists());
}

#[test]
fn a_bad_setting_stops_the_process_before_serving_and_the_key_path_is_not_printed() {
    let root = fresh("set");
    let key = root.join("dummy-key").to_string_lossy().into_owned();
    let p = |n: &str| root.join(n).to_string_lossy().into_owned();
    let cases: [(&str, Vec<String>); 3] = [
        (
            "missing key",
            vec![
                "--workspace".into(),
                p("ws"),
                "--key".into(),
                p("no-such-key"),
                "--hosts".into(),
                p("hosts.toml"),
            ],
        ),
        (
            "missing hosts",
            vec![
                "--workspace".into(),
                p("ws"),
                "--key".into(),
                key.clone(),
                "--hosts".into(),
                p("no-such-hosts.toml"),
            ],
        ),
        (
            "unknown profile",
            vec![
                "--workspace".into(),
                p("ws"),
                "--key".into(),
                key.clone(),
                "--hosts".into(),
                p("hosts.toml"),
                "--profile".into(),
                "nothing".into(),
            ],
        ),
    ];
    for (what, a) in cases {
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        let o = spawn(&root, "ok", &a)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "{what}");
        assert!(o.stdout.is_empty(), "{what}: stdout stays empty");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains("error:"), "{what}: {err}");
        for private in [key.as_str(), SSH_TARGET, "no-such-key"] {
            assert!(!err.contains(private), "{what}: names {private}: {err}");
        }
    }
}

// ---- compile and export by name ----

fn golden(name: &str) -> PathBuf {
    workspace_root()
        .join("crates/cfdcraft-backend-esi/tests/golden")
        .join(name)
}

/// Every file under `dir`, by `/`-separated relative path.
fn files_under(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path.strip_prefix(root).unwrap().to_string_lossy();
                out.insert(name.replace('\\', "/"), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn store_cavity(s: &mut Server) {
    let r = s.tool(
        "create_study",
        json!({"name": "cavity", "study": value(CAVITY)}),
    );
    assert_eq!(r["exit_class"], 0, "{r}");
}

#[test]
fn compile_and_export_write_the_golden_trees() {
    let root = fresh("ce");
    let mut s = Server::start(&root, "ok");
    store_cavity(&mut s);

    let r = s.tool("compile", json!({"name": "cavity"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    let compiled = files_under(&golden("cavity-v2512"));
    assert_eq!(files_under(&root.join("ws/compiled/cavity")), compiled);

    let r = s.tool("export", json!({"name": "cavity"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    let mut exported = compiled;
    exported.extend(files_under(&golden("cavity-v2512-export")));
    assert_eq!(files_under(&root.join("ws/exports/cavity")), exported);
    let artifacts: Vec<&str> = r["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert!(
        artifacts.contains(&"Allrun") && artifacts.contains(&"README.md"),
        "{artifacts:?}"
    );
    let public = r.to_string();
    assert!(
        !public.contains(&root.to_string_lossy().into_owned()),
        "{public}"
    );
}

// ---- documents and packs by name ----

fn codes_of(report: &Value) -> Vec<&str> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect()
}

#[test]
fn a_study_is_saved_as_a_document_and_opened_by_name() {
    let root = fresh("doc");
    let mut s = Server::start(&root, "ok");
    store_cavity(&mut s);

    let saved = s.tool("save_document", json!({"name": "cavity"}));
    assert_eq!(saved["exit_class"], 0, "{saved}");
    assert_eq!(saved["operation"], "save_document");
    assert!(root.join("ws/documents/cavity.cfdcraft").is_file());
    let opened = s.tool("open_document", json!({"name": "cavity"}));
    assert_eq!(opened["exit_class"], 0, "{opened}");
    assert_eq!(opened["study_hash"], CAVITY_HASH, "{opened}");

    // A second save is refused, shows the document relative to the workspace and changes nothing.
    let before = files_under(&root.join("ws"));
    let again = s.tool("save_document", json!({"name": "cavity"}));
    assert_eq!(again["exit_class"], 2, "{again}");
    assert_eq!(codes_of(&again), ["CFD-DOC-008"], "{again}");
    assert!(
        again.to_string().contains("documents/cavity.cfdcraft"),
        "{again}"
    );
    assert_eq!(files_under(&root.join("ws")), before);

    // A path is not an input.
    let refused = s.raw_tool("open_document", json!({"name": "cavity", "path": "/x"}));
    assert!(
        refused.get("error").is_some() || refused["result"]["isError"] == true,
        "{refused}"
    );
    assert!(s.close_and_wait(Duration::from_secs(20)).success());
    assert_nothing_private(&root, &s);
}

#[test]
fn a_document_is_packed_and_a_pack_the_operator_placed_is_unpacked() {
    let root = fresh("pack");
    let mut s = Server::start(&root, "ok");
    store_cavity(&mut s);
    s.tool("save_document", json!({"name": "cavity"}));

    let packed = s.tool("pack_document", json!({"name": "cavity"}));
    assert_eq!(packed["exit_class"], 0, "{packed}");
    assert!(root.join("ws/packs/cavity.cfdpack").is_file());
    let again = s.tool("pack_document", json!({"name": "cavity"}));
    assert_eq!(codes_of(&again), ["CFD-DOC-008"], "{again}");

    // Only a pack already in the folder can be unpacked; the operator puts it there.
    let none = s.tool("unpack_document", json!({"name": "copy"}));
    assert_eq!(codes_of(&none), ["CFD-DOC-001"], "{none}");
    std::fs::copy(
        root.join("ws/packs/cavity.cfdpack"),
        root.join("ws/packs/copy.cfdpack"),
    )
    .unwrap();
    let unpacked = s.tool("unpack_document", json!({"name": "copy"}));
    assert_eq!(unpacked["exit_class"], 0, "{unpacked}");
    assert!(root.join("ws/documents/copy.cfdcraft").is_file());
    let opened = s.tool("open_document", json!({"name": "copy"}));
    assert_eq!(opened["study_hash"], CAVITY_HASH, "{opened}");
    let twice = s.tool("unpack_document", json!({"name": "copy"}));
    assert_eq!(codes_of(&twice), ["CFD-DOC-008"], "{twice}");
    assert!(s.close_and_wait(Duration::from_secs(20)).success());
    assert_nothing_private(&root, &s);
}

// ---- runs: start returns at once, polling, busy ----

fn run_count(root: &Path) -> usize {
    events(root).iter().filter(|e| *e == "run").count()
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

fn release(root: &Path) {
    std::fs::write(root.join("release"), "").unwrap();
}

/// Store and export the cavity, then start a validate run of the export; returns the run id.
fn start_held_run(s: &mut Server, root: &Path) -> String {
    store_cavity(s);
    let r = s.tool("export", json!({"name": "cavity"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    let began = Instant::now();
    let r = s.tool(
        "start_run",
        json!({"name": "cavity", "mode": "validate", "from_export": true}),
    );
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "start returns at once"
    );
    assert_eq!(r["status"], "started", "{r}");
    let id = r["run_id"].as_str().unwrap().to_owned();
    wait_until("the container to start", || run_count(root) == 1);
    id
}

#[test]
fn a_run_is_started_at_once_polled_and_collected() {
    let root = fresh("poll");
    let mut s = Server::start(&root, "hold");
    let id = start_held_run(&mut s, &root);

    let st = s.tool("run_status", json!({"run_id": id}));
    assert_eq!(st["status"], "running", "{st}");
    assert_eq!(st["mode"], "validate");
    let g = s.tool("get_result", json!({"run_id": id}));
    assert_eq!(g["status"], "running", "{g}");

    release(&root);
    wait_until("the run to finish", || {
        s.tool("run_status", json!({"run_id": id}))["status"] == "finished"
    });
    let g = s.tool("get_result", json!({"run_id": id}));
    assert_eq!(g["status"], "finished", "{g}");
    // The results archive is empty, so the evidence fails: a failed gate, not a pass.
    assert_eq!(g["report"]["exit_class"], 1, "{g}");
    assert_eq!(g["report"]["evidence"]["verdict"], "failed", "{g}");
    let st = s.tool("run_status", json!({"run_id": id}));
    assert_eq!(st["exit_class"], 1, "{st}");
    let public = g.to_string();
    for private in [
        root.join("dummy-key").to_string_lossy().into_owned(),
        SSH_TARGET.to_owned(),
    ] {
        assert!(
            !public.contains(&private),
            "the result names {private}: {public}"
        );
    }
}

#[test]
fn a_second_start_is_busy_with_its_holder_and_is_never_queued() {
    let root = fresh("busy");
    let mut s = Server::start(&root, "hold");
    let id = start_held_run(&mut s, &root);

    let r = s.tool("start_run", json!({"name": "cavity", "mode": "run"}));
    assert_eq!(r["status"], "busy", "{r}");
    assert_eq!(r["holder"]["run_id"], id, "{r}");
    assert!(r["retry_after_seconds"].as_u64().is_some(), "{r}");
    assert!(r.get("run_id").is_none(), "nothing was queued: {r}");

    release(&root);
    wait_until("the run to finish", || {
        s.tool("run_status", json!({"run_id": id}))["status"] == "finished"
    });
    assert_eq!(
        run_count(&root),
        1,
        "exactly one container ran: {:?}",
        events(&root)
    );
}

#[test]
fn a_poll_that_arrives_while_ssh_has_the_inherited_pipes_non_blocking_is_still_answered() {
    let root = fresh("nonblock");
    let mut s = Server::start_with_env(&root, "hold", &[("CFC_TEST_SSH_NONBLOCK", "3")]);
    let id = start_held_run(&mut s, &root);
    release(&root);
    // The results download now keeps ssh running for three seconds. Polling through that window
    // must not end the server: every poll is answered and the report is there at the end.
    wait_until("the run to finish", || {
        s.tool("run_status", json!({"run_id": id}))["status"] == "finished"
    });
    let g = s.tool("get_result", json!({"run_id": id}));
    assert_eq!(g["status"], "finished", "{g}");
    assert!(s.close_and_wait(Duration::from_secs(20)).success());
}

#[test]
fn what_a_container_prints_never_reaches_the_protocol_stream() {
    let root = fresh("noise");
    let mut s =
        Server::start_with_env(&root, "hold", &[("CFC_TEST_DOCKER_STDOUT", "not protocol")]);
    let id = start_held_run(&mut s, &root);
    // Every line the server writes must be a protocol message: reading one that is not panics.
    assert_eq!(
        s.tool("run_status", json!({"run_id": id}))["status"],
        "running"
    );
    release(&root);
    wait_until("the run to finish", || {
        s.tool("run_status", json!({"run_id": id}))["status"] == "finished"
    });
    assert!(s.close_and_wait(Duration::from_secs(20)).success());
    assert!(
        s.raw
            .iter()
            .all(|l| serde_json::from_str::<Value>(l).is_ok()),
        "{:?}",
        s.raw
    );
}

#[test]
fn a_lock_held_by_another_process_is_busy_with_that_holder() {
    let root = fresh("other");
    let mut s = Server::start(&root, "ok");
    store_cavity(&mut s);
    hold_lock(&root, now_epoch(), 3600);
    let r = s.tool("start_run", json!({"name": "cavity", "mode": "run"}));
    assert_eq!(r["status"], "busy", "{r}");
    assert_eq!(r["holder"]["run_id"], "other-run-1", "{r}");
    assert!(
        lock_dir(&root).is_dir(),
        "somebody else's lock is never removed"
    );
    assert_eq!(run_count(&root), 0);
}

#[test]
fn refused_starts_answer_with_the_report_and_touch_nothing() {
    let root = fresh("refused");
    let mut s = Server::start(&root, "ok");
    store_cavity(&mut s);
    for (arguments, code) in [
        (json!({"name": "Not/A Name", "mode": "run"}), "CFD-WS-001"),
        (json!({"name": "nothing-here", "mode": "run"}), "CFD-WS-002"),
        (json!({"name": "cavity", "mode": "smoke"}), "CFD-RUN-001"),
    ] {
        let r = s.tool("start_run", arguments.clone());
        assert_eq!(r["status"], "refused", "{arguments}: {r}");
        assert_eq!(r["report"]["exit_class"], 2, "{r}");
        assert_eq!(r["report"]["diagnostics"][0]["code"], code, "{r}");
        assert!(r.get("run_id").is_none(), "{r}");
    }
    assert!(ssh_log(&root).is_empty(), "nothing reached the host");
    assert_eq!(run_count(&root), 0);
}

#[test]
fn unknown_or_malformed_run_ids_are_not_found() {
    let root = fresh("unknown");
    let mut s = Server::start(&root, "ok");
    for id in ["cfdcraft-nothing-1", "../etc/passwd", "a b", ""] {
        let st = s.tool("run_status", json!({"run_id": id}));
        assert_eq!(st["status"], "not_found", "{id:?}: {st}");
        let g = s.tool("get_result", json!({"run_id": id}));
        assert_eq!(g["status"], "not_found", "{id:?}: {g}");
    }
}

// ---- shutdown ----

/// With a job blocked in the fake `docker run`, the server stops the owned container, removes it
/// and gives up the lock before it exits.
fn assert_cleaned_up(root: &Path) {
    let e = events(root);
    assert!(
        e.iter().any(|c| c == "stop") && e.iter().any(|c| c == "rm"),
        "{e:?}"
    );
    assert!(!lock_dir(root).exists(), "the lock is released");
}

#[test]
fn closing_stdin_cancels_the_job_and_cleans_up() {
    let root = fresh("eof");
    let mut s = Server::start(&root, "hold");
    start_held_run(&mut s, &root);
    let status = s.close_and_wait(Duration::from_secs(60));
    assert!(status.success(), "{status:?}");
    assert_cleaned_up(&root);
}

#[test]
fn sigterm_cancels_the_job_and_cleans_up() {
    let root = fresh("term");
    let mut s = Server::start(&root, "hold");
    start_held_run(&mut s, &root);
    let pid = s.child.id().to_string();
    assert!(
        Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .unwrap()
            .success()
    );
    let status = s.wait_exit(Duration::from_secs(60));
    assert!(status.success(), "{status:?}");
    assert_cleaned_up(&root);
}

// ---- how the server's input ends ----

/// Start the server with standard input on one end of a socket pair that already holds the whole
/// handshake, so the server completes it and then reads again. `nonblocking` makes that second
/// read fail with an I/O error (nothing to read yet) instead of waiting. Returns the process and
/// our end of the socket, which stays open so that the server never sees an end of file unless the
/// caller drops it.
fn start_on_socket(root: &Path, nonblocking: bool) -> (Child, std::os::unix::net::UnixStream) {
    use std::os::unix::net::UnixStream;
    let (mut ours, theirs) = UnixStream::pair().unwrap();
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "0"}
        }
    });
    writeln!(ours, "{init}").unwrap();
    writeln!(
        ours,
        "{}",
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .unwrap();
    theirs.set_nonblocking(nonblocking).unwrap();
    let a = args(root);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let child = spawn(root, "ok", &a)
        .stdin(Stdio::from(std::os::fd::OwnedFd::from(theirs)))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    (child, ours)
}

/// Wait for the process to exit and return its status and everything it wrote to standard error.
fn finish(mut child: Child, limit: Duration) -> (std::process::ExitStatus, String) {
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the server did not exit in time");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    (status, err)
}

#[test]
fn a_read_failure_on_standard_input_is_said_on_standard_error_and_is_not_success() {
    let root = fresh("readfail");
    let (child, _ours) = start_on_socket(&root, true);
    let (status, err) = finish(child, Duration::from_secs(60));
    assert_eq!(status.code(), Some(5), "{status:?} {err:?}");
    assert_eq!(err.lines().count(), 1, "one plain line: {err:?}");
    assert!(err.starts_with("error: "), "{err:?}");
    assert!(
        !err.contains(root.to_string_lossy().as_ref()),
        "no path in the note: {err:?}"
    );
    assert!(!lock_dir(&root).exists(), "the lock is released");
}

#[test]
fn a_clean_end_of_input_exits_zero_and_says_nothing() {
    let root = fresh("eofquiet");
    let (child, ours) = start_on_socket(&root, false);
    drop(ours);
    let (status, err) = finish(child, Duration::from_secs(60));
    assert!(status.success(), "{status:?} {err:?}");
    assert_eq!(err, "", "a normal close is not a failure");
}

// ---- the advertised surface is closed ----

/// Words that must not name an input property outside the study itself.
const FORBIDDEN_WORDS: [&str; 14] = [
    "path",
    "file",
    "dir",
    "directory",
    "key",
    "host",
    "ssh",
    "command",
    "shell",
    "script",
    "dictionary",
    "dict",
    "url",
    "uri",
];

fn forbidden_name(name: &str) -> bool {
    name.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| FORBIDDEN_WORDS.contains(&word))
}

/// Every `$ref` target (`#/$defs/<name>`) reachable from `v`, following the definitions in `defs`.
fn reachable_defs(v: &Value, defs: &Value, found: &mut std::collections::BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, c) in m {
                match (k.as_str(), c.as_str()) {
                    ("$ref", Some(target)) => {
                        if let Some(name) = target.strip_prefix("#/$defs/")
                            && found.insert(name.to_owned())
                            && let Some(d) = defs.get(name)
                        {
                            reachable_defs(d, defs, found);
                        }
                    }
                    _ => reachable_defs(c, defs, found),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|c| reachable_defs(c, defs, found)),
        _ => {}
    }
}

/// Property names in `v` that look like a path, host, key or command.
fn named_like_a_handle(v: &Value, at: &str, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, c) in m {
                if k == "properties"
                    && let Some(props) = c.as_object()
                {
                    for (name, schema) in props {
                        let here = format!("{at}/{name}");
                        if forbidden_name(name) {
                            out.push(format!("{here} is named like a path, host, key or command"));
                        }
                        named_like_a_handle(schema, &here, out);
                    }
                } else {
                    named_like_a_handle(c, &format!("{at}/{k}"), out);
                }
            }
        }
        Value::Array(a) => {
            for (i, c) in a.iter().enumerate() {
                named_like_a_handle(c, &format!("{at}/{i}"), out);
            }
        }
        _ => {}
    }
}

/// What is wrong with a `tools/list` result: a tool that accepts unknown arguments, or an input
/// property outside the study that is named like a path, host, key or command. The `study`
/// property and the definitions only it reaches are the published study schema and are skipped.
fn surface_problems(tools: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for t in tools.as_array().expect("a list of tools") {
        let name = t["name"].as_str().unwrap_or("?");
        let mut schema = t["inputSchema"].clone();
        if schema["additionalProperties"] != false {
            out.push(format!("{name}: does not refuse unknown arguments"));
        }
        if let Some(study) = schema["properties"].get("study").cloned() {
            let mut skip = std::collections::BTreeSet::new();
            reachable_defs(&study, &schema["$defs"], &mut skip);
            schema["properties"]
                .as_object_mut()
                .unwrap()
                .remove("study");
            for d in skip {
                schema["$defs"].as_object_mut().unwrap().remove(&d);
            }
        }
        named_like_a_handle(&schema, name, &mut out);
    }
    out
}

#[test]
fn the_surface_is_still_closed_with_eighteen_tools() {
    let root = fresh("closed");
    let mut s = Server::start(&root, "ok");
    let r = s.call("tools/list", json!({}));
    let tools = &r["result"]["tools"];
    assert_eq!(tools.as_array().unwrap().len(), 18);
    for name in [
        "save_document",
        "open_document",
        "pack_document",
        "unpack_document",
        "document_status",
        "list_runs",
    ] {
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is advertised"));
        let inputs: Vec<&str> = tool["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(inputs, ["name"], "{name} takes a name and nothing else");
    }
    for (name, expected) in [
        ("get_run_result", &["name", "run"][..]),
        ("get_provenance", &["name", "run"][..]),
        (
            "compare_runs",
            &["a", "abs_tolerance", "b", "name", "rel_tolerance"][..],
        ),
        ("plan_update", &["items", "names", "order"][..]),
        ("start_document_job", &["job", "name", "run"][..]),
    ] {
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is advertised"));
        let mut inputs: Vec<&str> = tool["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        inputs.sort_unstable();
        assert_eq!(
            inputs, expected,
            "{name} takes names, runs, tolerances or items only"
        );
    }
    for tool in tools.as_array().unwrap() {
        assert_eq!(
            tool["inputSchema"]["additionalProperties"], false,
            "{}: unknown arguments must be refused",
            tool["name"]
        );
    }
    assert_eq!(surface_problems(tools), Vec::<String>::new());
    let create = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "create_study")
        .unwrap();
    let schema = create["inputSchema"].to_string();
    assert!(
        schema.contains("boundary_conditions"),
        "the study schema is published"
    );
}

#[test]
fn the_surface_check_reports_what_it_is_meant_to_catch() {
    let tools = json!([
        {"name": "ok", "inputSchema": {"type": "object", "additionalProperties": false,
            "properties": {"name": {"type": "string"}}}},
        {"name": "leaky", "inputSchema": {"type": "object", "additionalProperties": false,
            "properties": {"path": {"type": "string"}}}},
        {"name": "open", "inputSchema": {"type": "object",
            "properties": {"name": {"type": "string"}}}},
        {"name": "nested", "inputSchema": {"type": "object", "additionalProperties": false,
            "properties": {"options": {"$ref": "#/$defs/Options"}},
            "$defs": {"Options": {"type": "object",
                "properties": {"ssh_target": {"type": "string"}}}}}},
        {"name": "study", "inputSchema": {"type": "object", "additionalProperties": false,
            "properties": {"study": {"$ref": "#/$defs/Study"}},
            "$defs": {"Study": {"type": "object",
                "properties": {"file_name": {"type": "string"}}}}}}
    ]);
    let problems = surface_problems(&tools).join("\n");
    assert!(
        problems.contains("leaky/properties") || problems.contains("leaky/path"),
        "{problems}"
    );
    assert!(problems.contains("open: does not refuse"), "{problems}");
    assert!(problems.contains("ssh_target"), "{problems}");
    assert!(!problems.contains("ok"), "{problems}");
    assert!(
        !problems.contains("file_name"),
        "the study's own properties are not checked: {problems}"
    );
}

#[test]
fn unknown_fields_are_refused_by_every_tool_and_nothing_happens() {
    let root = fresh("uf");
    let mut s = Server::start(&root, "ok");
    store_cavity(&mut s);
    let before = files_under(&root.join("ws"));
    let valid: [(&str, Value); 12] = [
        (
            "create_study",
            json!({"name": "other", "study": value(CAVITY)}),
        ),
        ("validate_study", json!({"name": "cavity"})),
        ("compile", json!({"name": "cavity"})),
        ("export", json!({"name": "cavity"})),
        ("save_document", json!({"name": "cavity"})),
        ("open_document", json!({"name": "cavity"})),
        ("pack_document", json!({"name": "cavity"})),
        ("unpack_document", json!({"name": "cavity"})),
        ("start_run", json!({"name": "cavity", "mode": "run"})),
        (
            "start_document_job",
            json!({"name": "cavity", "job": "solve"}),
        ),
        ("run_status", json!({"run_id": "cfdcraft-nothing-1"})),
        ("get_result", json!({"run_id": "cfdcraft-nothing-1"})),
    ];
    for (tool, arguments) in valid {
        for extra in ["dictionary", "path", "key", "host", "command"] {
            let mut a = arguments.clone();
            a[extra] = json!("x");
            let r = s.raw_tool(tool, a);
            let refused = r.get("error").is_some() || r["result"]["isError"] == true;
            assert!(refused, "{tool} accepted `{extra}`: {r}");
            assert!(
                r.to_string().contains(extra),
                "{tool}: the answer names `{extra}`: {r}"
            );
        }
    }
    assert_eq!(
        files_under(&root.join("ws")),
        before,
        "nothing was created or changed"
    );
    assert!(!root.join("ws/studies/other.json").exists());
    assert_eq!(run_count(&root), 0, "{:?}", events(&root));
    assert!(ssh_log(&root).is_empty(), "nothing reached the host");
}

// ---- nothing private reaches an agent ----

/// The values this test configured and that must never be printed: the key path, the ssh target,
/// the work directory of the host profile, and the directory holding the whole fake box.
fn configured_private_values(root: &Path) -> Vec<String> {
    let hosts = std::fs::read_to_string(root.join("hosts.toml")).unwrap();
    let work_dir = hosts
        .lines()
        .find(|l| l.trim_start().starts_with("work_dir"))
        .and_then(|l| l.split('"').nth(1))
        .expect("the hosts file names a work directory")
        .to_owned();
    vec![
        root.join("dummy-key").to_string_lossy().into_owned(),
        SSH_TARGET.to_owned(),
        work_dir,
        root.to_string_lossy().into_owned(),
    ]
}

fn assert_nothing_private(root: &Path, s: &Server) {
    // the handshake alone is one line; a session that answered a call has at least two
    assert!(s.raw.len() >= 2, "the session produced output");
    let out = s.raw.join("\n");
    for private in configured_private_values(root) {
        assert!(!private.is_empty());
        assert!(!out.contains(&private), "the session printed {private}");
    }
}

#[test]
fn a_full_session_with_a_busy_answer_prints_nothing_private() {
    let root = fresh("lkb");
    let mut s = Server::start(&root, "hold");
    let id = start_held_run(&mut s, &root);
    s.tool("validate_study", json!({"name": "cavity"}));
    s.tool("compile", json!({"name": "cavity"}));
    s.tool("run_status", json!({"run_id": id}));
    let busy = s.tool("start_run", json!({"name": "cavity", "mode": "run"}));
    assert_eq!(busy["status"], "busy", "{busy}");
    s.raw_tool("compile", json!({"name": "cavity", "path": "/x"}));
    release(&root);
    wait_until("the run to finish", || {
        s.tool("run_status", json!({"run_id": id}))["status"] == "finished"
    });
    s.tool("get_result", json!({"run_id": id}));
    assert_nothing_private(&root, &s);
}

#[test]
fn a_full_session_with_an_ssh_failure_prints_nothing_private() {
    let root = fresh("lks");
    let mut s = Server::start_with_env(&root, "ok", &[("CFC_TEST_FAIL_255", "uptime")]);
    store_cavity(&mut s);
    s.tool("validate_study", json!({"name": "cavity"}));
    s.tool("compile", json!({"name": "cavity"}));
    s.tool("export", json!({"name": "cavity"}));
    let r = s.tool(
        "start_run",
        json!({"name": "cavity", "mode": "run", "from_export": true}),
    );
    assert_eq!(r["status"], "refused", "{r}");
    assert_eq!(r["report"]["exit_class"], 4, "{r}");
    assert!(!ssh_log(&root).is_empty(), "the failing ssh was reached");
    assert_nothing_private(&root, &s);
}

// ---- reading what a document records ----

/// A runner that writes a folder like the one a good run leaves instead of running a case. The
/// samples of each call are moved by the next shift in the list, the last one repeating.
struct FixtureRunner {
    dir: PathBuf,
    calls: usize,
    shifts: Vec<f64>,
}

impl CaseRunner for FixtureRunner {
    fn run_case(&mut self, _study_json: &str) -> Result<CaseRun, Box<Report>> {
        let shift = self.shifts[self.calls.min(self.shifts.len() - 1)];
        self.calls += 1;
        let dir = self.dir.join(format!("call-{}", self.calls));
        let spec = FixtureSpec {
            end_time: 5000,
            cells: 16384,
            samples: 64,
            shift,
        };
        write_results(&dir, &spec).unwrap();
        let checks = json!([{"name": "evidence", "passed": true, "detail": "passed"}]);
        Ok(CaseRun {
            results_dir: dir,
            checks: serde_json::from_value(checks).unwrap(),
            host: Some("box".to_owned()),
        })
    }
}

/// The cavity with another speed for the lid.
fn lid_study(speed: f64) -> String {
    let mut study = value(CAVITY);
    study["boundary_conditions"]["lid"]["velocity"]["value"] = json!(speed);
    study.to_string()
}

/// The cavity with another number of cells in x.
fn finer_study() -> String {
    let mut study = value(CAVITY);
    study["mesh"]["cells_x"] = json!(256);
    study.to_string()
}

fn document_file(root: &Path, name: &str) -> PathBuf {
    root.join("ws/documents").join(format!("{name}.cfdcraft"))
}

/// Save the document `name` in the workspace folder the server will use, replacing what was there.
fn put_document(root: &Path, name: &str, study: &str, replace: bool) -> PathBuf {
    let path = document_file(root, name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let saved = save_document(study, &path, replace);
    assert_eq!(saved.report.exit_class, 0, "{}", saved.report.render_text());
    path
}

/// The document `name`, solved once for each pair of lid speed and sample shift in turn, before the
/// server starts. Each solve after the first follows a change of the lid speed.
fn solve_named(root: &Path, name: &str, solves: &[(f64, f64)]) -> PathBuf {
    let mut runner = FixtureRunner {
        dir: root.join("fixture").join(name),
        calls: 0,
        shifts: solves.iter().map(|(_, shift)| *shift).collect(),
    };
    let mut path = PathBuf::new();
    for (at, (speed, _)) in solves.iter().enumerate() {
        path = put_document(root, name, &lid_study(*speed), at > 0);
        let solved = solve_document(&path, &mut runner);
        assert_eq!(
            solved.report.exit_class,
            0,
            "{}",
            solved.report.render_text()
        );
    }
    path
}

fn solve_cavity(root: &Path, solves: &[(f64, f64)]) -> PathBuf {
    solve_named(root, "cavity", solves)
}

/// Two documents solved once and then given a finer mesh, so both stages of both are out of date.
fn stale_pair(root: &Path) {
    for name in ["a", "b"] {
        solve_named(root, name, &[(0.1, 0.0)]);
        put_document(root, name, &finer_study(), true);
    }
}

fn refused(r: &Value) -> bool {
    r.get("error").is_some() || r["result"]["isError"] == true
}

#[test]
fn document_status_over_the_protocol_names_the_stale_result() {
    let root = fresh("st");
    let path = solve_cavity(&root, &[(0.1, 0.0)]);
    let saved = save_document(&lid_study(0.2), &path, true);
    assert_eq!(saved.report.exit_class, 0);
    let mut s = Server::start(&root, "ok");

    let r = s.tool("document_status", json!({"name": "cavity"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    assert_eq!(r["operation"], "document_status");
    assert_eq!(r["repro"]["kind"], "status", "{r}");
    assert_eq!(r["repro"]["mesh"]["state"], "fresh", "{r}");
    assert_eq!(r["repro"]["result"]["state"], "stale", "{r}");
    let stale: Vec<&Value> = r["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "CFD-REPRO-001")
        .collect();
    assert_eq!(stale.len(), 1, "{r}");
    assert_eq!(
        stale[0]["message"],
        "result stale because lid velocity 0.1 → 0.2 m/s"
    );
    assert!(
        !r.to_string().contains(&root.to_string_lossy().into_owned()),
        "{r}"
    );
    assert!(s.close_and_wait(Duration::from_secs(20)).success());
    assert_nothing_private(&root, &s);
}

#[test]
fn an_unknown_document_is_a_report_not_a_protocol_error() {
    let root = fresh("unk");
    let mut s = Server::start(&root, "ok");
    for tool in ["document_status", "list_runs"] {
        let r = s.tool(tool, json!({"name": "nothing"}));
        assert_eq!(r["exit_class"], 2, "{tool}: {r}");
        assert_eq!(codes_of(&r), ["CFD-DOC-001"], "{tool}: {r}");
        let r = s.tool(tool, json!({"name": "../x"}));
        assert_eq!(r["exit_class"], 2, "{tool}: {r}");
        assert_eq!(codes_of(&r), ["CFD-WS-001"], "{tool}: {r}");
    }
    assert!(s.close_and_wait(Duration::from_secs(20)).success());
    assert_nothing_private(&root, &s);
}

#[test]
fn list_runs_returns_the_recorded_runs() {
    let root = fresh("lr");
    solve_cavity(&root, &[(0.1, 0.0)]);
    let mut s = Server::start(&root, "ok");
    let r = s.tool("list_runs", json!({"name": "cavity"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    assert_eq!(r["repro"]["kind"], "runs", "{r}");
    let runs = r["repro"]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1, "{r}");
    assert_eq!(runs[0]["id"], "run-1");
    assert_eq!(runs[0]["recorded"], json!(["mesh", "result"]));
    assert_eq!(runs[0]["checks"][0]["name"], "evidence");
}

#[test]
fn get_run_result_returns_the_checks_and_key_numbers() {
    let root = fresh("rr");
    solve_cavity(&root, &[(0.1, 0.0)]);
    let mut s = Server::start(&root, "ok");

    let r = s.tool("get_run_result", json!({"name": "cavity"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    assert_eq!(r["repro"]["kind"], "result", "{r}");
    assert_eq!(r["repro"]["run"], "run-1");
    assert_eq!(r["repro"]["checks"][0]["name"], "evidence");
    assert!(r["repro"]["key_numbers"].is_object(), "{r}");
    assert!(
        r["repro"]["key_numbers"]["solver.iterations"].is_number(),
        "{r}"
    );
    let named = s.tool("get_run_result", json!({"name": "cavity", "run": "run-1"}));
    assert_eq!(named["repro"], r["repro"]);

    let missing = s.tool("get_run_result", json!({"name": "cavity", "run": "run-9"}));
    assert_eq!(missing["exit_class"], 2, "{missing}");
    assert_eq!(codes_of(&missing), ["CFD-REPRO-004"], "{missing}");
}

#[test]
fn get_provenance_returns_the_provenance() {
    let root = fresh("pv");
    solve_cavity(&root, &[(0.1, 0.0)]);
    let mut s = Server::start(&root, "ok");
    let r = s.tool("get_provenance", json!({"name": "cavity"}));
    assert_eq!(r["exit_class"], 0, "{r}");
    assert_eq!(r["repro"]["kind"], "provenance", "{r}");
    let provenance = &r["repro"]["provenance"];
    assert!(
        provenance["tool"]["image_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:")),
        "{r}"
    );
    assert_eq!(provenance["host"], "box", "{r}");
    for key in ["mesh_key", "result_key"] {
        assert!(
            is_hex64(provenance[key].as_str().unwrap_or("")),
            "{key}: {r}"
        );
    }
    let named = s.tool("get_provenance", json!({"name": "cavity", "run": "run-1"}));
    assert_eq!(named["repro"], r["repro"]);
}

#[test]
fn compare_runs_returns_a_comparison_and_class_1_for_a_difference() {
    let root = fresh("cr");
    solve_cavity(&root, &[(0.1, 0.0), (0.2, 0.1)]);
    let mut s = Server::start(&root, "ok");

    let r = s.tool(
        "compare_runs",
        json!({"name": "cavity", "a": "run-1", "b": "run-2"}),
    );
    assert_eq!(r["exit_class"], 1, "{r}");
    assert_eq!(r["repro"]["kind"], "comparison", "{r}");
    assert_eq!(r["repro"]["comparison"]["verdict"], "differs", "{r}");

    let loose = s.tool(
        "compare_runs",
        json!({
            "name": "cavity", "a": "run-1", "b": "run-2",
            "abs_tolerance": 1, "rel_tolerance": 1
        }),
    );
    assert_eq!(loose["exit_class"], 0, "{loose}");
    assert_eq!(loose["repro"]["comparison"]["verdict"], "within_tolerance");

    let negative = s.tool(
        "compare_runs",
        json!({"name": "cavity", "a": "run-1", "b": "run-2", "abs_tolerance": -1}),
    );
    assert_eq!(negative["exit_class"], 2, "{negative}");
    assert!(negative["repro"].is_null(), "{negative}");
}

#[test]
fn plan_update_returns_the_plan_and_refuses_what_an_update_would() {
    let root = fresh("pu");
    stale_pair(&root);
    let mut s = Server::start(&root, "ok");
    let before = files_under(&root.join("ws"));

    let r = s.tool("plan_update", json!({"names": ["a", "b"]}));
    assert_eq!(r["exit_class"], 0, "{r}");
    assert_eq!(r["repro"]["kind"], "plan", "{r}");
    let all = json!(["a:mesh", "a:result", "b:mesh", "b:result"]);
    assert_eq!(r["repro"]["pending"], all, "{r}");
    assert_eq!(r["repro"]["order"], all, "{r}");

    let reordered = s.tool(
        "plan_update",
        json!({
            "names": ["a", "b"],
            "order": ["b:mesh", "b:result", "a:mesh", "a:result"]
        }),
    );
    assert_eq!(reordered["exit_class"], 0, "{reordered}");
    assert_eq!(
        reordered["repro"]["order"],
        json!(["b:mesh", "b:result", "a:mesh", "a:result"])
    );

    let refused_item = s.tool(
        "plan_update",
        json!({"names": ["a"], "items": ["a:result"]}),
    );
    assert_eq!(refused_item["exit_class"], 2, "{refused_item}");
    assert_eq!(codes_of(&refused_item), ["CFD-REPRO-008"], "{refused_item}");

    let many: Vec<String> = (0..17).map(|n| format!("d{n}")).collect();
    for names in [json!(many), json!(["a", "../x"])] {
        let bad = s.tool("plan_update", json!({"names": names}));
        assert_eq!(bad["exit_class"], 2, "{bad}");
        assert_eq!(codes_of(&bad), ["CFD-WS-001"], "{bad}");
    }
    assert_eq!(files_under(&root.join("ws")), before, "nothing was written");
}

#[test]
fn these_tools_never_write_and_never_start_a_run() {
    let root = fresh("nw");
    solve_cavity(&root, &[(0.1, 0.0), (0.2, 0.1)]);
    put_document(&root, "cavity", &lid_study(0.3), true);
    let mut s = Server::start(&root, "ok");
    let files = files_under(&root.join("ws"));
    let log = events(&root);
    let ssh = ssh_log(&root);

    let calls: [(&str, Value); 8] = [
        ("document_status", json!({"name": "cavity"})),
        ("list_runs", json!({"name": "cavity"})),
        ("get_run_result", json!({"name": "cavity"})),
        ("get_provenance", json!({"name": "cavity", "run": "run-2"})),
        (
            "compare_runs",
            json!({"name": "cavity", "a": "run-1", "b": "run-2"}),
        ),
        ("plan_update", json!({"names": ["cavity"]})),
        ("document_status", json!({"name": "absent"})),
        ("get_run_result", json!({"name": "cavity", "run": "run-9"})),
    ];
    for (tool, arguments) in calls {
        s.tool(tool, arguments);
    }
    assert_eq!(files_under(&root.join("ws")), files);
    assert_eq!(events(&root), log, "the fake host saw nothing");
    assert_eq!(ssh_log(&root), ssh, "nothing reached the host");
    assert_eq!(run_count(&root), 0);
}

// ---- the new tools stay inside the closed surface ----

#[test]
fn an_argument_that_names_a_path_is_refused_by_every_new_tool() {
    let root = fresh("np");
    solve_cavity(&root, &[(0.1, 0.0), (0.2, 0.1)]);
    let mut s = Server::start(&root, "ok");
    let files = files_under(&root.join("ws"));
    let valid: [(&str, Value); 6] = [
        ("document_status", json!({"name": "cavity"})),
        ("list_runs", json!({"name": "cavity"})),
        ("get_run_result", json!({"name": "cavity"})),
        ("get_provenance", json!({"name": "cavity"})),
        (
            "compare_runs",
            json!({"name": "cavity", "a": "run-1", "b": "run-2"}),
        ),
        ("plan_update", json!({"names": ["cavity"]})),
    ];
    for (tool, arguments) in valid {
        for extra in ["path", "dictionary", "key", "host", "command"] {
            let mut a = arguments.clone();
            a[extra] = json!("x");
            let r = s.raw_tool(tool, a);
            assert!(refused(&r), "{tool} accepted `{extra}`: {r}");
            assert!(r.to_string().contains(extra), "{tool}: {r}");
        }
    }
    assert_eq!(files_under(&root.join("ws")), files);
}

#[test]
fn a_full_session_with_the_new_tools_prints_nothing_private() {
    let root = fresh("nps");
    solve_cavity(&root, &[(0.1, 0.0), (0.2, 0.1)]);
    put_document(&root, "cavity", &lid_study(0.3), true);
    stale_pair(&root);
    let mut s = Server::start(&root, "ok");
    for (tool, arguments) in [
        ("document_status", json!({"name": "cavity"})),
        ("document_status", json!({"name": "absent"})),
        ("document_status", json!({"name": "../x"})),
        ("list_runs", json!({"name": "cavity"})),
        ("get_run_result", json!({"name": "cavity"})),
        ("get_run_result", json!({"name": "cavity", "run": "run-9"})),
        ("get_provenance", json!({"name": "cavity", "run": "run-2"})),
        (
            "compare_runs",
            json!({"name": "cavity", "a": "run-1", "b": "run-2"}),
        ),
        (
            "compare_runs",
            json!({"name": "cavity", "a": "run-1", "b": "run-2", "rel_tolerance": -1}),
        ),
        ("plan_update", json!({"names": ["a", "b", "cavity"]})),
        (
            "plan_update",
            json!({"names": ["a"], "items": ["a:result"]}),
        ),
        (
            "plan_update",
            json!({"names": ["a"], "items": ["../x:mesh"]}),
        ),
    ] {
        s.tool(tool, arguments);
    }
    let r = s.raw_tool("list_runs", json!({"name": "cavity", "path": "/x"}));
    assert!(refused(&r), "{r}");
    assert_nothing_private(&root, &s);
}

// ---- jobs on a document, started by name ----

/// The fake box's results download for these tests: a good run of the cavity, as the tar the fake
/// ssh sends. Returns the tar's path.
fn results_tar(root: &Path) -> String {
    let src = root.join("results-src");
    let spec = FixtureSpec {
        end_time: 5000,
        cells: 16384,
        samples: 64,
        shift: 0.0,
    };
    write_results(&src, &spec).unwrap();
    let tar = root.join("results.tar");
    let made = Command::new("tar")
        .arg("-C")
        .arg(&src)
        .arg("-cf")
        .arg(&tar)
        .arg(".")
        .status()
        .unwrap();
    assert!(made.success());
    tar.to_string_lossy().into_owned()
}

/// A server on a fake box that serves a good run, with the cavity saved as a document.
fn job_server(tag: &str) -> (PathBuf, Server) {
    let root = fresh(tag);
    let tar = results_tar(&root);
    put_document(&root, "cavity", CAVITY, false);
    let s = Server::start_with_env(&root, "ok", &[("CFC_TEST_RESULTS_TAR", tar.as_str())]);
    (root, s)
}

/// The cavity with another mesh of the same number of cells, so the served results still pass.
fn remeshed_study() -> String {
    let mut study = value(CAVITY);
    study["mesh"]["cells_x"] = json!(64);
    study["mesh"]["cells_y"] = json!(256);
    study.to_string()
}

/// Start a job that must be accepted, wait for it, and return the report it finished with.
fn run_job(s: &mut Server, arguments: Value) -> Value {
    let r = s.tool("start_document_job", arguments);
    assert_eq!(r["status"], "started", "{r}");
    let id = r["run_id"].as_str().unwrap().to_owned();
    wait_until("the job to finish", || {
        s.tool("run_status", json!({"run_id": id}))["status"] == "finished"
    });
    let g = s.tool("get_result", json!({"run_id": id}));
    assert_eq!(g["status"], "finished", "{g}");
    g["report"].clone()
}

#[test]
fn start_document_job_solves_over_the_protocol() {
    let (root, mut s) = job_server("jsolve");
    let report = run_job(&mut s, json!({"name": "cavity", "job": "solve"}));
    assert_eq!(report["exit_class"], 0, "{report}");
    assert_eq!(report["operation"], "solve", "{report}");
    assert_eq!(report["repro"]["kind"], "run", "{report}");
    assert_eq!(report["repro"]["run"], "run-1", "{report}");

    let listed = s.tool("list_runs", json!({"name": "cavity"}));
    let runs = listed["repro"]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1, "{listed}");
    assert_eq!(runs[0]["id"], "run-1");
    assert_eq!(run_count(&root), 1, "{:?}", events(&root));
}

#[test]
fn a_second_start_says_nothing_to_run() {
    let (root, mut s) = job_server("jagain");
    run_job(&mut s, json!({"name": "cavity", "job": "solve"}));

    let r = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(r["status"], "nothing_to_run", "{r}");
    assert!(r.get("run_id").is_none(), "nothing started: {r}");
    assert_eq!(r["report"]["exit_class"], 0, "{r}");
    assert_eq!(r["report"]["repro"]["recorded"], json!([]), "{r}");
    assert_eq!(r["report"]["repro"]["reused"], json!(["mesh", "result"]));
    assert_eq!(run_count(&root), 1, "{:?}", events(&root));
}

#[test]
fn a_rerun_and_an_update_item_over_the_protocol() {
    let (root, mut s) = job_server("jrerun");
    run_job(&mut s, json!({"name": "cavity", "job": "solve"}));

    let report = run_job(
        &mut s,
        json!({"name": "cavity", "job": "rerun", "run": "run-1"}),
    );
    assert_eq!(report["exit_class"], 0, "{report}");
    assert_eq!(report["operation"], "rerun", "{report}");
    assert_eq!(report["repro"]["kind"], "comparison", "{report}");
    assert_eq!(
        report["repro"]["comparison"]["verdict"], "reproduced",
        "{report}"
    );

    put_document(&root, "cavity", &remeshed_study(), true);
    let r = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "update_result"}),
    );
    assert_eq!(r["status"], "refused", "{r}");
    assert_eq!(r["report"]["exit_class"], 2, "{r}");
    assert!(codes_of(&r["report"]).contains(&"CFD-REPRO-008"), "{r}");
    assert_eq!(run_count(&root), 2, "{:?}", events(&root));

    let report = run_job(&mut s, json!({"name": "cavity", "job": "update_mesh"}));
    assert_eq!(report["exit_class"], 0, "{report}");
    assert_eq!(report["operation"], "update", "{report}");
    let report = run_job(&mut s, json!({"name": "cavity", "job": "update_result"}));
    assert_eq!(report["exit_class"], 0, "{report}");
    assert_eq!(run_count(&root), 4, "{:?}", events(&root));
}

#[test]
fn a_wrong_combination_is_a_refused_report() {
    let (root, mut s) = job_server("jwrong");
    for arguments in [
        json!({"name": "cavity", "job": "rerun"}),
        json!({"name": "cavity", "job": "solve", "run": "run-1"}),
        json!({"name": "cavity", "job": "update_mesh", "run": "run-1"}),
    ] {
        let r = s.tool("start_document_job", arguments.clone());
        assert_eq!(r["status"], "refused", "{arguments}: {r}");
        assert_eq!(r["report"]["exit_class"], 2, "{arguments}: {r}");
        assert_eq!(codes_of(&r["report"]), ["CFD-CLI-001"], "{r}");
    }
    let r = s.raw_tool(
        "start_document_job",
        json!({"name": "cavity", "job": "frobnicate"}),
    );
    assert!(refused(&r), "an unknown job word is refused: {r}");
    assert_eq!(run_count(&root), 0, "{:?}", events(&root));
}

#[test]
fn a_busy_host_is_busy_over_the_protocol() {
    let (root, mut s) = job_server("jbusy");
    hold_lock(&root, now_epoch(), 3600);
    let r = s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    assert_eq!(r["status"], "busy", "{r}");
    assert_eq!(r["holder"]["run_id"], "other-run-1", "{r}");
    assert!(r["retry_after_seconds"].as_u64().is_some(), "{r}");
    assert!(r.get("run_id").is_none(), "nothing was queued: {r}");
    assert!(
        lock_dir(&root).is_dir(),
        "somebody else's lock is never removed"
    );
    assert_eq!(run_count(&root), 0, "{:?}", events(&root));
}

#[test]
fn a_full_session_with_document_jobs_prints_nothing_private() {
    let (root, mut s) = job_server("jpriv");
    run_job(&mut s, json!({"name": "cavity", "job": "solve"}));
    s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve"}),
    );
    s.tool(
        "start_document_job",
        json!({"name": "cavity", "job": "rerun"}),
    );
    s.tool(
        "start_document_job",
        json!({"name": "absent", "job": "solve"}),
    );
    s.tool(
        "start_document_job",
        json!({"name": "../x", "job": "solve"}),
    );
    run_job(
        &mut s,
        json!({"name": "cavity", "job": "rerun", "run": "run-1"}),
    );
    let r = s.raw_tool(
        "start_document_job",
        json!({"name": "cavity", "job": "solve", "path": "/x"}),
    );
    assert!(refused(&r), "{r}");
    assert_nothing_private(&root, &s);
}
