//! The code_saturne backend: a case written from a resolved study as a setup file, a Gmsh mesh
//! and a run script.
//!
//! The case is built from constants only. The solver compiles any formula typed into a setup and
//! runs any user source or script placed in a case, so none is ever written and the scan refuses
//! all of them.

pub mod backend;
pub mod boundaries;
pub mod capabilities;
pub mod export;
pub mod guard;
pub mod markers;
pub mod mesh;
pub mod models;
pub mod plan;
pub mod probes;
pub mod reader;
pub mod setup;
pub mod xml;

pub use backend::CodeSaturneBackend;

use cfdcraft_backend::Backend;

static CODE_SATURNE: CodeSaturneBackend = CodeSaturneBackend;

static ROWS: [&dyn Backend; 1] = [&CODE_SATURNE];

/// Every backend this crate offers.
pub fn backends() -> &'static [&'static dyn Backend] {
    &ROWS
}
