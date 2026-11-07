use crate::{
    CaseTree, CompileOptions, ConvergenceOutcome, JudgeMode, JudgeOutcome, Refusal, RunFiles,
    RunPlan,
};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy};

/// The five files of an OpenFOAM mesh, relative to the results folder, in bundle order.
pub const POLY_MESH_FILES: [&str; 5] = [
    "constant/polyMesh/boundary",
    "constant/polyMesh/faces",
    "constant/polyMesh/neighbour",
    "constant/polyMesh/owner",
    "constant/polyMesh/points",
];

/// The solver family of a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// The OpenCFD line of OpenFOAM.
    Esi,
    /// The Foundation line of OpenFOAM, version 14.
    Of14,
    Su2,
    CodeSaturne,
    /// The lattice-Boltzmann preview solver.
    Lbm,
}

impl Family {
    /// Whether the family writes an OpenFOAM case: time folders, `log.*` files and
    /// `postProcessing`.
    pub fn openfoam(self) -> bool {
        matches!(self, Self::Esi | Self::Of14)
    }

    /// The stable name of the family.
    pub fn name(self) -> &'static str {
        match self {
            Self::Esi => "esi",
            Self::Of14 => "of14",
            Self::Su2 => "su2",
            Self::CodeSaturne => "codesaturne",
            Self::Lbm => "lbm",
        }
    }
}

/// Why a backend could not write a case.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompileFailure {
    #[error("the backend does not offer {setting}: {reason}")]
    NotSupported { setting: String, reason: String },
    #[error("{reason}")]
    Invalid { reason: String },
    #[error("raw override {label}: {reason}")]
    Raw { label: String, reason: String },
}

/// One solver backend. The trait is object safe, and a registry row is a `&'static dyn Backend`.
pub trait Backend: Send + Sync {
    /// The stable identifier recorded in a stage key, for example `esi-v2512`.
    fn id(&self) -> &'static str;

    fn family(&self) -> Family;

    /// The image the case runs in, as `repository@sha256:...`; `None` while no image is pinned.
    fn image(&self) -> Option<&'static str>;

    /// Whether results of this backend are previews and not gated results.
    fn preview(&self) -> bool;

    /// What the backend can run. The study is checked against it before anything is compiled.
    fn capabilities(&self) -> Capabilities;

    /// The case files of a resolved study.
    fn compile(
        &self,
        study: &ResolvedStudy,
        options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure>;

    /// Every construct of the written case the backend refuses to run; empty when the case is clean.
    fn scan(&self, tree: &CaseTree) -> Vec<Refusal>;

    /// The values of the written case read back through a reader that does not share the writer's
    /// code and compared with the study; empty when every value matches.
    fn read_back(&self, study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic>;

    /// The case files plus the generated run script and README, for a case written for the
    /// processes in `options`.
    fn export(
        &self,
        study: &ResolvedStudy,
        tree: &CaseTree,
        hash: &str,
        options: &CompileOptions,
    ) -> CaseTree;

    /// How the case is run and what a finished run is judged on.
    fn run_plan(&self, study: &ResolvedStudy, np: u32) -> RunPlan;

    /// Whether a finished run is a good one, judged from the logs and result files it left and
    /// never from the exit status of the process. A backend that has no evidence rules of its own
    /// can never pass a run, so the default fails.
    fn judge(
        &self,
        _files: &dyn RunFiles,
        _study: &ResolvedStudy,
        _mode: JudgeMode,
    ) -> JudgeOutcome {
        JudgeOutcome::failed("the backend has no evidence rules, so it cannot judge a run")
    }

    /// The view of the result files the benchmark comparers read. The default is the files as the
    /// solver wrote them; a backend that writes another layout maps it to the one comparers know.
    fn results(&self, files: Box<dyn ResultFiles>, _study: &ResolvedStudy) -> Box<dyn ResultFiles> {
        files
    }

    /// Whether the run converged, when the backend judges that itself. `None` means the generic
    /// OpenFOAM assessment applies.
    fn convergence(
        &self,
        _files: &dyn RunFiles,
        _study: &ResolvedStudy,
    ) -> Option<ConvergenceOutcome> {
        None
    }

    /// The files, relative to the results folder, that stand for the mesh a run was made on. The
    /// default is the five files of an OpenFOAM mesh; a backend with another mesh file names its
    /// own, and one with none returns an empty list.
    fn mesh_files(&self, _study: &ResolvedStudy) -> Vec<String> {
        POLY_MESH_FILES
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    }

    /// The source files that decide what the writer emits, as (name, text) pairs. Their text is
    /// part of the tool identity of a stage key.
    fn writer_sources(&self) -> &'static [(&'static str, &'static str)];
}
