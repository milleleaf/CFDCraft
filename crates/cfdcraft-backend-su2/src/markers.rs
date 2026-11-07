//! Names, file names and facts about SU2 that the writer relies on.
//!
//! Two kinds of entry live here. Entries marked `UNPROBED` are believed but have not been shown by a
//! real run yet; a hosted probe checks each one, and a mismatch there is a reviewed change of the
//! constant. Entries marked `READ FROM SOURCE` were read in the SU2 8.6.0 sources (reading is
//! allowed, nothing is copied) and are fixed facts of that release.

/// The configuration file of a case.
pub const CONFIG_FILE: &str = "config.cfg";
/// The mesh file of a case.
pub const MESH_FILE: &str = "mesh.su2";
/// The inlet profile file of a case. Only a study with a parabolic inlet writes it.
pub const INLET_FILE: &str = "inlet.dat";
/// The log the solver's output is written to.
pub const SOLVER_LOG: &str = "log.SU2";
/// The solver program.
pub const SOLVER: &str = "SU2_CFD";

/// UNPROBED: the spelling of the flag that sets the number of threads of the OpenMP build.
pub const THREAD_FLAG: &str = "-t";
/// UNPROBED: the line a finished run ends its output with.
pub const END_BANNER: &str = "Exit Success (SU2_CFD)";
/// The name the history file is written under, before the format adds its extension.
pub const HISTORY_STEM: &str = "history";
/// UNPROBED: the history file the solver writes for the stem above with tabular format CSV.
pub const HISTORY_FILE: &str = "history.csv";
/// The name the volume solution is written under, before the format adds its extension.
pub const VOLUME_STEM: &str = "flow";
/// UNPROBED: the volume solution file for the stem above with the CSV output file type.
pub const VOLUME_FILE: &str = "flow.csv";
/// UNPROBED: the history column groups of a steady run.
pub const HISTORY_GROUPS_STEADY: &str = "ITER, RMS_RES";
/// UNPROBED: the history column groups of a transient run.
pub const HISTORY_GROUPS_TRANSIENT: &str = "TIME_ITER, CUR_TIME, RMS_RES";
/// UNPROBED: the screen columns of a steady run.
pub const SCREEN_STEADY: &str = "INNER_ITER, RMS_PRESSURE, RMS_VELOCITY-X, RMS_VELOCITY-Y";
/// UNPROBED: the screen columns of a transient run.
pub const SCREEN_TRANSIENT: &str =
    "TIME_ITER, INNER_ITER, RMS_PRESSURE, RMS_VELOCITY-X, RMS_VELOCITY-Y";
/// UNPROBED: the residual fields that decide whether a steady run has converged.
pub const CONVERGENCE_FIELDS: &str = "RMS_PRESSURE, RMS_VELOCITY-X, RMS_VELOCITY-Y";
/// UNPROBED: the residual field added when the energy equation runs.
pub const CONVERGENCE_FIELD_ENERGY: &str = "RMS_ENTHALPY";
/// UNPROBED: the number of inner iterations of every time step; chosen, not measured.
pub const INNER_ITERATIONS: u64 = 50;
/// UNPROBED: the CFL numbers of a steady and of a transient run; starting values, not tuned.
pub const CFL_STEADY: f64 = 50.0;
/// See `CFL_STEADY`.
pub const CFL_TRANSIENT: f64 = 200.0;

/// UNPROBED: the temperature the incompressible solver starts from when no buoyancy gives one. With
/// the energy equation off it changes nothing.
pub const NEUTRAL_TEMPERATURE: f64 = 300.0;

/// READ FROM SOURCE (the Boussinesq source term of the incompressible solver, `flow_sources.cpp`):
/// gravity is fixed to this value and points down the y axis in two dimensions. The case cannot set
/// it. A study's gravity enters through the thermal expansion coefficient instead, because the
/// source is proportional to their product.
pub const STANDARD_GRAVITY: f64 = 9.80665;

/// The most a judged or presented native file is read from, in bytes. A longer file is read only up
/// to it and the report says so.
pub const READ_CAP_BYTES: u64 = 8 * 1024 * 1024;

/// UNPROBED: the text the solver prints when a steady run stops early on its residual limit.
pub const CONVERGED_MESSAGE: &str = "All convergence criteria satisfied.";
/// UNPROBED: the text the solver prints when the flow solution has diverged.
pub const DIVERGED_MESSAGE: &str = "SU2 has diverged";

/// UNPROBED: the history column that holds the residual of the pressure, as a base-10 logarithm.
pub const HISTORY_RESIDUAL_P: &str = "rms[P]";
/// UNPROBED: the history column of the residual of the x velocity, as a base-10 logarithm.
pub const HISTORY_RESIDUAL_U: &str = "rms[U]";
/// UNPROBED: the history column of the residual of the y velocity, as a base-10 logarithm.
pub const HISTORY_RESIDUAL_V: &str = "rms[V]";
/// UNPROBED: the history column of the physical time of a time step.
pub const HISTORY_TIME: &str = "Cur_Time";
/// UNPROBED: the history columns of the drag and lift coefficients of the monitored boundary. A
/// run has them only when the case asks for them and normalises with the study's own reference
/// speed and length; the view never rescales them.
pub const HISTORY_DRAG: &str = "CD";
/// See `HISTORY_DRAG`.
pub const HISTORY_LIFT: &str = "CL";

/// UNPROBED: the volume file columns of the node coordinates.
pub const VOLUME_X: &str = "x";
/// See `VOLUME_X`.
pub const VOLUME_Y: &str = "y";
/// UNPROBED: the volume file columns of the velocity components.
pub const VOLUME_VELOCITY_X: &str = "Velocity_x";
/// See `VOLUME_VELOCITY_X`.
pub const VOLUME_VELOCITY_Y: &str = "Velocity_y";
/// UNPROBED: the volume file column of the temperature, present when the energy equation runs.
pub const VOLUME_TEMPERATURE: &str = "Temperature";
