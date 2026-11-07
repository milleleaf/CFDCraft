//! The judge over the logs a real solver wrote for the cylinder and the turbulent step.
//!
//! The logs are the output of sixty iterations (or sixty time steps) at the coarse mesh in the
//! pinned image; see `tests/fixtures/PROVENANCE.md`.

use cfdcraft_backend_esi::{
    EvidencePlan, RunEvidence, SmokeExpectations, Verdict, judge_smoke_with,
};

const CYLINDER_BLOCK: &str = include_str!("fixtures/v2512-real-cylinder-blockMesh.log");
const CYLINDER_MESH: &str = include_str!("fixtures/v2512-real-cylinder-checkMesh.log");
const CYLINDER_SOLVER: &str =
    include_str!("../../cfdcraft-logscan/tests/fixtures/v2512-real-cylinder-pimpleFoam.log");
const STEP_BLOCK: &str = include_str!("fixtures/v2512-real-step-blockMesh.log");
const STEP_MESH: &str = include_str!("fixtures/v2512-real-step-checkMesh.log");
const STEP_SOLVER: &str =
    include_str!("../../cfdcraft-logscan/tests/fixtures/v2512-real-step-simpleFoam.log");
const STEP_POTENTIAL: &str =
    include_str!("../../cfdcraft-logscan/tests/fixtures/v2512-real-step-potentialFoam.log");
const STEP_FATAL: &str =
    include_str!("../../cfdcraft-logscan/tests/fixtures/v2512-real-step-blockMesh-fatal.log");

/// The volume the mesh check printed for the cylinder channel, and the volume of its exact
/// outline (2.2 m by 0.41 m by 0.01 m, less a disc of 0.1 m across).
const CYLINDER_MESHED_VOLUME: f64 = 0.00894151624492;
const CYLINDER_OUTLINE_VOLUME: f64 = 0.008941460183660255;

fn transient_plan() -> EvidencePlan {
    EvidencePlan {
        solver: "pimpleFoam".into(),
        pressure: "p".into(),
        temperature: false,
        residual_limits: false,
        potential_log: None,
    }
}

fn cylinder(volume: f64) -> Verdict {
    let times = [0.0, 0.03, 0.06];
    judge_smoke_with(
        &RunEvidence {
            block_mesh_log: CYLINDER_BLOCK,
            check_mesh_log: CYLINDER_MESH,
            solver_log: CYLINDER_SOLVER,
            time_directories: &times,
        },
        &SmokeExpectations {
            end_time: 0.06,
            cell_count: 5784,
            domain_volume: volume,
        },
        &transient_plan(),
    )
}

#[test]
fn a_real_cylinder_smoke_run_passes_with_the_volume_the_mesh_check_printed() {
    assert_eq!(cylinder(CYLINDER_MESHED_VOLUME), Verdict::Passed);
}

#[test]
fn the_exact_outline_volume_is_not_what_a_mesh_of_a_circle_has() {
    // The mesh joins the points on the circle by straight chords, so its volume is larger than
    // the exact outline's by 5.6e-8 m3, six parts in a million: more than the judge allows.
    assert_eq!(
        cylinder(CYLINDER_OUTLINE_VOLUME),
        Verdict::Failed(vec![format!(
            "checkMesh: volume Some({CYLINDER_MESHED_VOLUME}) differs from expected {CYLINDER_OUTLINE_VOLUME}"
        )])
    );
}

#[test]
fn a_real_turbulent_step_run_with_a_potential_flow_start_passes() {
    // Four blocks, 20760 cells, a total volume the layout predicts to the last printed digit.
    let times = [0.0, 60.0];
    let plan = EvidencePlan {
        solver: "simpleFoam".into(),
        pressure: "p".into(),
        temperature: false,
        residual_limits: true,
        potential_log: Some(STEP_POTENTIAL.to_owned()),
    };
    let verdict = judge_smoke_with(
        &RunEvidence {
            block_mesh_log: STEP_BLOCK,
            check_mesh_log: STEP_MESH,
            solver_log: STEP_SOLVER,
            time_directories: &times,
        },
        &SmokeExpectations {
            end_time: 60.0,
            cell_count: 20760,
            domain_volume: 0.00305209067,
        },
        &plan,
    );
    assert_eq!(verdict, Verdict::Passed);
}

#[test]
fn a_real_mesh_dictionary_error_fails_the_run_at_the_mesher() {
    let times = [0.0, 60.0];
    let verdict = judge_smoke_with(
        &RunEvidence {
            block_mesh_log: STEP_FATAL,
            check_mesh_log: STEP_MESH,
            solver_log: STEP_SOLVER,
            time_directories: &times,
        },
        &SmokeExpectations {
            end_time: 60.0,
            cell_count: 20760,
            domain_volume: 0.00305209067,
        },
        &EvidencePlan::cavity(),
    );
    let Verdict::Failed(reasons) = verdict else {
        panic!("a mesher that died fails the run")
    };
    assert!(reasons.contains(&"blockMesh: missing terminal End marker".to_owned()));
    assert!(reasons.contains(&"blockMesh: fatal signal FOAM FATAL".to_owned()));
}
