//! The table of known solver errors: the end of a failed run's log in, a stable error id out.
//!
//! The patterns are tried in the order of [`ErrorId`] and the first match wins. The start-up lines
//! that say floating-point trapping is on are never read as an error. A tail that matches nothing
//! is [`ErrorId::Unknown`]; the caller then shows a redacted tail (see `redact`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::scan::is_banner;

/// A known solver error. The order is the order the patterns are tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorId {
    /// A file the case needs could not be opened.
    MissingFile,
    /// The solver died on an invalid number.
    NumericalBlowup,
    /// A boundary, function or other type name is not known to the solver.
    UnknownType,
    /// A dictionary lacks a required entry.
    MissingKeyword,
    /// A linear solver ran out of iterations.
    LinearSolverNotConverged,
    /// The mesh failed its checks.
    PoorMesh,
    /// The container ran out of memory.
    OutOfMemory,
    /// The disk is full.
    DiskFull,
    /// The decomposition does not match the processor folders.
    DecompositionMismatch,
    /// The parallel launcher found too few cores.
    NotEnoughSlots,
    /// The solver crashed.
    SegmentationFault,
    /// Nothing in the table matched.
    Unknown,
}

impl ErrorId {
    /// The stable finding code.
    pub fn code(self) -> &'static str {
        match self {
            Self::MissingFile => "CFD-SOLVER-001",
            Self::NumericalBlowup => "CFD-SOLVER-002",
            Self::UnknownType => "CFD-SOLVER-003",
            Self::MissingKeyword => "CFD-SOLVER-004",
            Self::LinearSolverNotConverged => "CFD-SOLVER-005",
            Self::PoorMesh => "CFD-SOLVER-006",
            Self::OutOfMemory => "CFD-SOLVER-007",
            Self::DiskFull => "CFD-SOLVER-008",
            Self::DecompositionMismatch => "CFD-SOLVER-009",
            Self::NotEnoughSlots => "CFD-SOLVER-010",
            Self::SegmentationFault => "CFD-SOLVER-011",
            Self::Unknown => "CFD-SOLVER-099",
        }
    }
}

/// Which known error the end of a log shows. The first pattern in the table that matches wins.
pub fn explain(tail: &str) -> ErrorId {
    let kept: Vec<&str> = tail.lines().filter(|line| !is_banner(line)).collect();
    let text = kept.join("\n");
    let has = |needle: &str| text.contains(needle);
    let lower = text.to_ascii_lowercase();

    if has("FOAM FATAL IO ERROR") && (has("Could not open") || has("cannot open file")) {
        ErrorId::MissingFile
    } else if has("Floating point exception")
        || has("sigFpe")
        || has("Foam::sigFpe::sigHandler")
        || has("signal SIGFPE")
    {
        ErrorId::NumericalBlowup
    } else if has("Unknown patchField type")
        || has("Unknown function type")
        || has("Unknown functionObject type")
        || has("Unknown boundaryField")
    {
        ErrorId::UnknownType
    } else if has("keyword") && has("is undefined in dictionary") {
        ErrorId::MissingKeyword
    } else if has("Maximum number of iterations exceeded") {
        ErrorId::LinearSolverNotConverged
    } else if (has("Failed") && has("mesh checks")) || has("***") {
        ErrorId::PoorMesh
    } else if has("std::bad_alloc") || has("Cannot allocate memory") {
        ErrorId::OutOfMemory
    } else if has("No space left on device") {
        ErrorId::DiskFull
    } else if has("Wrong number of processor directories") || lower.contains("number of processors")
    {
        ErrorId::DecompositionMismatch
    } else if has("There are not enough slots available") || has("mpirun was unable to start") {
        ErrorId::NotEnoughSlots
    } else if has("Segmentation fault") {
        ErrorId::SegmentationFault
    } else {
        ErrorId::Unknown
    }
}
