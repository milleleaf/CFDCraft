//! The OpenFOAM backend of the OpenCFD line as an implementor of the shared backend trait. Every
//! method hands over to the function the writer, the safety scan, the reader and the exporter
//! already have, so a study written through the trait gives the same bytes as the direct call.

use crate::{
    CompileError, Dialect, ResidualLimit, SolverPlan, WRITER_SOURCES,
    case::{CaseOptions, compile_with},
    export::export_tree_with,
    guard,
    reader::{EsiReader, source_files},
    shaped_expected_facts,
};
use cfdcraft_backend::{
    Backend, CaseTree, CompileFailure, CompileOptions, Family, JudgeMode, JudgeOutcome, LimitKind,
    Refusal, RunFiles, RunPlan, RunStep,
};
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy, roundtrip::check_roundtrip_with};

mod judge;

pub use judge::judge_openfoam;

/// One dialect of the OpenCFD line, as a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EsiBackend {
    dialect: Dialect,
}

impl EsiBackend {
    pub const fn new(dialect: Dialect) -> Self {
        Self { dialect }
    }
}

/// A refusal of the safety scan in the form every backend reports.
fn neutral_refusal(r: &guard::Refusal) -> Refusal {
    Refusal {
        code: r.rule.code(),
        file: r.file.clone(),
        line: u32::try_from(r.line).unwrap_or(u32::MAX),
        entry: r.entry.clone(),
        message: r.message.clone(),
    }
}

/// A compile error in the form every backend reports; the text is the one the error always had.
fn neutral_failure(error: CompileError) -> CompileFailure {
    match error {
        CompileError::Raw { label, reason } => CompileFailure::Raw { label, reason },
        other => CompileFailure::Invalid {
            reason: other.to_string(),
        },
    }
}

fn limit_kind(limit: ResidualLimit) -> LimitKind {
    match limit {
        ResidualLimit::Pressure => LimitKind::Pressure,
        ResidualLimit::Velocity => LimitKind::Velocity,
    }
}

impl Backend for EsiBackend {
    fn id(&self) -> &'static str {
        self.dialect.id()
    }

    fn family(&self) -> Family {
        Family::Esi
    }

    fn image(&self) -> Option<&'static str> {
        Some(self.dialect.image())
    }

    fn preview(&self) -> bool {
        false
    }

    fn capabilities(&self) -> Capabilities {
        self.dialect.capabilities()
    }

    fn compile(
        &self,
        study: &ResolvedStudy,
        options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure> {
        compile_with(study, self.dialect, &CaseOptions { np: options.np }).map_err(neutral_failure)
    }

    fn scan(&self, tree: &CaseTree) -> Vec<Refusal> {
        guard::scan_tree(tree).iter().map(neutral_refusal).collect()
    }

    fn read_back(&self, study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic> {
        check_roundtrip_with(
            &EsiReader,
            &source_files(tree),
            study,
            &shaped_expected_facts(study),
        )
    }

    fn export(
        &self,
        study: &ResolvedStudy,
        tree: &CaseTree,
        hash: &str,
        options: &CompileOptions,
    ) -> CaseTree {
        export_tree_with(tree, hash, options.np, &SolverPlan::for_study(study))
    }

    fn run_plan(&self, study: &ResolvedStudy, np: u32) -> RunPlan {
        let plan = SolverPlan::for_study(study);
        RunPlan {
            solver: plan.binary.clone(),
            steps: plan
                .allrun_steps(np)
                .into_iter()
                .map(|(command, log)| RunStep { command, log })
                .collect(),
            solver_log: plan.log_file.clone(),
            judged_logs: plan.judged_logs(),
            final_fields: plan.final_fields.clone(),
            transient: plan.transient,
            buoyant: plan.buoyant,
            energy_field: "T".to_owned(),
            pressure_field: plan.pressure_field.clone(),
            residual_checks: plan
                .residual_checks
                .iter()
                .map(|(name, limit)| (name.clone(), limit_kind(*limit)))
                .collect(),
            potential: plan.potential,
        }
    }

    fn judge(&self, files: &dyn RunFiles, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
        judge_openfoam(files, study, mode, &self.run_plan(study, 1))
    }

    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &WRITER_SOURCES
    }
}
