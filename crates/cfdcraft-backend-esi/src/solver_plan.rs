//! The solver a study runs, as one value derived from the resolved study. The `Allrun`, the
//! control dictionary and the judge of a run all read it, so a new solver is one more row here and
//! not another hard-coded name somewhere else. Every name in it is chosen from a fixed table: no
//! text of the study ever reaches a binary name, a log name or a field name.

use crate::evidence::EvidencePlan;
use cfdcraft_model::{InitMode, ResolvedStudy, ResolvedTime, TurbulenceModel};

/// Which limit of the study a residual is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidualLimit {
    /// The pressure limit of the study.
    Pressure,
    /// The velocity limit of the study, also used for the temperature.
    Velocity,
}

/// What the writer and the judge need to know about the solver of one study.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolverPlan {
    /// The solver program, for example `simpleFoam`.
    pub binary: String,
    /// The log the `Allrun` writes the solver's output to.
    pub log_file: String,
    pub transient: bool,
    pub buoyant: bool,
    /// The pressure field the solver solves for: `p`, or `p_rgh` when buoyant.
    pub pressure_field: String,
    /// The fields that must exist, non-empty, in the folder of the end time.
    pub final_fields: Vec<String>,
    /// The fields whose last initial residual gates a steady run; empty when transient.
    pub residual_checks: Vec<(String, ResidualLimit)>,
    /// Whether a potential-flow step runs before the solver.
    pub potential: bool,
}

/// The program of each combination of time and buoyancy.
fn binary_of(transient: bool, buoyant: bool) -> &'static str {
    match (buoyant, transient) {
        (false, false) => "simpleFoam",
        (false, true) => "pimpleFoam",
        (true, false) => "buoyantBoussinesqSimpleFoam",
        (true, true) => "buoyantBoussinesqPimpleFoam",
    }
}

impl SolverPlan {
    /// Steady, laminar, no buoyancy, uniform start: the solver of the lid-driven cavity.
    pub fn steady_laminar() -> Self {
        Self::build(false, false, TurbulenceModel::Laminar, false)
    }

    /// The plan of a resolved study; a study with no physics is steady and laminar.
    pub fn for_study(s: &ResolvedStudy) -> Self {
        let Some(physics) = s.physics.as_ref() else {
            return Self::steady_laminar();
        };
        Self::build(
            matches!(physics.time, ResolvedTime::Transient { .. }),
            physics.buoyancy.is_some(),
            physics.turbulence.model,
            physics.init == InitMode::Potential,
        )
    }

    fn build(transient: bool, buoyant: bool, model: TurbulenceModel, potential: bool) -> Self {
        let binary = binary_of(transient, buoyant);
        let pressure_field = if buoyant { "p_rgh" } else { "p" };
        let mut final_fields = vec!["U", pressure_field];
        if buoyant {
            final_fields.push("T");
            // The steady solver also writes the plain pressure; the transient one does not.
            if !transient {
                final_fields.push("p");
            }
        }
        match model {
            TurbulenceModel::Laminar => {}
            TurbulenceModel::KOmegaSst => final_fields.extend(["k", "omega", "nut"]),
            TurbulenceModel::RealizableKe | TurbulenceModel::RngKEpsilon => {
                final_fields.extend(["k", "epsilon", "nut"]);
            }
        }
        let mut residual_checks = Vec::new();
        if !transient {
            residual_checks.push((pressure_field.to_string(), ResidualLimit::Pressure));
            residual_checks.push(("Ux".to_string(), ResidualLimit::Velocity));
            residual_checks.push(("Uy".to_string(), ResidualLimit::Velocity));
            if buoyant {
                residual_checks.push(("T".to_string(), ResidualLimit::Velocity));
            }
        }
        Self {
            binary: binary.to_string(),
            log_file: format!("log.{binary}"),
            transient,
            buoyant,
            pressure_field: pressure_field.to_string(),
            final_fields: final_fields.into_iter().map(str::to_string).collect(),
            residual_checks,
            potential,
        }
    }

    /// The commands of the `Allrun` and the log each writes, for `np` processes. Plain commands
    /// on purpose: the tutorial helper functions skip a step whose log already exists, and a stale
    /// log would be judged as new. The potential-flow step runs before any decomposition.
    pub fn allrun_steps(&self, np: u32) -> Vec<(String, String)> {
        let step = |command: &str, log: &str| (command.to_string(), log.to_string());
        let mut steps = vec![
            step("blockMesh", "log.blockMesh"),
            step("checkMesh", "log.checkMesh"),
        ];
        if self.potential {
            steps.push(step("potentialFoam -initialiseUBCs", "log.potentialFoam"));
        }
        if np <= 1 {
            steps.push((self.binary.clone(), self.log_file.clone()));
        } else {
            steps.push(step("decomposePar -force", "log.decomposePar"));
            steps.push((
                format!("mpirun -np {np} {} -parallel", self.binary),
                self.log_file.clone(),
            ));
            steps.push(step("reconstructPar -latestTime", "log.reconstructPar"));
        }
        steps
    }

    /// The logs a run is judged on, in the order they are written: the mesh, the mesh checks, the
    /// potential-flow step when there is one, and the solver.
    pub fn judged_logs(&self) -> Vec<String> {
        let mut logs = vec!["log.blockMesh".to_string(), "log.checkMesh".to_string()];
        if self.potential {
            logs.push("log.potentialFoam".to_string());
        }
        logs.push(self.log_file.clone());
        logs
    }

    /// What the judge needs to know: no potential-flow log, which the caller supplies.
    pub fn evidence_plan(&self) -> EvidencePlan {
        EvidencePlan {
            solver: self.binary.clone(),
            pressure: self.pressure_field.clone(),
            temperature: self.buoyant,
            energy_field: "T".into(),
            residual_limits: !self.residual_checks.is_empty(),
            potential_log: None,
        }
    }
}
