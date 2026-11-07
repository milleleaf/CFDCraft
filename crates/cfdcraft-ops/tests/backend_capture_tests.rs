//! A run of a backend that is not an OpenFOAM line is captured and recorded from the files its run
//! plan names, its benchmark numbers are read through the backend's own view of its files, and the
//! plans of the OpenFOAM lines are what they were. No test needs a compute host or a container
//! engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_backend::{
    Backend, CaseFile, CaseTree, CompileFailure, CompileOptions, ConvergenceOutcome, Family,
    Refusal, RunFiles, RunPlan, RunStep,
};
use cfdcraft_capture::{CapturePlan, POLY_MESH_FILES, entry_names};
use cfdcraft_doc::{open, store_dir};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy};
use cfdcraft_ops::backends::{Admission, Row, by_id, default_row};
use cfdcraft_ops::convergence::{spec_for, spec_on};
use cfdcraft_ops::jobs::{CaseRun, JobAction, capture_plan_for, capture_plan_on, plan_job_on_row};
use cfdcraft_runs::Check;
use cfdcraft_settle::VerdictState;
use cfdcraft_store::Store;

#[path = "plan_studies/mod.rs"]
mod plan_studies;
#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::{new_document, passed, scratch};

const CAVITY: &str = plan_studies::CAVITY;
const SOURCES: [(&str, &str); 1] = [("writer.rs", "fn write() {}")];
const END: &str = "5000";

/// A backend of a family that is not an OpenFOAM line, with the files its plan names set by the
/// test.
struct Double {
    id: &'static str,
    /// The judged logs; the first is the solver log.
    logs: &'static [&'static str],
    /// The final result files.
    fields: &'static [&'static str],
    /// The files that stand for the mesh.
    mesh: &'static [&'static str],
    /// Whether the run plan changes with the process count.
    parallel: bool,
    /// Whether the result view serves the two cavity centreline files from a ramp.
    view: bool,
}

static PLAIN: Double = Double {
    id: "double-plain",
    logs: &["solver.log", "other.log"],
    fields: &["result.dat"],
    mesh: &["mesh.dat"],
    parallel: false,
    view: false,
};
static VIEWED: Double = Double {
    id: "double-viewed",
    logs: &["solver.log"],
    fields: &["result.dat"],
    mesh: &["mesh.dat"],
    parallel: false,
    view: true,
};
static MANY: Double = Double {
    id: "double-many",
    logs: &["solver.log", "other.log"],
    fields: &["result.dat", "history.dat"],
    mesh: &[],
    parallel: true,
    view: false,
};

impl Backend for Double {
    fn id(&self) -> &'static str {
        self.id
    }
    fn family(&self) -> Family {
        Family::Lbm
    }
    fn image(&self) -> Option<&'static str> {
        Some("example/double@sha256:abc")
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
    fn run_plan(&self, _study: &ResolvedStudy, np: u32) -> RunPlan {
        let step = |command: &str| RunStep {
            command: command.to_owned(),
            log: self.logs[0].to_owned(),
        };
        let steps = if self.parallel && np > 1 {
            vec![step("double --split"), step("double --parallel")]
        } else {
            vec![step("double input.txt")]
        };
        RunPlan {
            solver: "double".to_owned(),
            steps,
            solver_log: self.logs[0].to_owned(),
            judged_logs: self.logs.iter().map(|name| (*name).to_owned()).collect(),
            final_fields: self.fields.iter().map(|name| (*name).to_owned()).collect(),
            transient: false,
            buoyant: false,
            energy_field: "T".to_owned(),
            pressure_field: String::new(),
            residual_checks: Vec::new(),
            potential: false,
        }
    }
    fn mesh_files(&self, _study: &ResolvedStudy) -> Vec<String> {
        self.mesh.iter().map(|name| (*name).to_owned()).collect()
    }
    fn convergence(
        &self,
        _files: &dyn RunFiles,
        _study: &ResolvedStudy,
    ) -> Option<ConvergenceOutcome> {
        Some(ConvergenceOutcome {
            converged: true,
            reasons: Vec::new(),
        })
    }
    fn results(&self, files: Box<dyn ResultFiles>, _study: &ResolvedStudy) -> Box<dyn ResultFiles> {
        if self.view {
            Box::new(RampView { inner: files })
        } else {
            files
        }
    }
    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &SOURCES
    }
}

const VERTICAL: &str = "postProcessing/centreline/5000/vertical_U.xy";
const HORIZONTAL: &str = "postProcessing/centreline/5000/horizontal_U.xy";

/// The two centreline files the cavity comparer reads, from a ramp: 64 samples from wall to wall,
/// `0.01 * index` for the streamwise velocity, so the middle is 0.315.
fn ramp(column: usize) -> String {
    let mut text = String::new();
    for index in 0..64 {
        let coordinate = f64::from(index) * 0.1 / 63.0;
        let value = f64::from(index) * 0.01;
        let mut row = [0.0; 3];
        row[column - 1] = value;
        text.push_str(&format!(
            "{coordinate:.12} {:.12} {:.12} {:.12}\n",
            row[0], row[1], row[2]
        ));
    }
    text
}

/// A view that serves the centreline files the native folder does not hold.
struct RampView {
    inner: Box<dyn ResultFiles>,
}

impl ResultFiles for RampView {
    fn read(&self, rel: &str) -> Option<String> {
        match rel {
            VERTICAL => Some(ramp(1)),
            HORIZONTAL => Some(ramp(2)),
            _ => self.inner.read(rel),
        }
    }
    fn list(&self, rel_dir: &str) -> Vec<String> {
        match rel_dir {
            "postProcessing/centreline" => vec![END.to_owned()],
            "postProcessing/centreline/5000" => {
                vec!["horizontal_U.xy".to_owned(), "vertical_U.xy".to_owned()]
            }
            _ => self.inner.list(rel_dir),
        }
    }
}

fn row_of(backend: &'static Double) -> Row {
    Row {
        backend,
        admission: Admission::Candidate,
    }
}

fn put(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The folder a good run of `backend` leaves: every file its plan names, and one more.
fn results_of(backend: &Double, tag: &str) -> PathBuf {
    let dir = scratch(&format!("{tag}-results"));
    for name in backend.logs.iter().chain(backend.fields) {
        put(&dir, name, "text\n");
    }
    for name in backend.mesh {
        put(&dir, name, "mesh\n");
    }
    put(&dir, "unrelated.txt", "not kept\n");
    dir
}

fn run_in(dir: PathBuf) -> CaseRun {
    CaseRun {
        results_dir: dir,
        checks: vec![Check {
            name: "evidence".to_owned(),
            passed: true,
            detail: "passed".to_owned(),
        }],
        host: None,
        np: None,
        stopped_at: None,
    }
}

fn blob_names(doc: &Path, blob: cfdcraft_store::ContentHash) -> Vec<String> {
    let store = Store::open(store_dir(doc)).unwrap();
    entry_names(&store.read(blob).unwrap()).unwrap()
}

#[test]
fn the_openfoam_plans_are_unchanged() {
    let r = plan_studies::resolved(CAVITY);
    assert_eq!(capture_plan_for(&r), CapturePlan::cavity());

    let default = default_row();
    assert_eq!(capture_plan_on(&r, &default), capture_plan_for(&r));
    assert_eq!(spec_on(&r, default.backend), spec_for(&r));

    let of14 = by_id("of14-20261008").expect("the second OpenFOAM line is registered");
    let plan = capture_plan_on(&r, &of14);
    assert!(plan.time_folder);
    assert_eq!(plan.log, of14.backend.run_plan(&r, 1).solver_log);
    assert_eq!(plan.mesh, POLY_MESH_FILES);

    assert_eq!(default.backend.mesh_files(&r), POLY_MESH_FILES);
    assert_eq!(cfdcraft_backend::backend::POLY_MESH_FILES, POLY_MESH_FILES);
}

#[test]
fn a_run_of_another_backend_is_recorded_from_its_plan() {
    let doc = new_document(&scratch("record-doc"), "cavity", CAVITY);
    let job = plan_job_on_row(&doc, JobAction::Solve, &row_of(&PLAIN)).unwrap_or_else(|_| {
        panic!("the study plans on the double");
    });
    let done = job.finish(run_in(results_of(&PLAIN, "record")));
    passed(&done.report);

    let runs = open(&doc).unwrap().document.artifacts.runs;
    assert_eq!(runs.len(), 1);
    let record = &runs[0];
    let result = record.result.as_ref().expect("the result is recorded");
    assert_eq!(
        blob_names(&doc, result.blob),
        ["other.log", "result.dat", "solver.log"]
    );
    let mesh = record.mesh.as_ref().expect("the mesh is recorded");
    assert_eq!(blob_names(&doc, mesh.blob), ["mesh.dat"]);
    assert!(record.key_numbers.is_empty(), "{:?}", record.key_numbers);
    let verdict = record.verdict.as_ref().expect("a verdict is recorded");
    assert_eq!(verdict.state, VerdictState::Converged);
    assert_eq!(record.provenance.tool.dialect, "double-plain");
}

#[test]
fn a_missing_file_the_plan_names_refuses_the_record_and_names_it() {
    let doc = new_document(&scratch("missing-doc"), "cavity", CAVITY);
    let job = plan_job_on_row(&doc, JobAction::Solve, &row_of(&PLAIN)).unwrap_or_else(|_| {
        panic!("the study plans on the double");
    });
    let results = results_of(&PLAIN, "missing");
    fs::remove_file(results.join("result.dat")).unwrap();
    let done = job.finish(run_in(results));

    let found = done
        .report
        .diagnostics
        .iter()
        .find(|d| d.code == "CFD-REPRO-009")
        .expect("the reason is told");
    assert!(found.message.contains("result.dat"), "{}", found.message);
    assert!(open(&doc).unwrap().document.artifacts.runs.is_empty());
}

#[test]
fn the_benchmark_numbers_come_through_the_backends_view() {
    let doc = new_document(&scratch("view-doc"), "cavity", CAVITY);
    let job = plan_job_on_row(&doc, JobAction::Solve, &row_of(&VIEWED)).unwrap_or_else(|_| {
        panic!("the study plans on the double");
    });
    let results = results_of(&VIEWED, "view");
    assert!(!results.join("postProcessing").exists());
    passed(&job.finish(run_in(results)).report);

    let runs = open(&doc).unwrap().document.artifacts.runs;
    let centre = runs[0].key_numbers["bench.ghia.u_centre"];
    assert!((centre - 3.15).abs() < 1e-6, "{centre}");
}

#[test]
fn the_spec_is_made_from_the_run_plan() {
    let r = plan_studies::resolved(CAVITY);
    let spec = spec_on(&r, &PLAIN);
    assert_eq!(spec.log_file, "solver.log");
    assert_eq!(spec.pressure_field, "");
    assert_eq!(spec.residual_limits, None);
    assert_eq!(spec_on(&r, default_row().backend), spec_for(&r));
}

#[test]
fn the_process_count_and_the_mirrored_files_follow_the_run_plan() {
    let doc = new_document(&scratch("np-doc"), "cavity", CAVITY);
    let plan = |row: &Row| {
        plan_job_on_row(&doc, JobAction::Solve, row).unwrap_or_else(|_| {
            panic!("the study plans on {}", row.backend.id());
        })
    };

    let default = plan(&default_row());
    assert_eq!(default.effective_np(4), 4);
    assert_eq!(default.effective_np(1), 1);
    assert!(default.mirrored_files().is_empty());

    let plain = plan(&row_of(&PLAIN));
    assert_eq!(plain.effective_np(4), 1);

    let many = plan(&row_of(&MANY));
    assert_eq!(many.effective_np(4), 4);
    assert_eq!(many.effective_np(1), 1);
    assert_eq!(
        many.mirrored_files(),
        ["solver.log", "other.log", "result.dat", "history.dat"]
    );
}
