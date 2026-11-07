//! The tool identity of a stage key, taken from a backend's own image and writer sources.

use crate::keys::writer_version_of;
use cfdcraft_backend::Backend;
use cfdcraft_keys::ToolIdentity;

/// The digest a stage key records for a backend whose image is not pinned.
const UNPINNED: &str = "unpinned";

/// The tools that make a case in `backend`: its image digest (the text after `@`, or `unpinned`),
/// its identifier, no geometry sidecar, and the version of this build's case writer over the
/// backend's own writer sources.
pub fn tool_identity_for(backend: &dyn Backend) -> ToolIdentity {
    let digest = backend.image().map_or(UNPINNED, |image| {
        image.split_once('@').map_or(image, |(_, digest)| digest)
    });
    let writer = writer_version_of(env!("CARGO_PKG_VERSION"), backend.writer_sources());
    ToolIdentity::new(digest, backend.id(), None, writer)
}
