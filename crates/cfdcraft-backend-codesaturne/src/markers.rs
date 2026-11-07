//! Every name of the solver that this crate has not yet seen on a real run, one constant each.
//!
//! The names come from the solver's published sources and documentation. Each is marked
//! `UNPROBED` until a run of the solver has shown it, and a mismatch found then is a change of
//! one constant here and nowhere else.

/// UNPROBED: the attribute that names a boundary zone in the setup. The sources show `label` as
/// the zone name the solver looks zones up by and `name` as the zone's code number.
pub const ZONE_NAME_ATTRIBUTE: &str = "label";

/// UNPROBED: the attribute that holds a boundary zone's code number.
pub const ZONE_NUMBER_ATTRIBUTE: &str = "name";

/// UNPROBED: the subcommand that runs a case.
pub const RUN_SUBCOMMAND: &str = "run";

/// UNPROBED: the flag that names the parameter file.
pub const RUN_PARAM_FLAG: &str = "--param";

/// UNPROBED: the flag that names the case folder.
pub const RUN_CASE_FLAG: &str = "--case";

/// UNPROBED: the flag that names the run.
pub const RUN_ID_FLAG: &str = "--id";

/// UNPROBED: the flag that sets the number of processes.
pub const RUN_PROCESSES_FLAG: &str = "-n";

/// The name every run of a case is given, so its results are in a known folder.
pub const RUN_ID: &str = "run";

/// UNPROBED: the folder the solver writes a run's results into, relative to the case folder.
pub const RESULTS_DIR: &str = "RESU/run";

/// UNPROBED: the residual history of a run, relative to the case folder.
pub const RESIDUALS_FILE: &str = "RESU/run/residuals.csv";

/// UNPROBED: the solver's own log of a run, relative to the case folder.
pub const SOLVER_LOG_FILE: &str = "RESU/run/run_solver.log";

/// UNPROBED: the start of the name of the file the solver leaves in the results folder when a run
/// fails.
pub const ERROR_FILE_PREFIX: &str = "error";

/// UNPROBED: the folder of probe histories inside the results folder.
pub const PROBES_DIR: &str = "RESU/run/monitoring";

/// UNPROBED: the history of the velocity at every monitoring point, relative to the case folder.
/// The solver's time plot writer names a file after the probe set (`probes`) and the field, and
/// the CSV format was asked for in the setup; the name was read from the writer sources, not seen
/// on a run.
pub const PROBES_VELOCITY_FILE: &str = "RESU/run/monitoring/probes_velocity.csv";

/// UNPROBED: the history of the pressure at every monitoring point, relative to the case folder.
/// Named the same way as [`PROBES_VELOCITY_FILE`].
pub const PROBES_PRESSURE_FILE: &str = "RESU/run/monitoring/probes_pressure.csv";

/// The log this crate's run step writes the solver's console output to.
pub const RUN_LOG: &str = "log.code_saturne";

/// The most any file of a run is read up to, 8 MiB. A longer file is read from its end (a log) or
/// not at all (a monitoring history, which would otherwise give a partial series), and a report
/// says so.
pub const READ_CAP_BYTES: u64 = 8 * 1024 * 1024;

/// UNPROBED: the text the solver log holds once the run has reached its end. Read from the
/// solver's sources, not seen on a run.
pub const COMPLETION_MARKER: &str = "Finished computation";

/// UNPROBED: the word a column of the residual history holds when it belongs to the pressure,
/// matched without regard to case. The columns are found by name, not by position.
pub const RESIDUAL_PRESSURE_WORD: &str = "pressure";

/// UNPROBED: the word a column of the residual history holds when it belongs to a velocity
/// component, matched without regard to case.
pub const RESIDUAL_VELOCITY_WORD: &str = "velocity";

/// UNPROBED: the number of columns before the first value of a row of a monitoring history. The
/// first column is the iteration or the time.
pub const MONITOR_TIME_COLUMNS: usize = 1;

/// UNPROBED: the number of columns a velocity takes for each monitoring point, in point order.
pub const MONITOR_VELOCITY_COLUMNS: usize = 3;

/// UNPROBED: the number of columns a pressure takes for each monitoring point, in point order.
pub const MONITOR_PRESSURE_COLUMNS: usize = 1;
