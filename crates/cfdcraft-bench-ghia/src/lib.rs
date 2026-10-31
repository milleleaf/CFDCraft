//! Re=100 cavity reference comparison in normalized lid velocities.

pub mod compare;
pub mod interp;

pub use compare::{Comparison, GhiaTable, LoadError, Row, Tolerances, render_table};
pub use interp::{InterpError, Profile, linear_sample};
