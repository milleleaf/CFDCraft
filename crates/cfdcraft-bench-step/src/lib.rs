//! The backward-facing step benchmarks: the turbulent step of Driver and Seegmiller and the
//! laminar step of Armaly and others, with their reference numbers and pure comparers.
//!
//! The crate reads no clock and no environment. A comparison receives the files of a finished run
//! through [`cfdcraft_gate::ResultFiles`] and returns a [`cfdcraft_gate::GateReport`].

mod armaly;
mod data;
mod ds;
#[doc(hidden)]
pub mod fixture;
mod lines;
mod reattach;

pub use armaly::{ArmalyBenchmark, ID as ARMALY_ID, reynolds_number};
pub use data::{
    ArmalyCurve, ArmalyGeometry, ArmalyReference, ArmalySource, ArmalyTolerance, DsGeometry,
    DsProfiles, DsReattachment, DsReference, DsSource, DsTolerance, LoadError, STATIONS, Station,
    Tolerances,
};
pub use ds::{ID as STEP_ID, StepBenchmark};
pub use lines::FLOOR;
pub use reattach::reattachment_length;
