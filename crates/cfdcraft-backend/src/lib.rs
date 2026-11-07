//! What a solver backend offers, in terms that name no solver. A backend states what it can run,
//! writes a case from a resolved study, scans and reads back what it wrote, and says how the case
//! is run. The code that drives a run asks a backend for these and never matches on a dialect, so
//! one more solver is one more implementor and not another branch somewhere else.

pub mod backend;
pub mod judge;
pub mod options;
pub mod plan;
pub mod refusal;
pub mod tree;

pub use backend::{Backend, CompileFailure, Family};
pub use judge::{ConvergenceOutcome, DirRunFiles, JudgeMode, JudgeOutcome, RunFiles};
pub use options::CompileOptions;
pub use plan::{LimitKind, RunPlan, RunStep};
pub use refusal::Refusal;
pub use tree::{CaseFile, CaseTree};
