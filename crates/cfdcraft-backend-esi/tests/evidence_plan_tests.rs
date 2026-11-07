//! The judge takes the plan of the solver that ran. Every log here is built in the test.

mod plan_studies;

use cfdcraft_backend_esi::{
    EvidenceExpectations, EvidencePlan, RunEvidence, SmokeExpectations, SolverPlan, Verdict,
    evidence::{PartialExpectations, judge_partial},
    judge_partial_with, judge_run, judge_run_with, judge_smoke, judge_smoke_with,
};
use plan_studies::{TRANSIENT_ONLY, cavity, heated};

const BLOCK: &str = "Created 1024 cells\nEnd\n";
const MESH: &str = "cells: 1024\nTotal volume = 0.0005\nMesh OK.\nEnd\n";

fn failure(v: Verdict) -> Vec<String> {
    match v {
        Verdict::Passed => panic!("should fail"),
        Verdict::Failed(r) => r,
    }
}

fn expected(end_time: f64) -> EvidenceExpectations {
    EvidenceExpectations {
        end_time,
        cell_count: 1024,
        domain_volume: 0.0005,
        p_threshold: 1e-6,
        u_threshold: 1e-6,
    }
}

fn evidence<'a>(solver_log: &'a str, times: &'a [f64]) -> RunEvidence<'a> {
    RunEvidence {
        block_mesh_log: BLOCK,
        check_mesh_log: MESH,
        solver_log,
        time_directories: times,
    }
}

fn solve(field: &str, initial: &str) -> String {
    format!(
        "smoothSolver:  Solving for {field}, Initial residual = {initial}, Final residual = 1e-11, No Iterations 1\n"
    )
}

/// One iteration block: the time line and the solves, each field with its initial residual.
fn block(time: &str, solves: &[(&str, &str)]) -> String {
    let mut text = format!("Time = {time}\n\n");
    for (field, initial) in solves {
        text.push_str(&solve(field, initial));
    }
    text.push_str("ExecutionTime = 1 s  ClockTime = 1 s\n\n");
    text
}

fn buoyant_plan() -> EvidencePlan {
    EvidencePlan {
        solver: "buoyantBoussinesqSimpleFoam".into(),
        pressure: "p_rgh".into(),
        temperature: true,
        energy_field: "T".into(),
        residual_limits: true,
        potential_log: None,
    }
}

fn transient_plan() -> EvidencePlan {
    EvidencePlan {
        solver: "pimpleFoam".into(),
        pressure: "p".into(),
        temperature: false,
        energy_field: "T".into(),
        residual_limits: false,
        potential_log: None,
    }
}

#[test]
fn a_buoyant_steady_run_is_gated_on_p_rgh_and_t() {
    let log = |p_rgh: &str, with_t: bool| {
        let mut solves = vec![("Ux", "1e-8"), ("Uy", "2e-8"), ("p_rgh", p_rgh)];
        if with_t {
            solves.push(("T", "3e-9"));
        }
        block("100", &solves) + "End\n"
    };
    let times = [0.0, 100.0];
    let plan = buoyant_plan();
    let good = log("1e-8", true);
    assert_eq!(
        judge_run_with(&evidence(&good, &times), &expected(100.0), &plan),
        Verdict::Passed
    );

    let high = log("1e-3", true);
    let reasons = failure(judge_run_with(
        &evidence(&high, &times),
        &expected(100.0),
        &plan,
    ));
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("p_rgh") && r.contains("exceeds")),
        "{reasons:?}"
    );

    let no_t = log("1e-8", false);
    let reasons = failure(judge_run_with(
        &evidence(&no_t, &times),
        &expected(100.0),
        &plan,
    ));
    assert!(
        reasons.iter().any(|r| r.contains("residual for T")),
        "{reasons:?}"
    );

    // The cavity plan asks for `p`, which this log does not solve.
    let reasons = failure(judge_run(&evidence(&good, &times), &expected(100.0)));
    assert!(
        reasons.iter().any(|r| r.contains("residual for p")),
        "{reasons:?}"
    );
}

#[test]
fn a_buoyant_run_is_gated_on_the_plans_energy_field() {
    // The Foundation fluid module solves the enthalpy `h`, so its plan names `h` and no `T`
    // residual is asked for; the ESI plan keeps `T`.
    let log = |field: &str| {
        block(
            "100",
            &[
                ("Ux", "1e-8"),
                ("Uy", "2e-8"),
                ("p_rgh", "1e-8"),
                (field, "3e-9"),
            ],
        ) + "End\n"
    };
    let times = [0.0, 100.0];
    let h_plan = EvidencePlan {
        energy_field: "h".into(),
        ..buoyant_plan()
    };
    let with_h = log("h");
    assert_eq!(
        judge_run_with(&evidence(&with_h, &times), &expected(100.0), &h_plan),
        Verdict::Passed
    );
    let reasons = failure(judge_run_with(
        &evidence(&with_h, &times),
        &expected(100.0),
        &buoyant_plan(),
    ));
    assert!(
        reasons.iter().any(|r| r.contains("residual for T")),
        "{reasons:?}"
    );
    let with_t = log("T");
    let reasons = failure(judge_run_with(
        &evidence(&with_t, &times),
        &expected(100.0),
        &h_plan,
    ));
    assert!(
        reasons.iter().any(|r| r.contains("residual for h")),
        "{reasons:?}"
    );
}

#[test]
fn reasons_carry_the_solver_name() {
    let log = block("8", &[("Ux", "1e-8"), ("Uy", "1e-8"), ("p", "1e-8")]);
    let times = [0.0, 8.0];
    let reasons = failure(judge_run_with(
        &evidence(&log, &times),
        &expected(8.0),
        &transient_plan(),
    ));
    assert!(
        reasons.contains(&"pimpleFoam: missing terminal End marker".to_string()),
        "{reasons:?}"
    );
    let reasons = failure(judge_run(&evidence(&log, &times), &expected(8.0)));
    assert!(
        reasons.contains(&"simpleFoam: missing terminal End marker".to_string()),
        "{reasons:?}"
    );
}

/// Two time steps of a transient run, the last with large residuals.
fn transient_log(last_p: &str) -> String {
    block("7.5", &[("Ux", "0.3"), ("Uy", "0.2"), ("p", "0.4")])
        + &block("8", &[("Ux", "0.3"), ("Uy", "0.2"), ("p", last_p)])
        + "End\n"
}

#[test]
fn a_transient_run_is_not_gated_on_residuals() {
    let times = [0.0, 4.0, 8.0];
    let plan = transient_plan();
    let log = transient_log("0.5");
    assert_eq!(
        judge_run_with(&evidence(&log, &times), &expected(8.0), &plan),
        Verdict::Passed
    );
    // The same log is not good for a steady plan: the residuals are over the limit.
    assert!(!judge_run(&evidence(&log, &times), &expected(8.0)).passed());

    // The end time folder is still required.
    let early = [0.0, 4.0];
    assert!(!judge_run_with(&evidence(&log, &early), &expected(8.0), &plan).passed());

    // A residual that is not a number still fails.
    let broken = transient_log("nan");
    assert!(!judge_run_with(&evidence(&broken, &times), &expected(8.0), &plan).passed());

    // A log with no iteration block still fails.
    let none = solve("p", "0.5") + "End\n";
    let reasons = failure(judge_run_with(
        &evidence(&none, &times),
        &expected(8.0),
        &plan,
    ));
    assert!(
        reasons.iter().any(|r| r.contains("no iteration blocks")),
        "{reasons:?}"
    );
}

#[test]
fn times_may_carry_a_unit_letter() {
    let times = [0.0, 8.0];
    let plan = transient_plan();
    for time in ["8s", "8"] {
        let log = block(time, &[("Ux", "0.3"), ("Uy", "0.2"), ("p", "0.5")]) + "End\n";
        assert_eq!(
            judge_run_with(&evidence(&log, &times), &expected(8.0), &plan),
            Verdict::Passed,
            "Time = {time}"
        );
    }
    let log = block("7s", &[("Ux", "0.3"), ("Uy", "0.2"), ("p", "0.5")]) + "End\n";
    let reasons = failure(judge_run_with(
        &evidence(&log, &times),
        &expected(8.0),
        &plan,
    ));
    assert!(
        reasons.iter().any(|r| r.contains("final time differs")),
        "{reasons:?}"
    );
}

#[test]
fn the_potential_flow_log_must_end() {
    let log = transient_log("0.5");
    let times = [0.0, 8.0];
    let with = |text: Option<&str>| EvidencePlan {
        potential_log: text.map(str::to_string),
        ..transient_plan()
    };
    let run = |plan: &EvidencePlan| judge_run_with(&evidence(&log, &times), &expected(8.0), plan);
    assert_eq!(run(&with(None)), Verdict::Passed);
    assert_eq!(
        run(&with(Some("Calculating potential flow\nEnd\n"))),
        Verdict::Passed
    );
    let reasons = failure(run(&with(Some("Calculating potential flow\n"))));
    assert!(
        reasons.contains(&"potentialFoam: missing terminal End marker".to_string()),
        "{reasons:?}"
    );
    let reasons = failure(run(&with(Some("--> FOAM FATAL ERROR:\nEnd\n"))));
    assert!(
        reasons.contains(&"potentialFoam: fatal signal FOAM FATAL".to_string()),
        "{reasons:?}"
    );
}

#[test]
fn the_old_entry_points_are_unchanged() {
    let good = block("2", &[("Ux", "1e-8"), ("Uy", "2e-8"), ("p", "3e-8")]) + "End\n";
    let bad = block("2", &[("Ux", "1e-3"), ("Uy", "2e-8"), ("p", "3e-8")]);
    let cavity_plan = EvidencePlan::cavity();
    assert_eq!(cavity_plan.solver, "simpleFoam");
    for (log, times) in [(&good, [0.0, 2.0]), (&bad, [0.0, 1.0])] {
        let e = evidence(log, &times);
        assert_eq!(
            judge_run(&e, &expected(2.0)),
            judge_run_with(&e, &expected(2.0), &cavity_plan)
        );
        let smoke = SmokeExpectations {
            end_time: 2.0,
            cell_count: 1024,
            domain_volume: 0.0005,
        };
        assert_eq!(
            judge_smoke(&e, &smoke),
            judge_smoke_with(&e, &smoke, &cavity_plan)
        );
        let partial = PartialExpectations {
            end_time: 2.0,
            cell_count: 1024,
            domain_volume: 0.0005,
        };
        assert_eq!(
            judge_partial(&e, &partial),
            judge_partial_with(&e, &partial, &cavity_plan)
        );
    }
    assert!(judge_run(&evidence(&good, &[0.0, 2.0]), &expected(2.0)).passed());
    assert!(!judge_run(&evidence(&bad, &[0.0, 1.0]), &expected(2.0)).passed());
}

#[test]
fn the_evidence_plan_comes_from_the_solver_plan() {
    let transient = SolverPlan::for_study(&cavity(TRANSIENT_ONLY, "8", "0.5", 0));
    assert_eq!(transient.evidence_plan(), transient_plan());
    let steady = SolverPlan::for_study(&heated("", "1000", "100"));
    assert_eq!(steady.evidence_plan(), buoyant_plan());
    assert_eq!(
        SolverPlan::steady_laminar().evidence_plan(),
        EvidencePlan::cavity()
    );
}
