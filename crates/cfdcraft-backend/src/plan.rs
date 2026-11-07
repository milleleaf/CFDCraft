/// Which limit of the study a residual is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// The pressure limit of the study.
    Pressure,
    /// The velocity limit of the study, also used for the temperature.
    Velocity,
}

/// One command of a run and the log it writes its output to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStep {
    pub command: String,
    /// The log file name, relative to the case root.
    pub log: String,
}

/// How a case is run and what a finished run is judged on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    /// The solver program.
    pub solver: String,
    /// The commands in the order they run.
    pub steps: Vec<RunStep>,
    /// The log the solver writes.
    pub solver_log: String,
    /// The logs a run is judged on, in the order they are written.
    pub judged_logs: Vec<String>,
    /// The result files that must exist and not be empty when the run has finished. A backend
    /// that does not write fields names its final result files here.
    pub final_fields: Vec<String>,
    pub transient: bool,
    pub buoyant: bool,
    /// The field whose residual gates a buoyant steady run: `T` for the ESI line, `h` for the
    /// Foundation fluid module.
    pub energy_field: String,
    /// The pressure field the solver solves for; empty when the backend has none.
    pub pressure_field: String,
    /// The residuals whose last value gates a steady run, each with the limit it is held to.
    pub residual_checks: Vec<(String, LimitKind)>,
    /// Whether a potential-flow step runs before the solver.
    pub potential: bool,
}
