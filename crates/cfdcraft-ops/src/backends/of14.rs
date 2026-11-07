//! The rows of the Foundation 14 family.

use super::{Row, rows_of};

/// Every Foundation 14 backend, as registry rows.
pub fn rows() -> Vec<Row> {
    rows_of(cfdcraft_backend_of14::backends())
}
