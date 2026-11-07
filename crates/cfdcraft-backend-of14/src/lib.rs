//! The Foundation OpenFOAM 14 backend: a resolved study becomes a case of that line, written by a
//! writer of its own.
//!
//! The patch table, the mesh dictionary body and the field boundary text are the public items of
//! the older writer; the solver module selection, the numerics, the property files, the heat
//! transfer and turbulence files, the function objects and the run plan are written here. A safety scan holds
//! every written file to the same rules as the older line, an independent reader recovers the
//! written values and compares them with the study, and a setting this writer cannot write yet is a
//! named finding and never dropped.

pub mod backend;
pub mod capabilities;
pub mod control;
pub mod export;
pub mod markers;
pub mod monitors;
pub mod physical;
pub mod plan;
pub mod reader;
pub mod solution;
pub mod thermo;
pub mod turbulence;
pub mod writer;

pub use backend::Of14Backend;

use cfdcraft_backend::Backend;

/// The source files that decide what is written into a case and how it is run: the control,
/// numerics and property writers, the run plan and script, the constants they share, the pieces of
/// the older writer this one calls (named by their path from `src`) and the dictionary editor that
/// applies raw overrides. Their text is part of the tool identity of a stage key, so a change to
/// what the writer emits cannot leave results made before it looking up to date. The safety scan,
/// the reader and the glue are deliberately not in the list.
pub const WRITER_SOURCES: [(&str, &str); 19] = [
    ("control.rs", include_str!("control.rs")),
    ("export.rs", include_str!("export.rs")),
    ("lib.rs", include_str!("lib.rs")),
    ("markers.rs", include_str!("markers.rs")),
    ("monitors.rs", include_str!("monitors.rs")),
    ("physical.rs", include_str!("physical.rs")),
    ("plan.rs", include_str!("plan.rs")),
    ("solution.rs", include_str!("solution.rs")),
    ("thermo.rs", include_str!("thermo.rs")),
    ("turbulence.rs", include_str!("turbulence.rs")),
    ("writer.rs", include_str!("writer.rs")),
    (
        "../../cfdcraft-backend-esi/src/block_mesh.rs",
        include_str!("../../cfdcraft-backend-esi/src/block_mesh.rs"),
    ),
    (
        "../../cfdcraft-backend-esi/src/control_dict.rs",
        include_str!("../../cfdcraft-backend-esi/src/control_dict.rs"),
    ),
    (
        "../../cfdcraft-backend-esi/src/fields.rs",
        include_str!("../../cfdcraft-backend-esi/src/fields.rs"),
    ),
    (
        "../../cfdcraft-backend-esi/src/lib.rs",
        include_str!("../../cfdcraft-backend-esi/src/lib.rs"),
    ),
    (
        "../../cfdcraft-backend-esi/src/monitors.rs",
        include_str!("../../cfdcraft-backend-esi/src/monitors.rs"),
    ),
    (
        "../../cfdcraft-backend-esi/src/patches.rs",
        include_str!("../../cfdcraft-backend-esi/src/patches.rs"),
    ),
    (
        "../../cfdcraft-backend-esi/src/transport.rs",
        include_str!("../../cfdcraft-backend-esi/src/transport.rs"),
    ),
    (
        "../../cfdcraft-dictedit/src/lib.rs",
        include_str!("../../cfdcraft-dictedit/src/lib.rs"),
    ),
];

static OF14: Of14Backend = Of14Backend;

static ROWS: [&dyn Backend; 1] = [&OF14];

/// Every backend this crate offers.
pub fn backends() -> &'static [&'static dyn Backend] {
    &ROWS
}
