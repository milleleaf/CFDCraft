//! The rows of the OpenCFD family.

use super::{Row, admission_of};

/// Every OpenCFD backend, as registry rows.
pub fn rows() -> Vec<Row> {
    cfdcraft_backend_esi::rows::backends()
        .iter()
        .map(|backend| Row {
            backend: *backend,
            admission: admission_of(backend.id()),
        })
        .collect()
}
