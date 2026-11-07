//! The FieldCraft lattice-Boltzmann solver as a preview backend. A study becomes a stream of
//! commands, one JSON message per line, for the solver's headless server; a guard admits only
//! commands on a reviewed list; results come back as the server's replies. The backend is always a
//! preview, and its list of claimed benchmarks is empty until a gate pass is recorded.

pub mod backend;
pub mod capabilities;
pub mod claims;
pub mod commands;
pub mod export;
pub mod guard;
pub mod markers;
pub mod obstacles;
pub mod plan;
pub mod reader;
pub mod thermal;
pub mod threemf;

use backend::LbmBackend;
use cfdcraft_backend::Backend;

/// The stable identifier recorded in a stage key.
pub const ID: &str = "fieldcraft-lbm-0.7.5";

/// The source files that decide what the writer emits. Their text is part of the tool identity of
/// a stage key.
pub static WRITER_SOURCES: [(&str, &str); 7] = [
    ("commands.rs", include_str!("commands.rs")),
    ("export.rs", include_str!("export.rs")),
    ("markers.rs", include_str!("markers.rs")),
    ("obstacles.rs", include_str!("obstacles.rs")),
    ("plan.rs", include_str!("plan.rs")),
    ("thermal.rs", include_str!("thermal.rs")),
    ("threemf.rs", include_str!("threemf.rs")),
];

static ROW: LbmBackend = LbmBackend;

static ROWS: [&dyn Backend; 1] = [&ROW];

/// Every backend this crate offers.
pub fn backends() -> &'static [&'static dyn Backend] {
    &ROWS
}
