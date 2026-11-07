//! How a Foundation 14 case is run: mesh, mesh check, the potential-flow step when the fields
//! start from a potential flow, then the solver module through `foamRun`.

use crate::{
    markers::{
        BLOCK_MESH_LOG, CHECK_MESH_LOG, ENERGY_FIELD, POTENTIAL_COMMAND, POTENTIAL_LOG, SOLVER,
        SOLVER_LOG,
    },
    turbulence::{model_of, transported},
};
use cfdcraft_backend::{LimitKind, RunPlan, RunStep};
use cfdcraft_model::{InitMode, ResolvedStudy, ResolvedTime};

fn step(command: &str, log: &str) -> RunStep {
    RunStep {
        command: command.to_owned(),
        log: log.to_owned(),
    }
}

/// The steps of the run script for `np` processes. Plain commands on purpose: each writes its own
/// log, so a stale log from an earlier run is never taken for a new one. The potential-flow step,
/// when there is one, runs before any decomposition.
fn steps(np: u32, potential: bool) -> Vec<RunStep> {
    let mut steps = vec![
        step("blockMesh", BLOCK_MESH_LOG),
        step("checkMesh", CHECK_MESH_LOG),
    ];
    if potential {
        steps.push(step(POTENTIAL_COMMAND, POTENTIAL_LOG));
    }
    if np <= 1 {
        steps.push(step(SOLVER, SOLVER_LOG));
    } else {
        steps.push(step("decomposePar -force", "log.decomposePar"));
        steps.push(step(
            &format!("mpirun -np {np} {SOLVER} -parallel"),
            SOLVER_LOG,
        ));
        steps.push(step("reconstructPar -latestTime", "log.reconstructPar"));
    }
    steps
}

/// The fields that must exist, non-empty, in the folder of the end time.
fn final_fields(buoyant: bool, study: &ResolvedStudy) -> Vec<String> {
    let mut fields = vec!["U"];
    if buoyant {
        fields.extend(["p_rgh", "T", "p"]);
    } else {
        fields.push("p");
    }
    let model = model_of(study);
    fields.extend(transported(model));
    if !transported(model).is_empty() {
        fields.push("nut");
    }
    fields.into_iter().map(str::to_owned).collect()
}

/// The run plan of a study for `np` processes. A steady run is held to the residual limits of the
/// study on the pressure, the two velocity components and, for heat transfer, the enthalpy.
pub fn run_plan(study: &ResolvedStudy, np: u32) -> RunPlan {
    let physics = study.physics.as_ref();
    let transient = physics.is_some_and(|p| matches!(p.time, ResolvedTime::Transient { .. }));
    let buoyant = physics.is_some_and(|p| p.buoyancy.is_some());
    let potential = physics.is_some_and(|p| p.init == InitMode::Potential);
    let pressure_field = if buoyant { "p_rgh" } else { "p" };
    let mut judged_logs = vec![BLOCK_MESH_LOG.to_owned(), CHECK_MESH_LOG.to_owned()];
    if potential {
        judged_logs.push(POTENTIAL_LOG.to_owned());
    }
    judged_logs.push(SOLVER_LOG.to_owned());
    let mut residual_checks = Vec::new();
    if !transient {
        residual_checks.push((pressure_field.to_owned(), LimitKind::Pressure));
        residual_checks.push(("Ux".to_owned(), LimitKind::Velocity));
        residual_checks.push(("Uy".to_owned(), LimitKind::Velocity));
        if buoyant {
            residual_checks.push((ENERGY_FIELD.to_owned(), LimitKind::Velocity));
        }
    }
    RunPlan {
        solver: SOLVER.to_owned(),
        steps: steps(np, potential),
        solver_log: SOLVER_LOG.to_owned(),
        judged_logs,
        final_fields: final_fields(buoyant, study),
        transient,
        buoyant,
        energy_field: ENERGY_FIELD.to_owned(),
        pressure_field: pressure_field.to_owned(),
        residual_checks,
        potential,
    }
}
