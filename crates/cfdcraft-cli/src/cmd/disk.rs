//! The verbs for the stored results of a study file: how much each variant holds, and removing
//! results on purpose. Pruning is a dry run unless it is applied.

use std::path::Path;

use cfdcraft_ops::disk_ops::{disk_use, prune};

use super::{Error, emit_data, print_data};

/// The bytes each variant holds in the store.
pub fn disk(doc: &Path, json: bool) -> Result<i32, Error> {
    print_data(&disk_use(doc), json)
}

/// Show what pruning would remove, and with `apply` remove it. `scenarios` limits the prune to
/// those variants; empty means every variant.
pub fn prune_results(
    doc: &Path,
    scenarios: &[String],
    apply: bool,
    json: bool,
) -> Result<i32, Error> {
    let scope = (!scenarios.is_empty()).then_some(scenarios);
    emit_data(&prune(doc, scope, apply), json)
}
