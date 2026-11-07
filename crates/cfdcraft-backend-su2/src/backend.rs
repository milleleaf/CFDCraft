//! The SU2 backend as an implementor of the shared backend trait: every method hands over to the
//! module that does the work.

use crate::{
    WRITER_SOURCES, capabilities::capabilities, config::compile, export::export_tree,
    guard::scan_tree, markers::MESH_FILE, plan::run_plan, reader::read_back,
};
use cfdcraft_backend::{
    Backend, CaseTree, CompileFailure, CompileOptions, ConvergenceOutcome, Family, JudgeMode,
    JudgeOutcome, Refusal, RunFiles, RunPlan,
};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy};

mod judge;
mod native;
mod results;

/// SU2 8.6.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Su2Backend;

impl Backend for Su2Backend {
    fn id(&self) -> &'static str {
        "su2-8.6.0"
    }

    fn family(&self) -> Family {
        Family::Su2
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
        _options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure> {
        compile(study)
    }

    fn scan(&self, tree: &CaseTree) -> Vec<Refusal> {
        scan_tree(tree)
    }

    fn read_back(&self, study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic> {
        read_back(study, tree)
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

    /// The mesh file the writer makes: the solver reads its mesh from it.
    fn mesh_files(&self, _study: &ResolvedStudy) -> Vec<String> {
        vec![MESH_FILE.to_owned()]
    }

    fn judge(&self, files: &dyn RunFiles, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
        judge::judge(files, study, mode)
    }

    fn results(&self, files: Box<dyn ResultFiles>, study: &ResolvedStudy) -> Box<dyn ResultFiles> {
        Box::new(results::NativeView::owned(files, study))
    }

    fn convergence(
        &self,
        files: &dyn RunFiles,
        study: &ResolvedStudy,
    ) -> Option<ConvergenceOutcome> {
        Some(judge::convergence(files, study))
    }

    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &WRITER_SOURCES
    }
}
