//! A run is judged by the solver its study asks for: the log it names, the fields it must leave,
//! the cell count and volume of its layout, the residual gate and the potential-flow step. The
//! export of a case carries the `Allrun` of the same plan. No test needs a compute host or a
//! container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_backend_esi::{Dialect, SolverPlan, export::allrun_script_with};
use cfdcraft_model::ResolvedStudy;
use cfdcraft_ops::run::{judge_results, judge_stopped_results};
use cfdcraft_ops::{RunMode, prepare};

#[path = "plan_studies/mod.rs"]
mod plan_studies;
use plan_studies::*;

fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("solver-plan-ops-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A solver log that ends at `time`, with one solved line per field, each at this initial residual.
fn solver_log(time: &str, fields: &[&str], residual: &str, extra: &str) -> String {
    let mut log = format!("Time = {time}\n");
    for field in fields {
        log.push_str(&format!(
            "smoothSolver: Solving for {field}, Initial residual = {residual}, Final residual = 1e-11, No Iterations 1\n"
        ));
    }
    log.push_str("ExecutionTime = 1 s ClockTime = 1 s\n");
    log.push_str(extra);
    log.push_str("End\n");
    log
}

/// The folder a good run of `r` leaves: both mesh logs for `cells` cells, the plan's solver log,
/// and the plan's fields at the end time.
fn write_folder(dir: &Path, r: &ResolvedStudy, cells: u64, residual_fields: &[&str]) {
    let plan = SolverPlan::for_study(r);
    let end = format!("{}", r.solver.end_time);
    fs::create_dir_all(dir.join(&end)).unwrap();
    fs::write(
        dir.join("log.blockMesh"),
        format!("Created {cells} cells\nEnd\n"),
    )
    .unwrap();
    fs::write(
        dir.join("log.checkMesh"),
        format!(
            "cells: {cells}\nTotal volume = {:.12}\nMesh OK.\nEnd\n",
            r.domain_volume()
        ),
    )
    .unwrap();
    fs::write(
        dir.join(&plan.log_file),
        solver_log(&end, residual_fields, "1e-07", ""),
    )
    .unwrap();
    for field in &plan.final_fields {
        fs::write(dir.join(&end).join(field), format!("field {field}\n")).unwrap();
    }
}

fn has_reason(reasons: &[String], part: &str) -> bool {
    reasons.iter().any(|r| r.contains(part))
}

#[test]
fn a_transient_results_folder_is_judged_by_its_plan() {
    let r = transient_cavity();
    let plan = SolverPlan::for_study(&r);
    assert_eq!(plan.log_file, "log.pimpleFoam");
    let dir = scratch("transient");
    write_folder(&dir, &r, r.cell_count().unwrap(), &["Ux", "Uy", "p"]);

    // There is no residual limit: a transient run is judged on its end markers, time and fields.
    fs::write(
        dir.join("log.pimpleFoam"),
        solver_log("8", &["Ux", "Uy", "p"], "1e-02", ""),
    )
    .unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "passed", "{:?}", judged.reasons);

    // A missing pressure field fails the run and the reason names the fields it needs.
    fs::remove_file(dir.join("8/p")).unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "failed");
    assert!(
        has_reason(&judged.reasons, "U and p field files"),
        "{:?}",
        judged.reasons
    );
    fs::write(dir.join("8/p"), "field p\n").unwrap();

    // A fatal marker in the solver log fails it.
    fs::write(
        dir.join("log.pimpleFoam"),
        solver_log(
            "8",
            &["Ux", "Uy", "p"],
            "1e-02",
            "Floating point exception (core dumped)\n",
        ),
    )
    .unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "failed");
    assert!(has_reason(&judged.reasons, "fatal"), "{:?}", judged.reasons);
}

#[test]
fn the_log_of_another_solver_is_not_the_evidence() {
    let r = transient_cavity();
    let dir = scratch("wrong-log");
    write_folder(&dir, &r, r.cell_count().unwrap(), &["Ux", "Uy", "p"]);
    // A good steady-solver log beside the transient run's own, which is gone.
    fs::rename(dir.join("log.pimpleFoam"), dir.join("log.simpleFoam")).unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "failed");
    assert!(
        has_reason(&judged.reasons, "missing terminal End marker"),
        "{:?}",
        judged.reasons
    );
}

#[test]
fn a_buoyant_folder_needs_t_and_p_rgh() {
    let r = resolved(&heated_json(""));
    let plan = SolverPlan::for_study(&r);
    assert_eq!(plan.final_fields, ["U", "p_rgh", "T", "p"]);
    let dir = scratch("buoyant");
    write_folder(
        &dir,
        &r,
        r.cell_count().unwrap(),
        &["Ux", "Uy", "p_rgh", "T"],
    );
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "passed", "{:?}", judged.reasons);

    // The temperature file is missing: the reason lists the fields the run must leave.
    fs::remove_file(dir.join("5000/T")).unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "failed");
    assert!(
        has_reason(&judged.reasons, "U, p_rgh, T and p field files"),
        "{:?}",
        judged.reasons
    );
    fs::write(dir.join("5000/T"), "field T\n").unwrap();

    // The residual gate names the reduced pressure and the temperature.
    fs::write(
        dir.join(&plan.log_file),
        solver_log("5000", &["Ux", "Uy", "p_rgh", "T"], "1e-03", ""),
    )
    .unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "failed");
    assert!(
        has_reason(&judged.reasons, "final iteration p_rgh"),
        "{:?}",
        judged.reasons
    );
    assert!(
        has_reason(&judged.reasons, "final iteration T "),
        "{:?}",
        judged.reasons
    );
}

#[test]
fn a_shape_study_uses_its_layout_for_the_mesh_check() {
    let r = resolved(&step_json());
    let layout_cells = r.cell_count().expect("the step has a layout");
    let box_cells = r.mesh.cells_x * r.mesh.cells_y * r.mesh.cells_z;
    let box_volume = r.domain.length_x * r.domain.length_y * r.domain.length_z;
    assert_ne!(layout_cells, box_cells, "the layout is not the box product");
    assert!(r.domain_volume() < box_volume, "the step removes volume");

    let dir = scratch("shape");
    write_folder(&dir, &r, layout_cells, &["Ux", "Uy", "p"]);
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "passed", "{:?}", judged.reasons);

    // The mesh check of the bounding box fails: wrong cell count and wrong volume.
    fs::write(
        dir.join("log.checkMesh"),
        format!("cells: {box_cells}\nTotal volume = {box_volume:.12}\nMesh OK.\nEnd\n"),
    )
    .unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "failed");
    assert!(
        has_reason(&judged.reasons, "expected"),
        "{:?}",
        judged.reasons
    );
    assert!(
        has_reason(&judged.reasons, "volume"),
        "{:?}",
        judged.reasons
    );
}

#[test]
fn a_potential_run_needs_its_log() {
    let r = potential_cavity();
    assert!(SolverPlan::for_study(&r).potential);
    let dir = scratch("potential");
    write_folder(&dir, &r, r.cell_count().unwrap(), &["Ux", "Uy", "p"]);

    // Without the log of the potential-flow step the run is not judged good.
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "failed");
    assert!(
        has_reason(&judged.reasons, "potentialFoam"),
        "{:?}",
        judged.reasons
    );

    // A log that does not end fails.
    fs::write(
        dir.join("log.potentialFoam"),
        "Calculating potential flow\n",
    )
    .unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "failed");
    assert!(
        has_reason(&judged.reasons, "potentialFoam"),
        "{:?}",
        judged.reasons
    );

    // A log that ends passes.
    fs::write(
        dir.join("log.potentialFoam"),
        "Calculating potential flow\nEnd\n",
    )
    .unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "passed", "{:?}", judged.reasons);
}

#[test]
fn without_potential_flow_the_potential_log_is_not_read() {
    let r = resolved(CAVITY);
    assert!(!SolverPlan::for_study(&r).potential);
    let dir = scratch("no-potential");
    write_folder(&dir, &r, r.cell_count().unwrap(), &["Ux", "Uy", "p"]);
    fs::write(dir.join("log.potentialFoam"), "Floating point exception\n").unwrap();
    let judged = judge_results(&dir, &r, RunMode::Run);
    assert_eq!(judged.verdict, "passed", "{:?}", judged.reasons);
}

/// The text of the `Allrun` of an exported tree.
fn allrun_of(tree: &cfdcraft_backend_esi::CaseTree) -> String {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new("Allrun"))
        .expect("the export holds an Allrun");
    String::from_utf8(file.bytes.clone()).unwrap()
}

#[test]
fn the_export_carries_the_plans_allrun() {
    // The cavity: the export is the one it always was.
    let cavity = prepare(CAVITY, Dialect::EsiV2512).expect("the cavity passes the gates");
    assert_eq!(
        allrun_of(&cavity.export_tree),
        allrun_script_with(&SolverPlan::steady_laminar(), 1)
    );

    // A transient or potential-flow study: when the case reads back, the export carries the script
    // of its plan. The read-back check may still refuse these until the reader knows the plan, and
    // then only that check may be the reason.
    for (name, text) in [
        ("transient", transient_cavity_text()),
        ("potential", potential_cavity_text()),
    ] {
        let plan = SolverPlan::for_study(&resolved(&text));
        match prepare(&text, Dialect::EsiV2512) {
            Ok(prepared) => {
                let script = allrun_of(&prepared.export_tree);
                assert_eq!(script, allrun_script_with(&plan, 1));
                assert!(script.contains(&plan.binary), "{name}");
            }
            Err(report) => {
                let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
                assert!(
                    !codes.is_empty() && codes.iter().all(|c| *c == "CFD-RT-001"),
                    "{name}: {codes:?}"
                );
            }
        }
    }
}

#[test]
fn a_stopped_transient_run_is_judged() {
    let r = transient_cavity();
    let dir = scratch("stopped");
    let cells = r.cell_count().unwrap();
    fs::create_dir_all(dir.join("3")).unwrap();
    fs::write(
        dir.join("log.blockMesh"),
        format!("Created {cells} cells\nEnd\n"),
    )
    .unwrap();
    fs::write(
        dir.join("log.checkMesh"),
        format!(
            "cells: {cells}\nTotal volume = {:.12}\nMesh OK.\nEnd\n",
            r.domain_volume()
        ),
    )
    .unwrap();
    fs::write(
        dir.join("log.pimpleFoam"),
        solver_log("3", &["Ux", "Uy", "p"], "1e-02", ""),
    )
    .unwrap();
    fs::write(dir.join("3/U"), "field U\n").unwrap();
    fs::write(dir.join("3/p"), "field p\n").unwrap();
    let judged = judge_stopped_results(&dir, &r, "3");
    assert_eq!(judged.verdict, "passed", "{:?}", judged.reasons);

    // The pressure field of the stopped time is needed.
    fs::remove_file(dir.join("3/p")).unwrap();
    let judged = judge_stopped_results(&dir, &r, "3");
    assert_eq!(judged.verdict, "failed");
    assert!(
        has_reason(&judged.reasons, "U and p field files of the stopped time 3"),
        "{:?}",
        judged.reasons
    );
}
