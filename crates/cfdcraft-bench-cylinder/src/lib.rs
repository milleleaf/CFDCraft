//! The flow around a cylinder at Reynolds number 100 (Schafer and Turek, benchmark 2D-2) as a
//! benchmark: the published intervals, the allowances around them, the numbers measured from the
//! recorded lift, drag and pressure series, and the comparison.
//!
//! The crate is pure. It reads no file, clock or environment; it reads a run through
//! [`cfdcraft_gate::ResultFiles`].
//!
//! A run is judged only when the lift has shed at least ten whole cycles, steadily, and its
//! coefficient file says the lift is measured along y. Anything else is refused with a stable
//! code, never judged.

mod cylinder;
mod data;
mod files;
mod metrics;

#[doc(hidden)]
pub mod fixture;

pub use cylinder::CylinderBenchmark;
pub use data::{
    Definition, ID, Interval, LoadError, Reference, Source, Tolerances, judge_all, judge_scalar,
};
pub use files::{FORCE, FRONT, REAR};
pub use metrics::{Measured, Refusal, measure};
