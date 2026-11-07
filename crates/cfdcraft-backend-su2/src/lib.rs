//! The SU2 backend: a resolved study becomes a configuration, a native mesh and a run script.
//!
//! The case is written from typed fields only. A safety scan holds every key and every value to an
//! allowlist, an independent reader recovers the written values and compares them with the study,
//! and a setting SU2 does not offer is a named finding and never dropped.

pub mod backend;
pub mod capabilities;
pub mod config;
pub mod export;
pub mod guard;
pub mod inlet;
pub mod markers;
pub mod mesh;
pub mod plan;
pub mod reader;

pub use backend::Su2Backend;

use cfdcraft_backend::Backend;

/// The source files that decide what is written into a case and how it is run: the configuration
/// and mesh writers, the profile writer, the run plan and script, the constants they share, and the
/// mesh generator (named by its path from `src`). Their text is part of the tool identity of a stage
/// key, so a change to what the writer emits cannot leave results made before it looking up to
/// date. The safety scan, the reader and the glue are deliberately not in the list.
pub const WRITER_SOURCES: [(&str, &str); 11] = [
    ("config.rs", include_str!("config.rs")),
    ("export.rs", include_str!("export.rs")),
    ("inlet.rs", include_str!("inlet.rs")),
    ("lib.rs", include_str!("lib.rs")),
    ("markers.rs", include_str!("markers.rs")),
    ("mesh.rs", include_str!("mesh.rs")),
    ("plan.rs", include_str!("plan.rs")),
    (
        "../../cfdcraft-blockmesh/src/boundary.rs",
        include_str!("../../cfdcraft-blockmesh/src/boundary.rs"),
    ),
    (
        "../../cfdcraft-blockmesh/src/curve.rs",
        include_str!("../../cfdcraft-blockmesh/src/curve.rs"),
    ),
    (
        "../../cfdcraft-blockmesh/src/grid.rs",
        include_str!("../../cfdcraft-blockmesh/src/grid.rs"),
    ),
    (
        "../../cfdcraft-blockmesh/src/lib.rs",
        include_str!("../../cfdcraft-blockmesh/src/lib.rs"),
    ),
];

static SU2: Su2Backend = Su2Backend;

static ROWS: [&dyn Backend; 1] = [&SU2];

/// Every backend this crate offers.
pub fn backends() -> &'static [&'static dyn Backend] {
    &ROWS
}
