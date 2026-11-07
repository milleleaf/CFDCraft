//! Jobs started on a stored document by name: solve, rerun and the update of one item. The compute
//! host is a fake ssh and a fake docker that answers with one prepared results archive, so a job
//! records a mesh and a result without any solver.
#![cfg(unix)]

mod support;

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_compare::Verdict;
use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::{
    DocJobKind, ExitClass, Report, Repro, ResultOutcome, RunRegistry, ServerSettings, Stage,
    StartOutcome, Workspace, save_document, scenario_ops::scenario_set,
};
use cfdcraft_scenarios::Override;
use cfdcraft_store::Store;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use support::*;

const GROUP: &str = "ops-workspace-jobs";

/// A fake box. With `good_results` its results download is a good run of the cavity, otherwise it
/// is an empty archive that fails the evidence check.
fn box_for(tag: &str, good_results: bool) -> PathBuf {
    let root = new_root(GROUP, tag);
    if good_results {
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
    }
    root
}

struct Setup {
    ws: Workspace,
    settings: ServerSettings,
    registry: RunRegistry,
    doc: PathBuf,
}

/// The cavity stored as `cavity` and saved as a document, on the fake box under `root`.
fn setup(root: &Path) -> Setup {
    let ws = Workspace::open(root.join("ws")).unwrap();
    let study: serde_json::Value = serde_json::from_str(CAVITY).unwrap();
    assert_eq!(ws.create_study("cavity", &study).class(), ExitClass::Passed);
    assert_eq!(ws.save_document("cavity").class(), ExitClass::Passed);
    let settings = ServerSettings::new(
        root.join("dummy-key"),
        root.join("hosts.toml"),
        "box",
        workspace_root().join("target/cfdcraft-sockets"),
    )
    .unwrap();
    Setup {
        doc: root.join("ws/documents/cavity.cfdcraft"),
        ws,
        settings,
        registry: RunRegistry::new(),
    }
}

impl Setup {
    fn start(&self, name: &str, kind: DocJobKind) -> StartOutcome {
        self.ws
            .start_document_job(&self.registry, &self.settings, name, kind)
    }

    /// Start a job that must be accepted and wait for its report.
    fn run(&self, kind: DocJobKind) -> Report {
        let id = match self.start("cavity", kind) {
            StartOutcome::Started { run_id } => run_id,
            other => panic!("expected Started, got {other:?}"),
        };
        let begun = Instant::now();
        loop {
            match self.ws.run_result(&self.registry, &id) {
                ResultOutcome::Finished(report) => return *report,
                ResultOutcome::Running => {
                    assert!(
                        begun.elapsed() < Duration::from_secs(30),
                        "timed out: the job to finish"
                    );
                    std::thread::sleep(Duration::from_millis(50));
                }
                ResultOutcome::NotFound => panic!("the registry does not know {id}"),
            }
        }
    }

    /// Start a job that must be refused.
    fn refused(&self, name: &str, kind: DocJobKind) -> Report {
        match self.start(name, kind) {
            StartOutcome::Refused(report) => *report,
            other => panic!("expected Refused, got {other:?}"),
        }
    }

    fn bytes(&self) -> Vec<u8> {
        std::fs::read(&self.doc).unwrap()
    }

    fn runs(&self) -> Vec<cfdcraft_runs::RunRecord> {
        open(&self.doc).unwrap().document.artifacts.runs
    }

    /// Save the cavity with a mesh of other cells but the same number of them, so the prepared
    /// results still pass.
    fn remesh(&self) {
        let mut study: serde_json::Value = serde_json::from_str(CAVITY).unwrap();
        study["mesh"]["cells_x"] = serde_json::Value::from(64);
        study["mesh"]["cells_y"] = serde_json::Value::from(256);
        let saved = save_document(&study.to_string(), &self.doc, true);
        assert_eq!(saved.report.class(), ExitClass::Passed);
    }
}

fn codes(report: &Report) -> Vec<&str> {
    report.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn passed(report: &Report) {
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
}

/// Child side. Does nothing unless the parent set the root. Each scenario asserts for itself.
#[test]
fn jobs_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let s = setup(&root);
    match std::env::var("CFC_TEST_SCENARIO").unwrap().as_str() {
        "solve" => {
            let report = s.run(DocJobKind::Solve);
            passed(&report);
            assert_eq!(report.operation, "solve");
            let Some(Repro::Run(data)) = &report.repro else {
                panic!("expected the data of a solve: {report:?}");
            };
            assert_eq!(data.run.as_deref(), Some("run-1"));
            assert_eq!(data.recorded, ["mesh", "result"]);
            let shown = serde_json::to_string(&report).unwrap();
            assert!(
                !shown.contains(&root.to_string_lossy().into_owned()),
                "{shown}"
            );

            let runs = s.runs();
            assert_eq!(runs.len(), 1);
            let run = &runs[0];
            assert_eq!(run.id, "run-1");
            let store = Store::open(store_dir(&s.doc)).unwrap();
            store.verify(&run.mesh.as_ref().unwrap().blob).unwrap();
            store.verify(&run.result.as_ref().unwrap().blob).unwrap();
        }
        "again" => {
            passed(&s.run(DocJobKind::Solve));
            let before = s.bytes();
            let report = s.refused("cavity", DocJobKind::Solve);
            passed(&report);
            assert_eq!(report.operation, "solve");
            let Some(Repro::Run(data)) = &report.repro else {
                panic!("expected the data of a solve: {report:?}");
            };
            assert_eq!(data.run, None);
            assert!(data.recorded.is_empty());
            assert_eq!(data.reused, ["mesh", "result"]);
            assert_eq!(s.bytes(), before);
        }
        "rerun" => {
            passed(&s.run(DocJobKind::Solve));
            let report = s.run(DocJobKind::Rerun {
                run: "run-1".to_owned(),
            });
            passed(&report);
            assert_eq!(report.operation, "rerun");
            let Some(Repro::Comparison(compared)) = &report.repro else {
                panic!("expected a comparison: {report:?}");
            };
            assert_eq!(compared.original, "run-1");
            assert_eq!(compared.comparison.verdict, Verdict::Reproduced);

            let runs = s.runs();
            assert_eq!(runs.len(), 2);
            assert_eq!(runs[1].id, "run-2");
            assert_eq!(runs[1].rerun_of.as_deref(), Some("run-1"));
            assert!(runs[1].comparison.is_some());
        }
        "item" => {
            passed(&s.run(DocJobKind::Solve));
            s.remesh();
            let before = s.bytes();
            let report = s.refused("cavity", DocJobKind::Item(Stage::Result));
            assert_eq!(report.class(), ExitClass::StudyInvalid);
            assert!(
                codes(&report).contains(&"CFD-REPRO-008"),
                "{}",
                report.render_text()
            );
            assert_eq!(s.bytes(), before);

            let report = s.run(DocJobKind::Item(Stage::Mesh));
            passed(&report);
            assert_eq!(report.operation, "update");
            let runs = s.runs();
            assert_eq!(runs.len(), 2);
            assert!(runs[1].mesh.is_some() && runs[1].result.is_none());

            let report = s.run(DocJobKind::Item(Stage::Result));
            passed(&report);
            assert_eq!(report.operation, "update");
            let runs = s.runs();
            assert_eq!(runs.len(), 3);
            assert!(runs[2].mesh.is_none() && runs[2].result.is_some());
        }
        "busy" => {
            let before = s.bytes();
            hold_lock(&root, now_epoch(), 3600);
            match s.start("cavity", DocJobKind::Solve) {
                StartOutcome::Busy {
                    holder,
                    retry_after_seconds,
                    ..
                } => {
                    assert_eq!(holder.unwrap().run_id, "other-run-1");
                    assert!(retry_after_seconds > 0);
                }
                other => panic!("expected Busy, got {other:?}"),
            }
            assert_eq!(s.bytes(), before);
        }
        "names" => {
            let report = s.refused("../x", DocJobKind::Solve);
            assert_eq!(report.class(), ExitClass::StudyInvalid);
            assert_eq!(codes(&report), ["CFD-WS-001"]);

            let report = s.refused("absent", DocJobKind::Solve);
            assert_eq!(report.class(), ExitClass::StudyInvalid);
            assert!(
                codes(&report).contains(&"CFD-DOC-001"),
                "{}",
                report.render_text()
            );
            let shown = serde_json::to_string(&report).unwrap();
            assert!(
                !shown.contains(&root.to_string_lossy().into_owned()),
                "{shown}"
            );

            passed(&s.run(DocJobKind::Solve));
            let before = s.bytes();
            let report = s.refused(
                "cavity",
                DocJobKind::Rerun {
                    run: "run-9".to_owned(),
                },
            );
            assert_eq!(report.class(), ExitClass::StudyInvalid);
            assert!(
                codes(&report).contains(&"CFD-REPRO-004"),
                "{}",
                report.render_text()
            );
            assert_eq!(s.bytes(), before);
        }
        "failed" => {
            let before = s.bytes();
            let report = s.run(DocJobKind::Solve);
            assert_eq!(report.class(), ExitClass::GateFailed);
            assert_eq!(report.operation, "solve");
            assert_eq!(s.bytes(), before);
            assert!(s.runs().is_empty());
        }
        "variant" => {
            // A finer mesh of the same number of cells, so the prepared results still pass.
            let cells = |setting: &str, count: u32| Override::Set {
                setting: setting.to_owned(),
                value: serde_json::Value::from(count),
            };
            let made = scenario_set(
                &s.doc,
                "fine-mesh",
                None,
                &[cells("/mesh/cells_x", 64), cells("/mesh/cells_y", 256)],
                None,
            );
            passed(&made.report);
            let solve = || DocJobKind::SolveScenario {
                scenario: Some("fine-mesh".to_owned()),
                remake_pruned: false,
            };
            let report = s.run(solve());
            passed(&report);
            assert_eq!(report.operation, "solve");
            let runs = s.runs();
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].scenario.as_deref(), Some("fine-mesh"));

            let before = s.bytes();
            let report = s.refused("cavity", solve());
            passed(&report);
            let Some(Repro::Run(data)) = &report.repro else {
                panic!("expected the data of a solve: {report:?}");
            };
            assert_eq!(data.run, None);
            assert!(data.recorded.is_empty());
            assert_eq!(s.bytes(), before);
        }
        "unknown-variant" => {
            let before = s.bytes();
            let report = s.refused(
                "cavity",
                DocJobKind::SolveScenario {
                    scenario: Some("nope".to_owned()),
                    remake_pruned: false,
                },
            );
            assert_eq!(report.class(), ExitClass::StudyInvalid);
            assert_eq!(codes(&report), ["CFD-SCN-001"], "{}", report.render_text());
            assert_eq!(s.bytes(), before);
            assert!(s.runs().is_empty());
        }
        other => panic!("unknown scenario {other}"),
    }
}

fn scenario(tag: &str, which: &str, good_results: bool) -> PathBuf {
    let root = box_for(tag, good_results);
    let mut c = worker(&root, "jobs_worker", "ok");
    c.env("CFC_TEST_SCENARIO", which);
    if good_results {
        c.env("CFC_TEST_RESULTS_TAR", root.join("results.tar"));
    }
    assert_worker_ok(&c.output().unwrap());
    root
}

fn containers(root: &Path) -> usize {
    events(root).iter().filter(|e| *e == "run").count()
}

#[test]
fn a_solve_job_records_the_run_in_the_named_document() {
    let root = scenario("solve", "solve", true);
    assert_eq!(containers(&root), 1, "{:?}", events(&root));
    assert!(!lock_dir(&root).exists());
}

#[test]
fn a_second_solve_is_nothing_to_run_and_starts_no_container() {
    let root = scenario("again", "again", true);
    assert_eq!(containers(&root), 1, "{:?}", events(&root));
}

#[test]
fn a_rerun_job_records_a_comparison() {
    let root = scenario("rerun", "rerun", true);
    assert_eq!(containers(&root), 2, "{:?}", events(&root));
}

#[test]
fn an_item_job_for_a_result_with_a_pending_mesh_is_refused() {
    let root = scenario("item", "item", true);
    assert_eq!(containers(&root), 3, "{:?}", events(&root));
}

#[test]
fn a_busy_host_is_busy_and_nothing_changes() {
    let root = scenario("busy", "busy", true);
    assert_eq!(containers(&root), 0, "{:?}", events(&root));
    assert!(lock_dir(&root).join("holder.json").exists());
}

#[test]
fn bad_names_and_missing_documents_are_refused_with_reports() {
    let root = scenario("names", "names", true);
    assert_eq!(containers(&root), 1, "{:?}", events(&root));
}

#[test]
fn a_failed_case_records_nothing() {
    let root = scenario("failed", "failed", false);
    assert_eq!(containers(&root), 1, "{:?}", events(&root));
}

#[test]
fn a_solve_job_for_a_variant_records_a_tagged_run() {
    let root = scenario("variant", "variant", true);
    assert_eq!(containers(&root), 1, "{:?}", events(&root));
}

#[test]
fn an_unknown_variant_in_a_job_is_a_refused_start() {
    let root = scenario("unknown-variant", "unknown-variant", true);
    assert_eq!(containers(&root), 0, "{:?}", events(&root));
}
