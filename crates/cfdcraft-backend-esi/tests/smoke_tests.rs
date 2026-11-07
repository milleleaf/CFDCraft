use cfdcraft_backend_esi::{
    EvidenceExpectations, RunEvidence, SmokeExpectations, Verdict, judge_run, judge_smoke,
};

const BLOCK: &str = include_str!("fixtures/v2512-blockMesh.log");
const MESH: &str = include_str!("fixtures/v2512-checkMesh.log");
const SOLVER: &str = include_str!("fixtures/v2512-simpleFoam-final-iteration.log");

fn smoke() -> SmokeExpectations {
    SmokeExpectations {
        end_time: 2000.0,
        cell_count: 16384,
        domain_volume: 0.0005,
    }
}
fn convergence() -> EvidenceExpectations {
    EvidenceExpectations {
        end_time: 2000.0,
        cell_count: 16384,
        domain_volume: 0.0005,
        p_threshold: 1e-6,
        u_threshold: 1e-6,
    }
}
fn evidence<'a>(
    block: &'a str,
    mesh: &'a str,
    solver: &'a str,
    dirs: &'a [f64],
) -> RunEvidence<'a> {
    RunEvidence {
        block_mesh_log: block,
        check_mesh_log: mesh,
        solver_log: solver,
        time_directories: dirs,
    }
}
fn failure(v: Verdict) -> Vec<String> {
    match v {
        Verdict::Passed => panic!("should fail"),
        Verdict::Failed(r) => r,
    }
}

#[test]
fn smoke_passes_a_real_run_that_has_not_converged() {
    let e = evidence(BLOCK, MESH, SOLVER, &[0.0, 2000.0]);
    assert_eq!(judge_smoke(&e, &smoke()), Verdict::Passed);
}

#[test]
fn smoke_pass_is_not_a_convergence_pass() {
    let e = evidence(BLOCK, MESH, SOLVER, &[0.0, 2000.0]);
    let r = failure(judge_run(&e, &convergence()));
    assert_eq!(r.len(), 3, "p, Ux and Uy residuals only: {r:?}");
    assert!(r.iter().all(|r| r.contains("initial residual")));
}

const RUN_DIRS: &[f64] = &[0.0, 2000.0];

fn smoke_failure(block: &str, mesh: &str, solver: &str, dirs: &[f64]) -> Vec<String> {
    failure(judge_smoke(&evidence(block, mesh, solver, dirs), &smoke()))
}

#[test]
fn smoke_fails_non_finite_numbers() {
    for bad in ["nan", "inf", "-inf"] {
        let solver = SOLVER.replacen(
            "Initial residual = 1.40824257965e-05",
            &format!("Initial residual = {bad}"),
            1,
        );
        let r = smoke_failure(BLOCK, MESH, &solver, RUN_DIRS);
        assert!(r.iter().any(|r| r.contains("non-finite")), "{bad}: {r:?}");
    }
    let solver = SOLVER.replace("Time = 2000\n", "Time = 2000\ncontinuity error nan\n");
    let r = smoke_failure(BLOCK, MESH, &solver, RUN_DIRS);
    assert!(r.iter().any(|r| r.contains("non-finite")), "{r:?}");
}

#[test]
fn smoke_fails_fatal_and_floating_point_markers() {
    for (marker, expect) in [
        ("--> FOAM FATAL ERROR: bad", "FOAM FATAL"),
        (
            "Floating point exception (core dumped)",
            "Floating point exception",
        ),
        ("#0  Foam::sigFpe::sigHandler(int)", "SIGFPE stack trace"),
    ] {
        let solver = SOLVER.replace("Time = 2000\n", &format!("Time = 2000\n{marker}\n"));
        let r = smoke_failure(BLOCK, MESH, &solver, RUN_DIRS);
        assert!(r.iter().any(|r| r.contains(expect)), "{marker}: {r:?}");
    }
}

#[test]
fn smoke_requires_every_step_log_to_end() {
    let cut = |s: &str| s.replace("End", "Nearly there");
    let cases = [
        (
            cut(BLOCK),
            MESH.to_string(),
            SOLVER.to_string(),
            "blockMesh",
        ),
        (
            BLOCK.to_string(),
            cut(MESH),
            SOLVER.to_string(),
            "checkMesh",
        ),
        (
            BLOCK.to_string(),
            MESH.to_string(),
            cut(SOLVER),
            "simpleFoam",
        ),
    ];
    for (b, m, s, step) in cases {
        let r = smoke_failure(&b, &m, &s, RUN_DIRS);
        assert!(
            r.iter().any(|r| r.contains(step) && r.contains("End")),
            "{step}: {r:?}"
        );
    }
}

#[test]
fn smoke_requires_the_final_time_to_be_reached_and_written() {
    let early = SOLVER.replace("Time = 2000", "Time = 1999");
    let r = smoke_failure(BLOCK, MESH, &early, RUN_DIRS);
    assert!(r.iter().any(|r| r.contains("final time")), "{r:?}");
    for dirs in [&[0.0, 1500.0][..], &[]] {
        let r = smoke_failure(BLOCK, MESH, SOLVER, dirs);
        assert!(r.iter().any(|r| r.contains("time directory")), "{r:?}");
    }
}

#[test]
fn smoke_requires_a_plausible_mesh() {
    let cases = [
        (MESH.replace("Mesh OK.", "Failed 1 mesh checks."), "Mesh OK"),
        (MESH.replace("16384", "100"), "cells"),
        (
            MESH.replace("Total volume = 0.0005", "Total volume = 0"),
            "volume",
        ),
        (
            MESH.replace("Total volume = 0.0005", "Total volume = 0.0004"),
            "volume",
        ),
    ];
    for (mesh, expect) in cases {
        assert_ne!(mesh, MESH, "case must differ from the passing mesh log");
        let r = smoke_failure(BLOCK, &mesh, SOLVER, RUN_DIRS);
        assert!(r.iter().any(|r| r.contains(expect)), "{expect}: {r:?}");
    }
}

#[test]
fn smoke_requires_at_least_one_solved_field_in_an_iteration() {
    let drop = |keep: fn(&str) -> bool| {
        SOLVER
            .lines()
            .filter(|l| keep(l))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let no_iteration = drop(|l| !l.starts_with("Time = ") && !l.contains("Solving for"));
    let r = smoke_failure(BLOCK, MESH, &no_iteration, RUN_DIRS);
    assert!(r.iter().any(|r| r.contains("no iteration")), "{r:?}");
    // Keep a `Time = 2000` line so only the missing `Solving for` lines differ.
    let empty_block = drop(|l| !l.contains("Solving for"));
    let r = smoke_failure(BLOCK, MESH, &empty_block, RUN_DIRS);
    assert!(r.iter().any(|r| r.contains("no solved field")), "{r:?}");
}

#[test]
fn smoke_rejects_negative_residual_values() {
    let solver = SOLVER.replacen(
        "Initial residual = 1.40824257965e-05",
        "Initial residual = -0.1",
        1,
    );
    let r = smoke_failure(BLOCK, MESH, &solver, RUN_DIRS);
    assert!(r.iter().any(|r| r.contains("invalid residual")), "{r:?}");
}

#[test]
fn smoke_ignores_the_harmless_trap_banner() {
    assert!(SOLVER.contains("Floating point exception trapping enabled (FOAM_SIGFPE)"));
    assert!(judge_smoke(&evidence(BLOCK, MESH, SOLVER, RUN_DIRS), &smoke()).passed());
}
