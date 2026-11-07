//! The differentially heated square cavity against the bench mark solution of de Vahl Davis
//! (1983), Table V, at Rayleigh numbers 1e3 to 1e6.
//!
//! A study is this benchmark when it is a closed square cavity with a hot left wall and a cold
//! right wall, insulated above and below, with Boussinesq buoyancy at Prandtl number 0.71 and a
//! Rayleigh number that is one of the four tabulated. The Rayleigh number is derived from the
//! study itself. A run is judged by the average Nusselt number from the hot-wall heat flux and by
//! the largest horizontal velocity on the vertical mid-plane and the largest vertical velocity on
//! the horizontal mid-plane, each with its position.

mod data;
mod dvd;
mod files;
mod metrics;

#[doc(hidden)]
pub mod fixture;

pub use data::{
    LoadError, OVERRIDE_FACTOR, PRANDTL, RA_MATCH, Row, Source, TOP_RA, Table, Tolerances,
    judge_position, judge_relative, rayleigh,
};
pub use dvd::DvdBenchmark;
pub use files::{Refusal, read_line, read_surface};
pub use metrics::{Measured, Setup, measure, measure_with, peak};
