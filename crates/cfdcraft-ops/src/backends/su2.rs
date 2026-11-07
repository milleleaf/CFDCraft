//! The rows of the SU2 family.

use super::{Row, rows_of};

/// Every SU2 backend, as registry rows.
pub fn rows() -> Vec<Row> {
    rows_of(cfdcraft_backend_su2::backends())
}
