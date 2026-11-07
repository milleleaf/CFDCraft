//! The benchmark and mesh independence tools over `cfdcraft mcp` as a real process: the list and
//! the study text, the check of a recorded run, the sweep and its report, and the inputs the five
//! tools take. Nothing here needs a compute host: no tool starts anything, and the fake box only
//! gives the server the files its settings name.
#![cfg(unix)]

#[path = "../../cfdcraft-ops/tests/support/mod.rs"]
mod support;

use cfdcraft_ops::save_document;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::{Duration, Instant},
};
use support::*;

const GROUP: &str = "mcpb";

/// The five tools of this file.
const NEW_TOOLS: [&str; 5] = [
    "benchmark_list",
    "benchmark_create",
    "benchmark_check",
    "gci_create",
    "gci_report",
];

/// The only input names the five tools may have.
const ALLOWED_INPUTS: [&str; 14] = [
    "name",
    "run",
    "sweep",
    "scenario",
    "levels",
    "quantity",
    "benchmark",
    "mesh",
    "turbulence",
    "time",
    "delta_t",
    "init",
    "rayleigh",
    "reynolds",
];

/// A running `cfdcraft mcp` and a client for it, as small as the tests here need.
struct Server {
    child: Child,
    stdin: ChildStdin,
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

/// The cavity with 32 cells along each side, so the levels 0, 1 and 2 make 32, 64 and 128.
fn coarse_cavity() -> String {
    let mut study: Value = serde_json::from_str(CAVITY).unwrap();
    study["mesh"]["cells_x"] = json!(32);
    study["mesh"]["cells_y"] = json!(32);
    study.to_string()
}

/// Save the cavity as the document `name` in the workspace the server will use.
fn put_document(root: &Path, name: &str) {
    let path = document_file(root, name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let saved = save_document(&coarse_cavity(), &path, false);
    assert_eq!(saved.report.exit_class, 0, "{}", saved.report.render_text());
}

/// Every file under `dir` with its bytes, by path relative to `dir`.
fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let name = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.insert(name, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn codes_of(report: &Value) -> Vec<&str> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect()
}

/// Nothing an answer said names the workspace folder, the key, the hosts file or the ssh target.
fn assert_nothing_private(root: &Path, answer: &Value) {
    let text = answer.to_string();
    let folder = root.to_string_lossy();
    for needle in [folder.as_ref(), "dummy-key", "hosts.toml", SSH_TARGET] {
        assert!(!text.contains(needle), "{needle} is in {text}");
    }
}

impl Server {
    /// Start the server on the fake box and complete the handshake.
    fn start(root: &Path) -> Self {
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
        let stdin = child.stdin.take().unwrap();
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
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
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
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn benchmark_list_returns_five_entries() {
    let root = fresh("blist");
    let mut s = Server::start(&root);
    let before = files_under(&root.join("ws"));
    let r = s.tool("benchmark_list", json!({}));
    assert_eq!(r["exit_class"], 0, "{r}");
    assert_eq!(r["operation"], "benchmark_list", "{r}");
    assert_eq!(r["repro"]["kind"], "benchmarks", "{r}");
    let entries = r["repro"]["benchmarks"].as_array().unwrap();
    let mut ids: Vec<&str> = entries.iter().map(|e| e["id"].as_str().unwrap()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "backward-step",
            "backward-step-laminar",
            "cylinder",
            "heated-cavity",
            "lid-cavity"
        ],
        "{r}"
    );
    let judge = |id: &str| {
        entries
            .iter()
            .find(|e| e["id"] == id)
            .map(|e| e["benchmark"].as_str().unwrap().to_owned())
    };
    assert_eq!(judge("lid-cavity").as_deref(), Some("ghia"), "{r}");
    assert_eq!(judge("cylinder").as_deref(), Some("cylinder"), "{r}");
    assert_eq!(judge("heated-cavity").as_deref(), Some("dvd"), "{r}");
    assert_eq!(files_under(&root.join("ws")), before, "nothing was written");
    assert_nothing_private(&root, &r);
}

#[test]
fn benchmark_create_returns_the_study_text() {
    let root = fresh("bcreate");
    let mut s = Server::start(&root);
    let before = files_under(&root.join("ws"));

    let made = s.tool(
        "benchmark_create",
        json!({"benchmark": "cylinder", "mesh": "fine"}),
    );
    assert_eq!(made["report"]["exit_class"], 0, "{made}");
    assert_eq!(made["report"]["operation"], "benchmark_create", "{made}");
    let text = made["study"].as_str().expect("the study text is returned");
    let study: Value = serde_json::from_str(text).expect("the text is a study file");
    assert_eq!(study["mesh"]["level"], 2, "{text}");
    assert!(made["report"]["study_hash"].is_string(), "{made}");

    // What was made is a study the server accepts as it is.
    let lid = s.tool("benchmark_create", json!({"benchmark": "lid-cavity"}));
    assert_eq!(lid["report"]["exit_class"], 0, "{lid}");
    let lid_study: Value = serde_json::from_str(lid["study"].as_str().unwrap()).unwrap();
    let stored = s.tool("create_study", json!({"name": "lid", "study": lid_study}));
    assert_eq!(stored["exit_class"], 0, "{stored}");
    assert_eq!(
        stored["study_hash"], lid["report"]["study_hash"],
        "{stored}"
    );

    for arguments in [
        json!({"benchmark": "nope"}),
        json!({"benchmark": "cylinder", "mesh": "huge"}),
        json!({"benchmark": "cylinder", "delta_t": -1.0}),
        json!({"benchmark": "../x"}),
    ] {
        let refused = s.tool("benchmark_create", arguments.clone());
        assert_eq!(refused["report"]["exit_class"], 2, "{arguments}: {refused}");
        assert!(refused.get("study").is_none(), "{arguments}: {refused}");
        assert!(
            !codes_of(&refused["report"]).is_empty(),
            "{arguments}: {refused}"
        );
        assert_nothing_private(&root, &refused);
    }

    let mut after = files_under(&root.join("ws"));
    assert!(
        after.remove("studies/lid.json").is_some(),
        "create_study stored the study the agent sent"
    );
    assert_eq!(
        after, before,
        "benchmark_create wrote nothing in the workspace"
    );
    assert!(!root.join("ws/studies/cylinder.json").exists());
}

#[test]
fn benchmark_check_refuses_what_the_cli_refuses() {
    let root = fresh("bcheck");
    put_document(&root, "cavity");
    let mut s = Server::start(&root);
    let before = files_under(&root.join("ws"));

    let none = s.tool("benchmark_check", json!({"name": "cavity"}));
    assert_eq!(none["exit_class"], 2, "{none}");
    assert_eq!(none["operation"], "benchmark_check", "{none}");
    assert_eq!(codes_of(&none), ["CFD-REPRO-005"], "{none}");

    let absent = s.tool("benchmark_check", json!({"name": "absent"}));
    assert_eq!(absent["exit_class"], 2, "{absent}");
    assert_eq!(codes_of(&absent), ["CFD-DOC-001"], "{absent}");

    for arguments in [
        json!({"name": "../x"}),
        json!({"name": "cavity", "run": "../x"}),
        json!({"name": "cavity", "run": "Run 1"}),
        json!({"name": "cavity", "benchmark": "GHIA"}),
    ] {
        let bad = s.tool("benchmark_check", arguments.clone());
        assert_eq!(bad["exit_class"], 2, "{arguments}: {bad}");
        assert_eq!(codes_of(&bad), ["CFD-WS-001"], "{arguments}: {bad}");
        assert_nothing_private(&root, &bad);
    }
    assert_nothing_private(&root, &none);
    assert_nothing_private(&root, &absent);
    assert_eq!(files_under(&root.join("ws")), before, "nothing was written");
}

#[test]
fn gci_create_and_gci_report() {
    let root = fresh("gci");
    put_document(&root, "cavity");
    put_document(&root, "other");
    let mut s = Server::start(&root);
    let other = std::fs::read(document_file(&root, "other")).unwrap();

    let created = s.tool("gci_create", json!({"name": "cavity", "sweep": "mesh"}));
    assert_eq!(created["exit_class"], 0, "{created}");
    assert_eq!(created["operation"], "gci_create", "{created}");
    assert_eq!(created["repro"]["kind"], "sweep", "{created}");
    assert_eq!(created["repro"]["stored"], true, "{created}");
    assert_eq!(created["repro"]["points"].as_array().unwrap().len(), 3);

    let twice = s.tool("gci_create", json!({"name": "cavity", "sweep": "mesh"}));
    assert_eq!(twice["exit_class"], 2, "{twice}");
    assert_eq!(codes_of(&twice), ["CFD-SWP-002"], "{twice}");

    let waiting = s.tool(
        "gci_report",
        json!({"name": "cavity", "sweep": "mesh", "quantity": "bench.ghia.u_centre"}),
    );
    assert_eq!(waiting["exit_class"], 2, "{waiting}");
    assert_eq!(waiting["operation"], "gci_report", "{waiting}");
    assert_eq!(codes_of(&waiting), ["CFD-GCI-001"], "{waiting}");
    let message = waiting["diagnostics"][0]["message"].as_str().unwrap();
    for point in ["mesh-001", "mesh-002", "mesh-003"] {
        assert!(message.contains(point), "{point} is waiting: {message}");
    }

    let listed = s.tool("scenario_list", json!({"name": "cavity"}));
    assert_eq!(
        listed["repro"]["scenarios"].as_array().unwrap().len(),
        4,
        "the study and one variant for each level: {listed}"
    );

    // A sweep that is not stored, a bad level list, and words that are not names.
    let unknown = s.tool(
        "gci_report",
        json!({"name": "cavity", "sweep": "nope", "quantity": "x"}),
    );
    assert_eq!(unknown["exit_class"], 2, "{unknown}");
    assert_eq!(codes_of(&unknown), ["CFD-GCI-001"], "{unknown}");
    let few = s.tool(
        "gci_create",
        json!({"name": "cavity", "sweep": "few", "levels": [0, 1]}),
    );
    assert_eq!(few["exit_class"], 2, "{few}");
    assert_eq!(codes_of(&few), ["CFD-SWP-002"], "{few}");
    let bad_sweep = s.tool("gci_create", json!({"name": "cavity", "sweep": "Mesh One"}));
    assert_eq!(codes_of(&bad_sweep), ["CFD-WS-001"], "{bad_sweep}");
    let bad_quantity = s.tool(
        "gci_report",
        json!({"name": "cavity", "sweep": "mesh", "quantity": ""}),
    );
    assert_eq!(bad_quantity["exit_class"], 2, "{bad_quantity}");
    assert_eq!(codes_of(&bad_quantity), ["CFD-CLI-001"], "{bad_quantity}");

    assert_eq!(
        std::fs::read(document_file(&root, "other")).unwrap(),
        other,
        "only the named document changed"
    );
    for answer in [&created, &twice, &waiting, &unknown, &few, &bad_quantity] {
        assert_nothing_private(&root, answer);
    }
}

#[test]
fn the_inputs_name_no_path_key_host_or_command() {
    let root = fresh("inputs");
    let mut s = Server::start(&root);
    let r = s.call("tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 43, "the closed surface has forty-three tools");
    for name in NEW_TOOLS {
        let tool = tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is advertised"));
        let inputs: Vec<&str> = tool["inputSchema"]["properties"]
            .as_object()
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();
        for input in &inputs {
            assert!(
                ALLOWED_INPUTS.contains(input),
                "{name} takes `{input}`, which is not an allowed input"
            );
        }
        if name == "benchmark_list" {
            assert!(inputs.is_empty(), "{name} takes nothing: {inputs:?}");
        }
        assert_eq!(
            tool["inputSchema"]["additionalProperties"], false,
            "{name}: unknown arguments must be refused"
        );
        let description = tool["description"].as_str().unwrap();
        assert!(
            description.contains("starts nothing"),
            "{name} says that it starts nothing: {description}"
        );
        assert!(
            description.contains("writes nothing")
                || description.contains("Writes nothing")
                || description.contains("Changes only the document named"),
            "{name} says whether it writes: {description}"
        );
    }

    // An argument that names a path, a key, a host or a command is refused by every one of them.
    let valid: [(&str, Value); 5] = [
        ("benchmark_list", json!({})),
        ("benchmark_create", json!({"benchmark": "cylinder"})),
        ("benchmark_check", json!({"name": "cavity"})),
        ("gci_create", json!({"name": "cavity", "sweep": "mesh"})),
        (
            "gci_report",
            json!({"name": "cavity", "sweep": "mesh", "quantity": "x"}),
        ),
    ];
    let before = files_under(&root.join("ws"));
    for (tool, arguments) in valid {
        for extra in ["path", "file", "key", "host", "command"] {
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
    assert_eq!(files_under(&root.join("ws")), before, "nothing was written");
}
