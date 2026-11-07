//! The lattice-Boltzmann backend as an implementor of the shared backend trait. Every method hands
//! over to the module that does the work.

use crate::{
    capabilities::capabilities, commands, export::export_tree, guard::scan_tree, obstacles, plan,
    reader::read_back,
};
use cfdcraft_backend::{
    Backend, CaseTree, CompileFailure, CompileOptions, ConvergenceOutcome, Family, JudgeMode,
    JudgeOutcome, Refusal, RunFiles, RunPlan,
};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy};

mod judge;
mod results;

/// The code of the notice every result of this backend carries: a preview result is not a gated
/// result. The sentence is the one the text catalogue holds as its preview line, which the
/// registry renders next to this code; this crate holds no wording of its own for it, and no
/// report or view of it claims a gate pass.
pub fn preview_notice() -> &'static str {
    "CFD-BACKEND-009"
}

/// The FieldCraft lattice-Boltzmann solver, run through its headless server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LbmBackend;

impl Backend for LbmBackend {
    fn id(&self) -> &'static str {
        crate::ID
    }

    fn family(&self) -> Family {
        Family::Lbm
    }

    /// No image is pinned yet, so the case compiles and exports and is not run.
    fn image(&self) -> Option<&'static str> {
        None
    }

    fn preview(&self) -> bool {
        true
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    /// `options.np` is not used: the result of the CPU solver does not depend on the number of
    /// threads, and the container's processor limit sets how many it takes.
    fn compile(
        &self,
        study: &ResolvedStudy,
        _options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure> {
        Ok(CaseTree {
            files: commands::files(study)?,
        })
    }

    fn scan(&self, tree: &CaseTree) -> Vec<Refusal> {
        scan_tree(tree)
    }

    fn read_back(&self, study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic> {
        read_back(study, tree)
    }

    fn export(
        &self,
        _study: &ResolvedStudy,
        tree: &CaseTree,
        hash: &str,
        _options: &CompileOptions,
    ) -> CaseTree {
        export_tree(tree, hash)
    }

    fn run_plan(&self, study: &ResolvedStudy, _np: u32) -> RunPlan {
        plan::run_plan(study)
    }

    /// The file of the obstacle surface when the study's lattice has a body, and nothing otherwise.
    fn mesh_files(&self, study: &ResolvedStudy) -> Vec<String> {
        obstacles::lattice(study)
            .ok()
            .and_then(|lattice| lattice.obstacle)
            .map(|body| vec![body.file.to_owned()])
            .unwrap_or_default()
    }

    fn judge(&self, files: &dyn RunFiles, _study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
        judge::judge(files, &mode)
    }

    /// The probe answers under the virtual paths the benchmark comparers read; nothing is written.
    fn results(&self, files: Box<dyn ResultFiles>, study: &ResolvedStudy) -> Box<dyn ResultFiles> {
        Box::new(results::View::new(files, study))
    }

    fn convergence(
        &self,
        files: &dyn RunFiles,
        study: &ResolvedStudy,
    ) -> Option<ConvergenceOutcome> {
        Some(judge::convergence(files, study))
    }

    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &crate::WRITER_SOURCES
    }
}
