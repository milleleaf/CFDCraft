//! Judging a finished OpenFOAM run from the logs and result files it left. The solver's exit status
//! takes no part in it. The function takes the neutral run plan, so another OpenFOAM line that
//! writes the same logs reuses it with its own plan.

use crate::evidence::{
    EvidenceExpectations, EvidencePlan, PartialExpectations, RunEvidence, SmokeExpectations,
    Verdict, judge_partial_with, judge_run_with, judge_smoke_with,
};
use cfdcraft_backend::{JudgeMode, JudgeOutcome, RunFiles, RunPlan};
use cfdcraft_model::ResolvedStudy;

/// The logs and the numeric time folders a run left.
struct Logs {
    block_mesh: String,
    check_mesh: String,
    solver: String,
    /// The log of the potential-flow step, when the plan has one.
    potential: Option<String>,
    times: Vec<f64>,
}

impl Logs {
    /// The logs are read by the names the plan gives, in the order it lists them: the mesh
    /// generator, the mesh check, the potential-flow step when there is one, then the solver. A
    /// missing log is an empty text, which the judge fails on its missing end marker.
    fn read(files: &dyn RunFiles, plan: &RunPlan) -> Self {
        let log = |name: Option<&String>| name.and_then(|n| files.read_text(n)).unwrap_or_default();
        let times = files
            .list("")
            .iter()
            .filter(|name| files.size(name).is_none())
            .filter_map(|name| name.parse::<f64>().ok())
            .collect();
        Self {
            block_mesh: log(plan.judged_logs.first()),
            check_mesh: log(plan.judged_logs.get(1)),
            solver: log(Some(&plan.solver_log)),
            potential: plan.potential.then(|| log(plan.judged_logs.get(2))),
            times,
        }
    }

    fn evidence(&self) -> RunEvidence<'_> {
        RunEvidence {
            block_mesh_log: &self.block_mesh,
            check_mesh_log: &self.check_mesh,
            solver_log: &self.solver,
            time_directories: &self.times,
        }
    }

    /// What the judge needs to know of the solver, with the potential-flow log it needs.
    fn judge_plan(&self, plan: &RunPlan) -> EvidencePlan {
        EvidencePlan {
            solver: plan.solver.clone(),
            pressure: plan.pressure_field.clone(),
            temperature: plan.buoyant,
            energy_field: plan.energy_field.clone(),
            residual_limits: !plan.residual_checks.is_empty(),
            potential_log: self.potential.clone(),
        }
    }
}

/// Field names as a phrase: `U and p`, `U, p_rgh and T`.
fn field_phrase(fields: &[String]) -> String {
    match fields.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

/// True when a time folder at the end time holds every named field, non-empty.
fn final_fields_present(files: &dyn RunFiles, end_time: f64, fields: &[String]) -> bool {
    files
        .list("")
        .iter()
        .filter(|name| {
            name.parse::<f64>()
                .is_ok_and(|t| (t - end_time).abs() < 1e-6)
        })
        .any(|dir| {
            fields
                .iter()
                .all(|name| files.size(&format!("{dir}/{name}")).is_some_and(|n| n > 0))
        })
}

/// Judge a finished OpenFOAM run from its logs and time folders. `Run` holds the residuals to the
/// limits of the study, `Smoke` and `Finished` hold none (`Smoke` says `smoke_passed`, `Finished`
/// says `passed`), and `Stopped` judges a run stopped on request at a time.
pub fn judge_openfoam(
    files: &dyn RunFiles,
    resolved: &ResolvedStudy,
    mode: JudgeMode,
    plan: &RunPlan,
) -> JudgeOutcome {
    let logs = Logs::read(files, plan);
    match mode {
        JudgeMode::Stopped { time } => judge_stopped(files, resolved, &time, plan, &logs),
        JudgeMode::Run => judge_ended(files, resolved, plan, &logs, true, "passed"),
        JudgeMode::Smoke => judge_ended(files, resolved, plan, &logs, false, "smoke_passed"),
        JudgeMode::Finished => judge_ended(files, resolved, plan, &logs, false, "passed"),
    }
}

/// A run that reached its end, with or without the residual limits.
fn judge_ended(
    files: &dyn RunFiles,
    r: &ResolvedStudy,
    plan: &RunPlan,
    logs: &Logs,
    limits: bool,
    passed: &str,
) -> JudgeOutcome {
    let evidence = logs.evidence();
    let judge = logs.judge_plan(plan);
    let mut reasons = Vec::new();
    let verdict = match r.cell_count() {
        None => {
            reasons.push("the study's cell count overflows".to_owned());
            None
        }
        Some(cell_count) if !limits => Some(judge_smoke_with(
            &evidence,
            &SmokeExpectations {
                end_time: r.solver.end_time,
                cell_count,
                domain_volume: r.meshed_volume(),
            },
            &judge,
        )),
        Some(cell_count) => Some(judge_run_with(
            &evidence,
            &EvidenceExpectations {
                end_time: r.solver.end_time,
                cell_count,
                domain_volume: r.meshed_volume(),
                p_threshold: r.solver.convergence.p,
                u_threshold: r.solver.convergence.u,
            },
            &judge,
        )),
    };
    if let Some(Verdict::Failed(found)) = &verdict {
        reasons.extend(found.iter().cloned());
    }
    if !final_fields_present(files, r.solver.end_time, &plan.final_fields) {
        reasons.push(format!(
            "the final-time {} field files are missing or empty",
            field_phrase(&plan.final_fields)
        ));
    }
    let verdict = if reasons.is_empty() { passed } else { "failed" };
    JudgeOutcome {
        verdict: verdict.to_owned(),
        reasons,
    }
}

/// A run stopped on request at `time`: every log ends, nothing is fatal, the mesh is good, the
/// residuals are well formed, the last logged time is the newest time folder, and that folder holds
/// a non-empty `U` and pressure field. There is no residual limit and no end time to reach.
fn judge_stopped(
    files: &dyn RunFiles,
    r: &ResolvedStudy,
    time: &str,
    plan: &RunPlan,
    logs: &Logs,
) -> JudgeOutcome {
    let evidence = logs.evidence();
    let mut reasons = Vec::new();
    match r.cell_count() {
        None => reasons.push("the study's cell count overflows".to_owned()),
        Some(cell_count) => {
            let expected = PartialExpectations {
                end_time: r.solver.end_time,
                cell_count,
                domain_volume: r.meshed_volume(),
            };
            if let Verdict::Failed(found) =
                judge_partial_with(&evidence, &expected, &logs.judge_plan(plan))
            {
                reasons.extend(found);
            }
        }
    }
    // The time becomes part of a path, so only a plain decimal number is used as one.
    let plain = !time.is_empty()
        && time.bytes().all(|c| c.is_ascii_digit() || c == b'.')
        && time.bytes().any(|c| c.is_ascii_digit());
    let filled = |name: &str| plain && files.size(&format!("{time}/{name}")).is_some_and(|n| n > 0);
    // A stopped run is held to the velocity and the pressure it solves for.
    let needed = ["U".to_owned(), plan.pressure_field.clone()];
    if !needed.iter().all(|name| filled(name)) {
        reasons.push(format!(
            "the {} field files of the stopped time {time} are missing or empty",
            field_phrase(&needed)
        ));
    }
    JudgeOutcome {
        verdict: if reasons.is_empty() {
            "passed"
        } else {
            "failed"
        }
        .to_owned(),
        reasons,
    }
}
