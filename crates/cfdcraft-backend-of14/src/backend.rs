//! The Foundation 14 backend as an implementor of the shared backend trait: every method hands over
//! to the module that does the work.

use crate::{
    WRITER_SOURCES, capabilities::capabilities, export::export_tree, plan::run_plan,
    reader::Of14Reader, writer::compile,
};
use cfdcraft_backend::{
    Backend, CaseTree, CompileFailure, CompileOptions, Family, JudgeMode, JudgeOutcome, Refusal,
    RunFiles, RunPlan,
};
use cfdcraft_backend_esi::{
    adapter::judge_openfoam,
    guard::{self, scan_tree},
    reader::source_files,
    shaped_expected_facts,
};
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy, roundtrip::check_roundtrip_with};

/// Foundation OpenFOAM 14, package pin `20261008`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Of14Backend;

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

impl Backend for Of14Backend {
    fn id(&self) -> &'static str {
        "of14-20261008"
    }

    fn family(&self) -> Family {
        Family::Of14
    }

    /// No image is pinned yet: the case compiles and exports, and running it is refused.
    fn image(&self) -> Option<&'static str> {
        None
    }

    fn preview(&self) -> bool {
        false
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    fn compile(
        &self,
        study: &ResolvedStudy,
        options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure> {
        compile(study, options)
    }

    fn scan(&self, tree: &CaseTree) -> Vec<Refusal> {
        scan_tree(tree).iter().map(neutral_refusal).collect()
    }

    fn read_back(&self, study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic> {
        check_roundtrip_with(
            &Of14Reader,
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
        export_tree(tree, hash, options.np, &run_plan(study, options.np))
    }

    fn run_plan(&self, study: &ResolvedStudy, np: u32) -> RunPlan {
        run_plan(study, np)
    }

    fn judge(&self, files: &dyn RunFiles, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
        judge_openfoam(files, study, mode, &run_plan(study, 1))
    }

    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &WRITER_SOURCES
    }
}
