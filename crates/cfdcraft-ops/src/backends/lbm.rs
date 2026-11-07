//! The rows of the lattice-Boltzmann family.

use super::{Row, rows_of};

// What the lattice backend claims, for callers that list its benchmark cases.
pub use cfdcraft_backend_lbm::claims::claimed;

/// Every lattice-Boltzmann backend, as registry rows.
pub fn rows() -> Vec<Row> {
    rows_of(cfdcraft_backend_lbm::backends())
}
