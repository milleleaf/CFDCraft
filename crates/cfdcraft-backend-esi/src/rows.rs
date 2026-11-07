//! The backends of this crate, as rows of a registry.

use crate::{Dialect, adapter::EsiBackend};
use cfdcraft_backend::Backend;

static V2512: EsiBackend = EsiBackend::new(Dialect::EsiV2512);

static V2606: EsiBackend = EsiBackend::new(Dialect::EsiV2606);

static ROWS: [&dyn Backend; 2] = [&V2512, &V2606];

/// Every backend this crate offers, the default one first.
pub fn backends() -> &'static [&'static dyn Backend] {
    &ROWS
}
