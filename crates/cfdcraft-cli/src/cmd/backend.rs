//! The verb that lists the registered solver backends: its id, family, admission state and image,
//! with a preview label and the candidate and pinning notices where they apply. The sentences are
//! those of the operation's report; nothing here needs a compute host.

use cfdcraft_ops::{Repro, backend_list};

use super::{Error, emit_lines};

/// Print the backends, one sentence each and the notices that follow it, in registry order.
pub fn list(json: bool) -> Result<i32, Error> {
    Ok(emit_lines(&backend_list(), json, |data| match data {
        Repro::Backends(backends) => backends.lines(),
        _ => Vec::new(),
    }))
}
