use cfdcraft_backend_esi::{EvidenceExpectations, RunEvidence, Verdict, judge, judge_run};

const GOOD: &str = include_str!("fixtures/cavity-good.log");
const BLOCK: &str = "Created 16384 cells\nEnd\n";
const MESH: &str = "cells: 16384\nTotal volume = 0.0005\nMesh OK.\nEnd\n";
fn expected() -> EvidenceExpectations {
    EvidenceExpectations {
        end_time: 2000.0,
        cell_count: 16384,
        domain_volume: 0.0005,
        p_threshold: 1e-6,
        u_threshold: 1e-6,
    }
}
fn failure(v: Verdict) -> Vec<String> {
    match v {
        Verdict::Passed => panic!("should fail"),
        Verdict::Failed(r) => r,
    }
}

#[test]
fn evidence_good_logs_and_actual_output_directory_pass() {
    assert_eq!(judge(GOOD, 2000.0, Some(16384)), Verdict::Passed);
    assert_eq!(
        judge(
            &GOOD.replace(
                "Total volume = 0.0005",
                "Total volume = 0.0005. Cell volumes OK."
            ),
            2000.0,
            Some(16384)
        ),
        Verdict::Passed
    );
    assert_eq!(
        judge_run(
            &RunEvidence {
                block_mesh_log: BLOCK,
                check_mesh_log: MESH,
                solver_log: GOOD,
                time_directories: &[0.0, 500.0, 2000.0]
            },
            &expected()
        ),
        Verdict::Passed
    );
}

#[test]
fn evidence_actual_v2512_trap_banner_and_mesh_punctuation_are_not_failures() {
    let good = GOOD.replace(
        "sigFpe : Enabling floating point exception trapping (FOAM_SIGFPE).",
        "trapFpe: Floating point exception trapping enabled (FOAM_SIGFPE).",
    );
    assert_eq!(
        judge_run(
            &RunEvidence {
                block_mesh_log: include_str!("fixtures/v2512-blockMesh.log"),
                check_mesh_log: include_str!("fixtures/v2512-checkMesh.log"),
                solver_log: &good,
                time_directories: &[0.0, 2000.0],
            },
            &expected()
        ),
        Verdict::Passed
    );
}

#[test]
fn evidence_actual_nonconverged_v2512_solver_still_fails() {
    let r = failure(judge_run(
        &RunEvidence {
            block_mesh_log: include_str!("fixtures/v2512-blockMesh.log"),
            check_mesh_log: include_str!("fixtures/v2512-checkMesh.log"),
            solver_log: include_str!("fixtures/v2512-simpleFoam-final-iteration.log"),
            time_directories: &[0.0, 2000.0],
        },
        &expected(),
    ));
    assert_eq!(
        r.len(),
        3,
        "only the three p/U convergence checks should fail: {r:?}"
    );
    assert!(r.iter().all(|r| r.contains("initial residual")));
}

#[test]
fn evidence_recorded_exit_zero_crash_is_not_success() {
    let crash = include_str!("fixtures/of14-windaround-buildings.log");
    let mesh = include_str!("fixtures/of14-snappyHexMesh.log");
    let solver = include_str!("fixtures/of14-foamRun.log");
    assert_eq!(crash.len(), 2321);
    assert_eq!(mesh.len(), 1786);
    assert_eq!(solver.len(), 319330);
    let reasons = failure(judge_run(
        &RunEvidence {
            block_mesh_log: crash,
            check_mesh_log: mesh,
            solver_log: solver,
            time_directories: &[0.0, 1.0],
        },
        &expected(),
    ));
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("SIGFPE") || r.contains("Floating point exception"))
    );
    assert!(reasons.iter().any(|r| r.contains("FOAM FATAL")));
}

#[test]
fn evidence_requires_every_step_to_complete_and_expected_output_directory() {
    let r = failure(judge_run(
        &RunEvidence {
            block_mesh_log: "End\ntruncated after End",
            check_mesh_log: MESH,
            solver_log: GOOD,
            time_directories: &[0.0, 1500.0],
        },
        &expected(),
    ));
    assert!(r.iter().any(|r| r.contains("blockMesh")));
    assert!(r.iter().any(|r| r.contains("time directory")));
    let r = failure(judge_run(
        &RunEvidence {
            block_mesh_log: BLOCK,
            check_mesh_log: MESH,
            solver_log: GOOD,
            time_directories: &[2000.0, 2500.0],
        },
        &expected(),
    ));
    assert!(r.iter().any(|r| r.contains("time directory")));
}

#[test]
fn evidence_does_not_confuse_linear_solver_final_residual_with_convergence() {
    let bad = GOOD.replace("Initial residual = 1e-8", "Initial residual = 0.1");
    let r = failure(judge(&bad, 2000.0, Some(16384)));
    assert!(r.iter().any(|r| r.contains("Ux initial residual")));
}

#[test]
fn evidence_rejects_each_missing_or_nonfinite_residual() {
    for field in ["p", "Ux", "Uy"] {
        let missing = GOOD
            .lines()
            .filter(|l| !l.contains(&format!("Solving for {field},")))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!judge(&missing, 2000.0, Some(16384)).passed());
    }
    for bad in ["nan", "inf", "-inf", "-0.1"] {
        let log = GOOD.replace(
            "Initial residual = 1e-8",
            &format!("Initial residual = {bad}"),
        );
        assert!(!judge(&log, 2000.0, Some(16384)).passed());
    }
    assert!(
        !judge(
            &GOOD.replace("Final residual = 1e-11", "Final residual = nan"),
            2000.0,
            Some(16384)
        )
        .passed()
    );
}

#[test]
fn evidence_requires_correct_cells_volume_and_last_time() {
    for bad in [
        GOOD.replace("cells: 16384", "cells: 100"),
        GOOD.replace("Total volume = 0.0005", "Total volume = 0"),
        GOOD.replace("Mesh OK.", "Failed 1 mesh checks."),
        GOOD.replace("Time = 2000", "Time = 1999"),
    ] {
        assert!(!judge(&bad, 2000.0, Some(16384)).passed());
    }
    let wrong = MESH.replace("0.0005", "0.0004");
    assert!(
        !judge_run(
            &RunEvidence {
                block_mesh_log: BLOCK,
                check_mesh_log: &wrong,
                solver_log: GOOD,
                time_directories: &[2000.0]
            },
            &expected()
        )
        .passed()
    );
}

#[test]
fn evidence_uses_last_iteration_and_accepts_threshold_equality() {
    let good = GOOD
        .replace("1e-8", "1e-6")
        .replace("2e-8", "1e-6")
        .replace("3e-8", "1e-6");
    assert!(judge(&good, 2000.0, Some(16384)).passed());
    let early = format!(
        "Time = 1999\nsmoothSolver: Solving for Ux, Initial residual = 0.9, Final residual = 0.1\n{GOOD}"
    );
    assert!(judge(&early, 2000.0, Some(16384)).passed());
    let missing = format!("{GOOD}\nTime = 2000\nEnd");
    assert!(!judge(&missing, 2000.0, Some(16384)).passed());
}
