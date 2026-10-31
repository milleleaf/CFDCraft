//! Solver-neutral, unit-carrying study inputs and resolved SI recipes.

pub mod hash;
pub mod quantity;
pub mod resolve;
pub mod study;
pub mod units;

pub use hash::canonical_hash;
pub use quantity::{Quantity, Unit};
pub use resolve::{ResolveError, ResolvedStudy, resolve};
pub use study::{Study, StudyError};
