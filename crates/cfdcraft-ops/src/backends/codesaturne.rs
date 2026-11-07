//! The rows of the code_saturne family.

use super::{Row, rows_of};

/// Every code_saturne backend, as registry rows.
pub fn rows() -> Vec<Row> {
    rows_of(cfdcraft_backend_codesaturne::backends())
}
