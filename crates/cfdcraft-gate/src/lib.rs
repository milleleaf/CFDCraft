//! The verdict of a benchmark comparison, as data: every checked quantity with the computed value,
//! the published value or range, the tolerance that was used and the verdict, side by side.
//!
//! The crate is pure. It reads no file, clock or environment; a comparison receives its input
//! through small traits and returns a [`GateReport`].

mod bench;
mod render;
mod report;

pub use bench::{Benchmark, Mismatch, ResultFiles, bound_override};
pub use render::render_report;
pub use report::{GHIA, GateConvergence, GateMetric, GateReport, GateRow, MetricKind};
