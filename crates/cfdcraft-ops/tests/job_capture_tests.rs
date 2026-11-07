//! A recorded run is captured by the solver its study asks for, and a run of a benchmark study
//! records the benchmark's own quantities among its key numbers. No test needs a compute host or a
//! container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_capture::fixture::{FixtureSpec, write_results, write_results_for};
use cfdcraft_capture::{
    CapturePlan, checkpoint_bundle_with, key_numbers, key_numbers_with, result_bundle,
    result_bundle_with,
};
use cfdcraft_doc::open;
use cfdcraft_ops::jobs::capture_plan_for;
use cfdcraft_ops::{CaseRun, CaseRunner, Report, solve_document};
use cfdcraft_runs::Check;

#[path = "plan_studies/mod.rs"]
mod plan_studies;
use plan_studies::*;
#[path = "repro_support/mod.rs"]
mod repro_support;

const END: u64 = 5000;

fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("job-capture-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn spec(end_time: u64) -> FixtureSpec {
    FixtureSpec {
        end_time,
        cells: 16384,
        samples: 64,
        shift: 0.0,
    }
}

/// A runner that leaves the folder of a good cavity run, with centreline samples that run from
/// wall to wall as a real sampled line does, so the benchmark can read them. The ramp is
/// `0.01 * index` for the streamwise velocity, 64 samples, so the middle (index 31.5) is 0.315.
struct CavityRunner {
    root: PathBuf,
    folders: Vec<PathBuf>,
}

impl CaseRunner for CavityRunner {
    fn run_case(&mut self, _study_json: &str) -> Result<CaseRun, Box<Report>> {
        let dir = self.root.join(format!("call-{}", self.folders.len() + 1));
        write_results(&dir, &spec(END)).unwrap();
        let samples = dir.join(format!("postProcessing/centreline/{END}"));
        let mut vertical = String::new();
        let mut horizontal = String::new();
        for index in 0..64 {
            let coordinate = f64::from(index) * 0.1 / 63.0;
            let value = f64::from(index) * 0.01;
            vertical.push_str(&format!(
                "{coordinate:.12} {value:.12} 0.000000000000 0.000000000000\n"
            ));
            horizontal.push_str(&format!(
                "{coordinate:.12} 0.000000000000 {value:.12} 0.000000000000\n"
            ));
        }
        fs::write(samples.join("vertical_U.xy"), vertical).unwrap();
        fs::write(samples.join("horizontal_U.xy"), horizontal).unwrap();
        self.folders.push(dir.clone());
        Ok(CaseRun {
            results_dir: dir,
            checks: vec![Check {
                name: "evidence".to_owned(),
                passed: true,
                detail: "passed".to_owned(),
            }],
            host: Some("fixture-host".to_owned()),
            np: None,
            stopped_at: None,
        })
    }
}

#[test]
fn the_cavity_capture_plan_is_the_old_behaviour() {
    assert_eq!(capture_plan_for(&resolved(CAVITY)), CapturePlan::cavity());
}

#[test]
fn a_cavity_run_records_the_same_numbers_plus_the_benchmark_one() {
    let dir = scratch("cavity");
    let path = repro_support::new_document(&dir, "cavity", CAVITY);
    let mut runner = CavityRunner {
        root: dir.join("runs"),
        folders: Vec::new(),
    };
    repro_support::passed(&solve_document(&path, &mut runner).report);

    let folder = &runner.folders[0];
    let runs = open(&path).unwrap().document.artifacts.runs;
    let record = &runs[0];

    // Every number the old capture made is there with the same value.
    let old = key_numbers(folder, END as f64).unwrap();
    assert!(old.contains_key("solver.iterations"));
    assert!(old.contains_key("residual.p"));
    assert!(old.contains_key("centreline.vertical.ux.0000"));
    for (name, value) in &old {
        assert_eq!(record.key_numbers.get(name), Some(value), "{name}");
    }
    // The benchmark adds its own, in lid velocities: 0.315 m/s over a 0.1 m/s lid.
    assert_eq!(record.key_numbers.len(), old.len() + 1);
    let centre = record.key_numbers["bench.ghia.u_centre"];
    assert!((centre - 3.15).abs() < 1e-6, "{centre}");

    // The stored result is the bundle the old capture makes.
    let stored = record.result.as_ref().expect("the result is recorded");
    let bundle = result_bundle(folder, END as f64).unwrap();
    assert_eq!(bundle.hash().unwrap(), stored.blob);
}

#[test]
fn a_study_that_is_no_benchmark_adds_no_bench_numbers() {
    let dir = scratch("no-benchmark");
    // A lid twice as fast is Reynolds number 200: not the benchmark.
    let path = repro_support::new_document(&dir, "fast", &repro_support::with_lid_speed(0.2));
    let mut runner = CavityRunner {
        root: dir.join("runs"),
        folders: Vec::new(),
    };
    repro_support::passed(&solve_document(&path, &mut runner).report);

    let runs = open(&path).unwrap().document.artifacts.runs;
    let numbers = &runs[0].key_numbers;
    assert!(numbers.contains_key("solver.iterations"));
    assert!(numbers.keys().all(|name| !name.starts_with("bench.")));
}

/// A plan's folder for `r`, written with the fixture writer, at `end_time`.
fn folder_for(tag: &str, plan: &CapturePlan, end_time: u64) -> PathBuf {
    let dir = scratch(tag);
    write_results_for(&dir, &spec(end_time), plan).unwrap();
    dir
}

#[test]
fn a_transient_run_is_captured_by_its_plan() {
    let r = resolved(&cylinder_json(&cylinder_monitors(
        "cylinder_forces",
        "front",
        "rear",
    )));
    let plan = capture_plan_for(&r);
    assert_eq!(plan.log, "log.pimpleFoam");
    assert_eq!(plan.fields, ["U", "p"]);
    assert!(
        !plan.centreline,
        "a channel with a cylinder has no sampled centrelines"
    );
    assert_eq!(plan.residual_fields, ["Ux", "Uy", "p"]);

    let dir = folder_for("transient", &plan, 1);
    let monitor = dir.join("postProcessing/force_1/0");
    fs::create_dir_all(&monitor).unwrap();
    fs::write(monitor.join("coefficient.dat"), "0.5 0 3.2 0.9 0.45 0.45\n").unwrap();

    let bundle = result_bundle_with(&dir, 1.0, &plan).unwrap();
    let names = bundle.names();
    for wanted in [
        "1/U",
        "1/p",
        "log.pimpleFoam",
        "postProcessing/force_1/0/coefficient.dat",
    ] {
        assert!(names.contains(&wanted), "{wanted} in {names:?}");
    }
    assert!(!names.contains(&"log.simpleFoam"));

    let numbers = key_numbers_with(&dir, 1.0, &plan).unwrap();
    for wanted in [
        "solver.iterations",
        "residual.Ux",
        "residual.Uy",
        "residual.p",
    ] {
        assert!(numbers.contains_key(wanted), "{wanted}");
    }
    assert!(numbers.keys().all(|name| !name.starts_with("centreline.")));
}

#[test]
fn a_buoyant_run_is_captured_with_its_fields() {
    let r = resolved(&heated_json(""));
    let plan = capture_plan_for(&r);
    assert_eq!(plan.log, "log.buoyantBoussinesqSimpleFoam");
    assert_eq!(plan.fields, ["U", "p_rgh", "T", "p"]);
    assert!(plan.centreline, "a box keeps its sampled centrelines");
    assert_eq!(plan.residual_fields, ["Ux", "Uy", "p_rgh", "T"]);

    let dir = folder_for("buoyant", &plan, END);
    let bundle = result_bundle_with(&dir, END as f64, &plan).unwrap();
    let names = bundle.names();
    for field in ["U", "p_rgh", "T", "p"] {
        let wanted = format!("{END}/{field}");
        assert!(names.contains(&wanted.as_str()), "{wanted} in {names:?}");
    }
    let numbers = key_numbers_with(&dir, END as f64, &plan).unwrap();
    assert!(numbers.contains_key("residual.p_rgh"));
    assert!(numbers.contains_key("residual.T"));
}

#[test]
fn a_stopped_run_is_checkpointed_by_its_plan() {
    let r = resolved(&cylinder_json(""));
    let plan = capture_plan_for(&r);
    let dir = folder_for("stopped", &plan, 1);
    let bundle = checkpoint_bundle_with(&dir, "1", &plan).unwrap();
    let names = bundle.names();
    for wanted in [
        "1/U",
        "1/p",
        "log.pimpleFoam",
        "log.blockMesh",
        "log.checkMesh",
    ] {
        assert!(names.contains(&wanted), "{wanted} in {names:?}");
    }
    assert!(!names.contains(&"log.simpleFoam"));
}
