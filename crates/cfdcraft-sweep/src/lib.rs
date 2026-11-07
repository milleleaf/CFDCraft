//! Parameter sweeps as pure data.
//!
//! A sweep is a small definition: a base and some parameters, each with a unit and the values to
//! try. It expands into points, and each point is a list of settings to change (a pointer into a
//! study and the value to put there). This crate knows nothing of studies: applying a point and
//! judging the result belong to the code that uses it. Every surface expands a sweep through this
//! one crate, so a sweep reads the same everywhere.

mod points;
mod values;

pub use points::{
    Assignment, MAX_PARAMETERS, MAX_SWEEP_POINTS, Parameter, Point, Sweep, SweepError, Target,
    check_sweep, point_count, points,
};
pub use values::{MAX_VALUES, Values, expand};
