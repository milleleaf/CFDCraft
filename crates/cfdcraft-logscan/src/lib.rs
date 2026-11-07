//! Reads an OpenFOAM solver log without touching a disk, a clock or a process.
//!
//! The caller owns the log. It passes text in whatever pieces it has: the whole log after a
//! run, or a few seconds of new output during one. The scanner waits for the end of a half-read
//! line, so the pieces never change the answer. Everything here is a pure function of the text
//! it is given.

mod divergence;
mod explain;
mod redact;
mod scan;

pub use divergence::{Divergence, DivergenceCode, Limits, divergence};
pub use explain::{ErrorId, explain};
pub use redact::{MAX_LINE_CHARS, redact, redact_sentence, tail_lines};
pub use scan::{Event, FatalKind, LOCAL_WINDOW, LogState, MAX_FIELDS, MAX_LINE, Scanner};
