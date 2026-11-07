//! The code_saturne backend as an implementor of the shared backend trait. Every method hands over
//! to a module of this crate.

mod judge;
mod results;

use crate::{
    capabilities::{capabilities, check_supported},
    export::export_tree,
    guard::scan_tree,
    mesh::gmsh_text,
    plan::run_plan,
    reader::read_back,
    setup::{MESH_FILE, SETUP_FILE, setup_xml},
};
use cfdcraft_backend::{
    Backend, CaseFile, CaseTree, CompileFailure, CompileOptions, ConvergenceOutcome, Family,
    JudgeMode, JudgeOutcome, Refusal, RunFiles, RunPlan,
};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy};

/// The sources whose text decides what the writer emits.
static WRITER_SOURCES: [(&str, &str); 9] = [
    ("setup.rs", include_str!("setup.rs")),
    ("boundaries.rs", include_str!("boundaries.rs")),
    ("models.rs", include_str!("models.rs")),
    ("probes.rs", include_str!("probes.rs")),
    ("mesh.rs", include_str!("mesh.rs")),
    (
        "../../cfdcraft-blockmesh/src/lib.rs",
        include_str!("../../cfdcraft-blockmesh/src/lib.rs"),
    ),
    (
        "../../cfdcraft-blockmesh/src/grid.rs",
        include_str!("../../cfdcraft-blockmesh/src/grid.rs"),
    ),
    (
        "../../cfdcraft-blockmesh/src/curve.rs",
        include_str!("../../cfdcraft-blockmesh/src/curve.rs"),
    ),
    (
        "../../cfdcraft-blockmesh/src/boundary.rs",
        include_str!("../../cfdcraft-blockmesh/src/boundary.rs"),
    ),
];

/// code_saturne 9.2.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeSaturneBackend;

impl Backend for CodeSaturneBackend {
    fn id(&self) -> &'static str {
        "codesaturne-9.2.0"
    }

    fn family(&self) -> Family {
        Family::CodeSaturne
    }

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
        if options.np == 0 {
            return Err(CompileFailure::Invalid {
                reason: "the number of processes must be at least 1".to_owned(),
            });
        }
        check_supported(study)?;
        let setup = setup_xml(study)?;
        let mesh = gmsh_text(study)?;
        Ok(CaseTree {
            files: vec![
                CaseFile {
                    rel_path: SETUP_FILE.into(),
                    bytes: setup.into_bytes(),
                },
                CaseFile {
                    rel_path: MESH_FILE.into(),
                    bytes: mesh.into_bytes(),
                },
            ],
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

    /// The one mesh file of the setup, with its folder, as a run folder holds it.
    fn mesh_files(&self, _study: &ResolvedStudy) -> Vec<String> {
        vec![MESH_FILE.to_owned()]
    }

    fn judge(&self, files: &dyn RunFiles, study: &ResolvedStudy, mode: JudgeMode) -> JudgeOutcome {
        judge::judge(files, study, mode)
    }

    fn results(&self, files: Box<dyn ResultFiles>, study: &ResolvedStudy) -> Box<dyn ResultFiles> {
        Box::new(results::ResultView::new(files, study))
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
