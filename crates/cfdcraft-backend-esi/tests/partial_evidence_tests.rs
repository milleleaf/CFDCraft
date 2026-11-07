use cfdcraft_backend_esi::{
    EvidenceExpectations, RunEvidence, Verdict,
    evidence::{PartialExpectations, judge_partial},
    judge, judge_run,
};

const GOOD: &str = include_str!("fixtures/cavity-good.log");
const BLOCK: &str = "Created 16384 cells\nEnd\n";
const MESH: &str = "cells: 16384\nTotal volume = 0.0005\nMesh OK.\nEnd\n";

fn expected() -> PartialExpectations {
    PartialExpectations {
        end_time: 2000.0,
        cell_count: 16384,
        domain_volume: 0.0005,
    }
}
fn stopped_log(time: &str) -> String {
    GOOD.replace("Time = 2000", &format!("Time = {time}"))
}
fn partial(solver: &str, dirs: &[f64]) -> Verdict {
    judge_partial(
        &RunEvidence {
            block_mesh_log: BLOCK,
            check_mesh_log: MESH,
            solver_log: solver,
            time_directories: dirs,
        },
        &expected(),
    )
}
fn failure(v: Verdict) -> Vec<String> {
    match v {
        Verdict::Passed => panic!("should fail"),
        Verdict::Failed(r) => r,
    }
}

#[test]
fn a_log_ending_with_the_parallel_trailer_still_ends_with_end() {
    let trailer = format!("{GOOD}\nFinalising parallel run\n");
    assert_eq!(judge(&trailer, 2000.0, Some(16384)), Verdict::Passed);
    for other in [
        format!("{GOOD}\nsomething else\n"),
        format!("{GOOD}\nFinalising parallel run\nFinalising parallel run\n"),
        format!("{GOOD}\nFinalising parallel run\nsomething else\n"),
        "Finalising parallel run\n".to_string(),
    ] {
        let reasons = failure(judge(&other, 2000.0, Some(16384)));
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("missing terminal End marker")),
            "{reasons:?}"
        );
    }
}

#[test]
fn a_write_now_stop_is_a_pass_when_the_evidence_agrees() {
    assert_eq!(
        partial(&stopped_log("1200"), &[0.0, 1000.0, 1200.0]),
        Verdict::Passed
    );
    let trailer = format!("{}\nFinalising parallel run\n", stopped_log("1200"));
    assert_eq!(partial(&trailer, &[0.0, 1200.0]), Verdict::Passed);
}

#[test]
fn the_last_logged_time_must_be_the_newest_folder() {
    let reasons = failure(partial(&stopped_log("1200"), &[0.0, 1000.0]));
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("1200") && r.contains("1000")),
        "{reasons:?}"
    );
}

#[test]
fn a_stop_at_time_zero_or_after_the_end_time_fails() {
    assert!(!partial(&stopped_log("0"), &[0.0]).passed());
    assert!(!partial(&stopped_log("2500"), &[0.0, 2500.0]).passed());
    assert!(!partial("End\n", &[0.0]).passed());
}

#[test]
fn a_stopped_run_with_a_trap_fails() {
    let trap = include_str!("fixtures/of14-windaround-buildings.log");
    let reasons = failure(partial(trap, &[0.0, 1.0]));
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("Floating point exception"))
    );
}

#[test]
fn a_missing_end_marker_fails() {
    let cut = stopped_log("1200").replace("End\n", "");
    let reasons = failure(partial(&cut, &[0.0, 1200.0]));
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("missing terminal End marker"))
    );
}

#[test]
fn the_full_judge_is_unchanged() {
    let full = EvidenceExpectations {
        end_time: 2000.0,
        cell_count: 16384,
        domain_volume: 0.0005,
        p_threshold: 1e-6,
        u_threshold: 1e-6,
    };
    let run = |solver: &str, dirs: &[f64]| {
        judge_run(
            &RunEvidence {
                block_mesh_log: BLOCK,
                check_mesh_log: MESH,
                solver_log: solver,
                time_directories: dirs,
            },
            &full,
        )
    };
    assert_eq!(run(GOOD, &[0.0, 500.0, 2000.0]), Verdict::Passed);
    let reasons = failure(judge_run(
        &RunEvidence {
            block_mesh_log: include_str!("fixtures/v2512-blockMesh.log"),
            check_mesh_log: include_str!("fixtures/v2512-checkMesh.log"),
            solver_log: include_str!("fixtures/v2512-simpleFoam-final-iteration.log"),
            time_directories: &[0.0, 2000.0],
        },
        &full,
    ));
    assert_eq!(reasons.len(), 3, "{reasons:?}");
    assert!(reasons.iter().all(|r| r.contains("initial residual")));
    assert!(!run(&stopped_log("1200"), &[0.0, 1200.0]).passed());
}
