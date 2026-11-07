//! The list of registered solver backends: id, family, admission state, image, whether its
//! results are a preview, and what a person should know about the image. The list is the registry
//! as it stands; nothing here runs a solver or reads a file.
//!
//! The sentences for a person are `BackendsData::lines`, built from the `Backend` lines of the
//! text crate. The report names no host and no directory.

use crate::backends::rows;
use crate::report::Report;
use crate::repro::{BackendEntry, BackendsData, IMAGE_NOT_PINNED, Repro};
use cfdcraft_backend::Family;
use cfdcraft_backend_esi::dialect::{Dialect, IMAGE_NOTE_V2606};

const LIST: &str = "backend-list";

/// The note for the image of a backend, empty when there is none to give.
fn note_for(id: &str) -> &'static str {
    if id == Dialect::EsiV2606.id() {
        IMAGE_NOTE_V2606
    } else {
        ""
    }
}

/// Every registered backend, in registry order with the default first.
///
/// `claims` counts the benchmark cases the preview backend is claimed for; it is zero for every
/// other family. A preview backend is labelled in the data (`preview`) and in the sentences.
pub fn backend_list() -> Report {
    let backends = rows()
        .iter()
        .map(|row| {
            let backend = row.backend;
            let claims = if backend.family() == Family::Lbm {
                u32::try_from(cfdcraft_backend_lbm::claims::CLAIMS.len()).unwrap_or(u32::MAX)
            } else {
                0
            };
            BackendEntry {
                id: backend.id().to_owned(),
                family: backend.family().name().to_owned(),
                admission: row.admission.as_str().to_owned(),
                image: backend
                    .image()
                    .map_or_else(|| IMAGE_NOT_PINNED.to_owned(), str::to_owned),
                preview: backend.preview(),
                claims,
                note: note_for(backend.id()).to_owned(),
            }
        })
        .collect();
    Report::new(LIST).with_repro(Repro::Backends(Box::new(BackendsData { backends })))
}
