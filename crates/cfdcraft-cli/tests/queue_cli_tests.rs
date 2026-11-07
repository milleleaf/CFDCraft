//! The queue verbs, run through the real binary: adding jobs, listing them and the refusals. No
//! test needs a compute host, and none of these verbs starts a run.

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cfdcraft_backend_esi::Dialect;
use cfdcraft_doc::{open, store_dir};
use cfdcraft_drain::LiveSpec;
use cfdcraft_ops::convergence::{settings_for, spec_for};
use cfdcraft_ops::{prepare, solve_document};
use repro_support::{CAVITY, FixtureRunner, new_document, passed, scratch, write_monitor_files};
use serde_json::{Value, json};

const LID: &str = "/boundary_conditions/lid/velocity/value";

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

fn json_out(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is exactly one JSON document")
}

/// The finding codes of a JSON report.
fn codes_of(report: &Value) -> Vec<String> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

/// A new document of the cavity.
fn cavity(tag: &str) -> PathBuf {
    new_document(&scratch(tag), "cavity", CAVITY)
}

/// A document of the cavity with a stored sweep `lids` of three points and the variant
/// `fast-lid`, which is not one of the points.
fn with_sweep_and_variant(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", CAVITY);
    let spec = json!({
        "id": "lids",
        "base": "base",
        "parameters": [{
            "name": "lid_speed",
            "target": { "kind": "number", "setting": LID },
            "values": { "mode": "list", "items": [0.2, 0.3, 0.4] },
        }],
    });
    let file = dir.join("sweep.json");
    fs::write(&file, spec.to_string()).unwrap();
    let o = run(&["sweep", "create", s(&doc), s(&file)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let set = format!("{LID}=0.5");
    let o = run(&["scenario", "set", s(&doc), "fast-lid", "--set", &set]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    doc
}

/// The `jobs` of the queue as the list verb reports them in JSON.
fn jobs(doc: &Path) -> Vec<Value> {
    let o = run(&["queue", "list", s(doc), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    json_out(&o)["repro"]["jobs"].as_array().unwrap().clone()
}

#[test]
fn queue_add_then_list_shows_one_waiting_job() {
    let doc = cavity("qcli-add-list");
    let o = run(&["queue", "add", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Added 1 jobs to the queue; 1 are waiting."),
        "{}",
        out(&o)
    );

    let o = run(&["queue", "list", s(&doc), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let report = json_out(&o);
    assert_eq!(report["repro"]["kind"], "queue");
    let jobs = report["repro"]["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["id"], "q-001");
    assert_eq!(jobs[0]["state"], "queued");
}

#[test]
fn add_a_sweep_and_a_variant_and_a_continue() {
    let doc = with_sweep_and_variant("qcli-add-kinds");
    let o = run(&["queue", "add", s(&doc), "--sweep", "lids"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Added 3 jobs to the queue; 3 are waiting."),
        "{}",
        out(&o)
    );
    let o = run(&["queue", "add", s(&doc), "--scenario", "fast-lid"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Added 1 jobs to the queue; 4 are waiting."),
        "{}",
        out(&o)
    );
    let scenarios: Vec<Option<String>> = jobs(&doc)
        .iter()
        .map(|job| job["scenario"].as_str().map(str::to_owned))
        .collect();
    let want = ["lids-001", "lids-002", "lids-003", "fast-lid"];
    assert_eq!(scenarios, want.map(|id| Some(id.to_owned())));

    // Two selectors at once are refused, and the queue stays as it was.
    let o = run(&[
        "queue",
        "add",
        s(&doc),
        "--scenario",
        "fast-lid",
        "--sweep",
        "lids",
        "--json",
    ]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert_eq!(codes_of(&json_out(&o)), ["CFD-QUEUE-001"]);
    assert_eq!(jobs(&doc).len(), 4);

    // A run that stopped on request can be continued.
    let dir = scratch("qcli-add-continue");
    let stopped = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.stopped_at = Some("1200".to_owned());
    passed(&solve_document(&stopped, &mut runner).report);
    let o = run(&["queue", "add", s(&stopped), "--continue", "run-1"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let queued = jobs(&stopped);
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0]["kind"], "continue");

    // A continue is not combined with a variant.
    let o = run(&[
        "queue",
        "add",
        s(&stopped),
        "--continue",
        "run-1",
        "--scenario",
        "fast-lid",
        "--json",
    ]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert_eq!(codes_of(&json_out(&o)), ["CFD-QUEUE-001"]);
}

#[test]
fn add_with_nothing_to_do_exits_zero() {
    let dir = scratch("qcli-add-nothing");
    let doc = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&doc, &mut runner).report);

    let o = run(&["queue", "add", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Nothing to add: everything asked for is up to date or already queued."),
        "{}",
        out(&o)
    );
    let o = run(&["queue", "add", s(&doc), "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(codes_of(&json_out(&o)), ["CFD-QUEUE-005"]);
    assert!(jobs(&doc).is_empty());
}

#[test]
fn list_text_output_has_one_line_per_job() {
    let doc = with_sweep_and_variant("qcli-list-text");
    let o = run(&["queue", "add", s(&doc), "--sweep", "lids"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let o = run(&["queue", "add", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));

    let o = run(&["queue", "list", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = out(&o);
    let rows: Vec<Vec<&str>> = shown
        .lines()
        .map(|line| line.split_whitespace().collect())
        .collect();
    assert_eq!(rows.len(), 4, "{shown}");
    assert_eq!(rows[0], ["q-001", "queued", "lids-001"]);
    assert_eq!(rows[2], ["q-003", "queued", "lids-003"]);
    assert_eq!(rows[3], ["q-004", "queued", "-"]);
}

/// The ids of the jobs in the order they start.
fn order(doc: &Path) -> Vec<String> {
    jobs(doc)
        .iter()
        .map(|job| job["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn reorder_cancel_and_stop_print_the_operation_sentences() {
    let doc = with_sweep_and_variant("qcli-moves");
    let o = run(&["queue", "add", s(&doc), "--sweep", "lids"]);
    assert_eq!(code(&o), 0, "{}", err(&o));

    // Place 1 is the first to start.
    let o = run(&["queue", "reorder", s(&doc), "q-003", "--to", "1"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Moved q-003 to position 1 among the waiting jobs."),
        "{}",
        out(&o)
    );
    assert_eq!(order(&doc), ["q-003", "q-001", "q-002"]);
    let o = run(&["queue", "reorder", s(&doc), "q-003", "--to", "0"]);
    assert_eq!(code(&o), 2, "{}", err(&o));

    let o = run(&["queue", "cancel", s(&doc), "q-002"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("Cancelled q-002."), "{}", out(&o));
    let states: Vec<String> = jobs(&doc)
        .iter()
        .map(|job| job["state"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(states, ["queued", "queued", "cancelled"]);

    // Only a running job can be asked to stop.
    let o = run(&["queue", "stop", s(&doc), "q-001", "--json"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert_eq!(codes_of(&json_out(&o)), ["CFD-QUEUE-003"]);
    let o = run(&["queue", "cancel", s(&doc), "q-009", "--json"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert_eq!(codes_of(&json_out(&o)), ["CFD-QUEUE-002"]);
    assert_eq!(order(&doc), ["q-003", "q-001", "q-002"]);
}

/// The log the mirror of a job holds: the third line names a path on the compute host.
const LOG_LINES: [&str; 8] = [
    "Time = 1",
    "Time = 2",
    "Reading /home/user/secret/path from cfdcraft-cavity-1-2 done",
    "Time = 3",
    "Time = 4",
    "Time = 5",
    "Time = 6",
    "Time = 7",
];

/// What the runner has copied of the job `job` so far: what to judge by, the five probe files, the
/// flux files and a log. Nothing here needs a compute host.
fn mirror(doc: &Path, job: &str) -> PathBuf {
    let live = store_dir(doc).join("live").join(job);
    fs::create_dir_all(&live).unwrap();
    let prepared = prepare(CAVITY, Dialect::EsiV2512).expect("the cavity passes the gates");
    let study = open(doc).unwrap().document.study;
    let given = LiveSpec {
        spec: spec_for(&prepared.resolved),
        settings: settings_for(&study),
    };
    fs::write(
        live.join("spec.json"),
        serde_json::to_string(&given).unwrap(),
    )
    .unwrap();
    write_monitor_files(&live);
    fs::write(live.join("log.simpleFoam"), LOG_LINES.join("\n") + "\n").unwrap();
    live
}

/// A document with one queued job whose mirror is in place.
fn watched(tag: &str) -> PathBuf {
    let doc = cavity(tag);
    let o = run(&["queue", "add", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    mirror(&doc, "q-001");
    doc
}

#[test]
fn watch_prints_the_monitor_table_the_balances_and_the_verdict() {
    let doc = watched("qcli-watch");
    let o = run(&["queue", "watch", s(&doc), "--job", "q-001"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = out(&o);
    let rows: Vec<&str> = shown.lines().filter(|l| l.contains(": last ")).collect();
    assert_eq!(rows.len(), 10, "five probes, two components each:\n{shown}");
    let first = rows.iter().find(|l| l.starts_with("probe_1.Ux: last "));
    let first = first.unwrap_or_else(|| panic!("probe_1.Ux is missing from:\n{shown}"));
    assert!(first.contains(", tail mean "), "{first}");
    assert!(first.contains(", spread "), "{first}");
    assert!(first.ends_with(": settled."), "{first}");
    assert!(shown.contains("Mass balance: "), "{shown}");
    assert!(shown.contains("Energy balance: "), "{shown}");
    let verdict = [
        "Converged:",
        "Not converged:",
        "Convergence cannot be judged:",
    ];
    assert!(
        shown
            .lines()
            .any(|l| verdict.iter().any(|v| l.starts_with(v))),
        "{shown}"
    );

    // Without --job the only job is the one shown.
    let o = run(&["queue", "watch", s(&doc)]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(out(&o), shown);
}

#[test]
fn watch_json_is_one_report() {
    let doc = watched("qcli-watch-json");
    let o = run(&["queue", "watch", s(&doc), "--job", "q-001", "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let report = json_out(&o);
    assert_eq!(report["operation"], "queue-watch");
    let table = &report["monitors"];
    assert_eq!(table["job"], "q-001");
    let rows = table["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    assert!(rows.iter().any(|row| row["name"] == "probe_1.Ux"));
    assert!(table["verdict"]["state"].is_string());
    assert!(table["mass"]["status"].is_string());
}

#[test]
fn follow_repeats_until_the_job_ends() {
    let doc = watched("qcli-follow");
    let o = run(&["queue", "cancel", s(&doc), "q-001"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    // The job has ended, so following looks once and returns.
    let o = run(&[
        "queue",
        "watch",
        s(&doc),
        "--job",
        "q-001",
        "--follow",
        "--poll",
        "0",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = out(&o);
    assert_eq!(shown.matches("Mass balance: ").count(), 1, "{shown}");
}

#[test]
fn log_prints_a_redacted_tail_and_the_next_offset() {
    let doc = watched("qcli-log");
    let o = run(&[
        "queue",
        "log",
        s(&doc),
        "q-001",
        "--offset",
        "0",
        "--lines",
        "5",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = out(&o);
    assert_eq!(shown.lines().count(), 5, "{shown}");
    assert!(shown.contains("Time = 4"), "{shown}");
    assert!(!shown.contains("Time = 5"), "{shown}");
    assert!(shown.contains("Reading <path> from <run> done"), "{shown}");
    assert!(!shown.contains("/home/user"), "{shown}");
    assert!(!shown.contains("cfdcraft-cavity"), "{shown}");
    // The next piece begins after the fifth line of the raw log.
    let used: usize = LOG_LINES[..5].iter().map(|line| line.len() + 1).sum();
    assert!(
        err(&o).contains(&format!("next offset: {used}")),
        "{}",
        err(&o)
    );

    let from = used.to_string();
    let o = run(&[
        "queue",
        "log",
        s(&doc),
        "q-001",
        "--offset",
        &from,
        "--json",
    ]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let report = json_out(&o);
    let text = report["log"]["text"].as_str().unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        ["Time = 5", "Time = 6", "Time = 7"]
    );
    let total: usize = LOG_LINES.iter().map(|line| line.len() + 1).sum();
    assert_eq!(report["log"]["next_offset"], total);
}

#[test]
fn a_bad_job_id_is_refused() {
    let doc = watched("qcli-bad-job");
    let d = s(&doc);
    let cases: [&[&str]; 2] = [
        &["queue", "watch", d, "--job", "../x"],
        &["queue", "log", d, "../x", "--lines", "5"],
    ];
    for args in cases {
        let o = run(args);
        assert_eq!(code(&o), 2, "{}", err(&o));
        assert!(out(&o).is_empty(), "{}", out(&o));
        assert!(err(&o).contains("is not a job id"), "{}", err(&o));
    }
    let o = run(&["queue", "watch", s(&doc), "--job", "q-009"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(err(&o).contains("there is no job q-009"), "{}", err(&o));
}

#[test]
fn monitors_of_a_recorded_run() {
    let dir = scratch("qcli-recorded");
    let doc = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.monitors = true;
    passed(&solve_document(&doc, &mut runner).report);

    let o = run(&["monitors", s(&doc), "--run", "run-1"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let shown = out(&o);
    assert!(shown.contains("probe_1.Ux: last "), "{shown}");
    assert!(shown.contains("recomputed: Converged:"), "{shown}");
    assert!(shown.contains("recorded: Converged:"), "{shown}");

    let o = run(&["monitors", s(&doc), "--run", "run-1", "--json"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let report = json_out(&o);
    assert_eq!(report["monitors"]["run"], "run-1");
    assert_eq!(report["monitors"]["verdict"]["state"], "converged");
    assert_eq!(report["monitors"]["recorded"]["state"], "converged");

    // A run and a job are not asked for together, and a run that is not there is refused.
    let o = run(&["monitors", s(&doc), "--run", "run-1", "--job", "q-001"]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    for wrong in ["run-9", "../x"] {
        let o = run(&["monitors", s(&doc), "--run", wrong]);
        assert_eq!(code(&o), 2, "{}", err(&o));
        assert!(out(&o).is_empty(), "{}", out(&o));
    }
}

#[test]
fn the_help_lists_the_verbs() {
    let o = run(&["queue", "--help"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let help = out(&o);
    for verb in [
        "add", "list", "reorder", "cancel", "stop", "drain", "watch", "log",
    ] {
        assert!(
            help.lines()
                .any(|line| line.split_whitespace().next() == Some(verb)),
            "{verb} is missing from:\n{help}"
        );
    }
}
