//! Whether a monitored quantity has settled, and whether a run is converged.
//!
//! The crate is pure: it takes numbers and returns numbers and words. The same formulas run
//! while a run is watched and when its verdict is recorded, so the figures a person sees while
//! waiting are the figures that decide.

mod balance;
mod series;
mod settings;
mod verdict;

pub use balance::{Balance, BalanceStatus, PatchFlux, energy_balance, mass_balance};
pub use series::{QuantityState, SampleKind, Samples, Status, assess_series};
pub use settings::{Settings, SettleError};
pub use verdict::{NOT_FINISHED, SOLVER_FAILED, Verdict, VerdictState, verdict};
