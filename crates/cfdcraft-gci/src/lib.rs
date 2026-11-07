//! Grid convergence index of three systematically refined meshes (Celik et al., 2008).
//!
//! The crate is pure: it takes the cell counts, the domain area and one number per mesh, and
//! returns the observed order, the extrapolated value, the fine-mesh index, the asymptotic-range
//! ratio and a convergence class. It reads no file, no clock and no environment, so a sweep, a
//! command line and a tool server cannot disagree about what the order and the index are.

mod report;
mod solve;

pub use report::{
    Convergence, GciError, GciReport, Level, Options, SAFETY_FACTOR, Warning, analyse,
    analyse_series,
};
pub use solve::observed_order;
