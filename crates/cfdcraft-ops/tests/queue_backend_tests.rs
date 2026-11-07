//! A queue job names its backend: the add path checks the id before anything is stored, the job
//! is planned for that backend's tools, a run request can carry the id, and a backend that has a
//! verdict of its own on convergence is asked before the OpenFOAM assessment. No test needs a
//! compute host or a container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_backend::{
    Backend, CaseFile, CaseTree, CompileFailure, CompileOptions, ConvergenceOutcome, Family,
    LimitKind, Refusal, RunFiles, RunPlan, RunStep,
};
use cfdcraft_doc::store_dir;
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy};
use cfdcraft_ops::backends::{default_row, rows};
use cfdcraft_ops::convergence::{assess_dir, spec_for, verdict_for};
use cfdcraft_ops::jobs::{JobAction, JobOptions, plan_scenario_job_on};
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_ops::queue_ops::{AddRequest, queue_add};
use cfdcraft_ops::{ExitClass, RunMode, RunRequest, begin};
use cfdcraft_settle::{Settings, VerdictState};

#[path = "plan_studies/mod.rs"]
mod plan_studies;
#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::{codes, new_document, passed, scratch};

const CAVITY: &str = plan_studies::CAVITY;

const SOURCES: [(&str, &str); 1] = [("writer.rs", "fn write() {}")];

/// A backend whose verdict on convergence is whatever the test sets.
struct Fake {
    verdict: Option<(bool, &'static str)>,
}

impl Backend for Fake {
    fn id(&self) -> &'static str {
        "fake-1"
    }
    fn family(&self) -> Family {
        Family::Lbm
    }
    fn image(&self) -> Option<&'static str> {
        Some("example/fake@sha256:abc")
    }
    fn preview(&self) -> bool {
        false
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::unrestricted()
    }
    fn compile(
        &self,
        _study: &ResolvedStudy,
        _options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure> {
        Ok(CaseTree {
            files: vec![CaseFile {
                rel_path: "input.txt".into(),
                bytes: b"hello".to_vec(),
            }],
        })
    }
    fn scan(&self, _tree: &CaseTree) -> Vec<Refusal> {
        Vec::new()
    }
    fn read_back(&self, _study: &ResolvedStudy, _tree: &CaseTree) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn export(
        &self,
        _study: &ResolvedStudy,
        tree: &CaseTree,
        _hash: &str,
        _options: &CompileOptions,
    ) -> CaseTree {
        tree.clone()
    }
    fn run_plan(&self, _study: &ResolvedStudy, _np: u32) -> RunPlan {
        RunPlan {
            solver: "fake".into(),
            steps: vec![RunStep {
                command: "fake input.txt".into(),
                log: "log.fake".into(),
            }],
            solver_log: "log.fake".into(),
            judged_logs: vec!["log.fake".into()],
            final_fields: vec!["out.txt".into()],
            transient: false,
            buoyant: false,
            energy_field: "T".to_owned(),
            pressure_field: String::new(),
            residual_checks: vec![("rho".into(), LimitKind::Pressure)],
            potential: false,
        }
    }
    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &SOURCES
    }
    fn convergence(
        &self,
        _files: &dyn RunFiles,
        _study: &ResolvedStudy,
    ) -> Option<ConvergenceOutcome> {
        self.verdict.map(|(converged, reason)| ConvergenceOutcome {
            converged,
            reasons: vec![reason.to_owned()],
        })
    }
}

static STALLED: Fake = Fake {
    verdict: Some((false, "the fake solver stalled")),
};
static SETTLED: Fake = Fake {
    verdict: Some((true, "")),
};
static SILENT: Fake = Fake { verdict: None };

fn cavity(tag: &str) -> PathBuf {
    new_document(&scratch(tag), "cavity", CAVITY)
}

fn queue_bytes(doc: &Path) -> Option<Vec<u8>> {
    fs::read(store_dir(doc).join("queue.json")).ok()
}

fn on(backend: Option<&str>) -> AddRequest {
    AddRequest {
        backend: backend.map(str::to_owned),
        ..AddRequest::default()
    }
}

#[test]
fn queue_add_with_a_backend_stores_it() {
    // A registered backend other than the default is stored on the job.
    let doc = cavity("qb-named");
    passed(&queue_add(&doc, &on(Some("esi-v2606"))).report);
    let queue = QueueFile::new(&doc).read().unwrap();
    assert_eq!(queue.jobs.len(), 1);
    assert_eq!(queue.jobs[0].backend.as_deref(), Some("esi-v2606"));

    // An id no backend has is refused with its code and nothing is stored.
    let doc = cavity("qb-unknown");
    let out = queue_add(&doc, &on(Some("nope")));
    assert_eq!(codes(&out.report), ["CFD-BACKEND-001"]);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(queue_bytes(&doc), None, "nothing is written");

    // Naming none stores none, and the registry default applies when the job starts.
    let doc = cavity("qb-none");
    passed(&queue_add(&doc, &on(None)).report);
    let queue = QueueFile::new(&doc).read().unwrap();
    assert_eq!(queue.jobs[0].backend, None);
    let text = String::from_utf8(queue_bytes(&doc).unwrap()).unwrap();
    assert!(!text.contains("backend"), "{text}");
}

#[test]
fn the_same_study_for_two_backends_is_two_jobs() {
    let doc = cavity("qb-two");
    passed(&queue_add(&doc, &on(None)).report);
    passed(&queue_add(&doc, &on(Some("esi-v2606"))).report);
    let queue = QueueFile::new(&doc).read().unwrap();
    let named: Vec<Option<&str>> = queue.jobs.iter().map(|j| j.backend.as_deref()).collect();
    assert_eq!(named, [None, Some("esi-v2606")]);
}

#[test]
fn a_job_runs_on_the_backend_it_names() {
    let doc = cavity("qb-plan");
    let options = JobOptions::default();
    let plan = |backend: Option<&str>| {
        plan_scenario_job_on(&doc, None, JobAction::Solve, &options, backend)
            .unwrap_or_else(|_| panic!("the study plans on {backend:?}"))
    };

    let default = plan(None);
    assert_eq!(default.backend_id(), default_row().backend.id());
    assert_eq!(
        default.image().unwrap(),
        default_row().backend.image().unwrap()
    );

    let named = plan(Some("esi-v2606"));
    assert_eq!(named.backend_id(), "esi-v2606");
    let row = rows()
        .into_iter()
        .find(|row| row.backend.id() == "esi-v2606")
        .expect("the second dialect is registered");
    assert_eq!(named.image().unwrap(), row.backend.image().unwrap());
    // The tool identity, and with it every stage key, carries the backend.
    assert_eq!(named.tool().dialect, "esi-v2606");
    assert_eq!(default.tool().dialect, default_row().backend.id());
    assert_ne!(named.tool(), default.tool());

    // An id that no longer exists is refused with its code, before the document is read.
    let Err(outcome) = plan_scenario_job_on(&doc, None, JobAction::Solve, &options, Some("gone-1"))
    else {
        panic!("an unknown backend must not plan");
    };
    assert_eq!(codes(&outcome.report), ["CFD-BACKEND-001"]);
}

#[test]
fn a_backend_verdict_is_asked_first() {
    let dir = scratch("qb-verdict");
    let resolved = plan_studies::resolved(CAVITY);
    let spec = spec_for(&resolved);
    let inputs = (spec, Settings::default());

    let stalled = verdict_for(&STALLED, &dir, &inputs, &resolved);
    assert_eq!(stalled.state, VerdictState::NotConverged);
    assert_eq!(stalled.reasons, ["the fake solver stalled"]);

    let settled = verdict_for(&SETTLED, &dir, &inputs, &resolved);
    assert_eq!(settled.state, VerdictState::Converged);

    // A backend with no verdict of its own leaves it to the OpenFOAM assessment.
    let generic = verdict_for(&SILENT, &dir, &inputs, &resolved);
    assert_eq!(generic, assess_dir(&dir, &inputs.0, &inputs.1).verdict);
    assert_ne!(generic.state, VerdictState::Converged);
}

/// A request whose host file and run folder do not exist, so any step past the gates shows.
fn request(tag: &str) -> RunRequest {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("queue-backend-{tag}"));
    RunRequest::new(
        RunMode::Run,
        base.join("key"),
        base.join("hosts.toml"),
        "none",
        base.join("runs"),
        base.join("sock"),
    )
}

#[test]
fn run_request_can_carry_a_backend() {
    // An unknown id is refused before any file is made.
    let unknown = request("unknown").with_backend(Some("nope".to_owned()));
    let Err(done) = begin(CAVITY, &unknown) else {
        panic!("an unknown backend must not begin a run");
    };
    let found: Vec<&str> = done
        .report
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(found, ["CFD-BACKEND-001"]);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    assert!(!unknown.runs_dir.exists());

    // A registered id goes on to the host, which is missing here, and a request that names none
    // behaves as it did.
    for backend in [Some("esi-v2606".to_owned()), None] {
        let request = request("known").with_backend(backend);
        let Err(done) = begin(CAVITY, &request) else {
            panic!("the host profile does not exist");
        };
        assert_eq!(done.report.class(), ExitClass::Infrastructure);
        assert!(
            done.report
                .diagnostics
                .iter()
                .all(|d| !d.code.starts_with("CFD-BACKEND-001"))
        );
    }
}
