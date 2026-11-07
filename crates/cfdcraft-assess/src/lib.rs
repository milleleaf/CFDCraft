//! One pure function from a solver log and the monitor files to the numbers a person sees and the verdict on the run.
//!
//! The live view and the verdict recorded at the end of a run both call [`assess`] over the same
//! bytes, so the table a person watches and the word that is stored cannot disagree. Nothing here
//! reads a disk, a clock or a process: the caller passes the text in.

mod assess;
mod spec;

pub use assess::{Assessment, ResidualRow, assess};
pub use spec::{Files, LineSpec, PatchSpec, ProbeSpec, Spec, SurfaceQuantityKind, SurfaceSpec};
