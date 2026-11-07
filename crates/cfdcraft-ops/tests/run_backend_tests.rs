//! A run takes its backend from a registry row, and a row without a pinned image never reaches
//! the compute host.

use cfdcraft_backend::{
    Backend, CaseFile, CaseTree, CompileFailure, CompileOptions, Family, LimitKind, Refusal,
    RunPlan, RunStep,
};
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy};
use cfdcraft_ops::{
    ExitClass, RunMode, RunRequest,
    backends::{Admission, Row},
    run::begin_row,
};
use std::path::PathBuf;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

const SOURCES: [(&str, &str); 1] = [("writer.rs", "fn write() {}")];

/// A backend with fixed small values and an image that is pinned or not.
struct Fake {
    image: Option<&'static str>,
}

impl Backend for Fake {
    fn id(&self) -> &'static str {
        "fake-1"
    }
    fn family(&self) -> Family {
        Family::Lbm
    }
    fn image(&self) -> Option<&'static str> {
        self.image
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
}

static NO_IMAGE: Fake = Fake { image: None };
static PINNED: Fake = Fake {
    image: Some("example/fake@sha256:abc"),
};

/// A request whose host file and run folder do not exist, so any step past the gates shows.
fn request(tag: &str) -> RunRequest {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("run-backend-{tag}"));
    RunRequest::new(
        RunMode::Run,
        base.join("key"),
        base.join("hosts.toml"),
        "none",
        base.join("runs"),
        base.join("sock"),
    )
}

fn row_of(backend: &'static dyn Backend) -> Row {
    Row {
        backend,
        admission: Admission::Supported,
    }
}

#[test]
fn run_refuses_an_unpinned_image_before_upload() {
    let request = request("unpinned");
    let Err(done) = begin_row(CAVITY, &request, &row_of(&NO_IMAGE)) else {
        panic!("a row without an image must not begin a run");
    };
    let codes: Vec<&str> = done
        .report
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(codes, ["CFD-BACKEND-010"]);
    assert_eq!(done.report.class(), ExitClass::StudyInvalid);
    // Nothing reached the host: no infrastructure finding, no run, and no run folder.
    assert!(done.report.infra.is_none());
    assert!(done.report.run.is_none());
    assert!(!request.runs_dir.exists());
}

#[test]
fn a_pinned_row_goes_on_to_the_host() {
    let request = request("pinned");
    let Err(done) = begin_row(CAVITY, &request, &row_of(&PINNED)) else {
        panic!("the host profile does not exist");
    };
    assert_eq!(done.report.class(), ExitClass::Infrastructure);
    assert!(done.report.infra.is_some());
    assert!(
        done.report
            .diagnostics
            .iter()
            .all(|d| d.code != "CFD-BACKEND-010")
    );
}
