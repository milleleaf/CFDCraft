//! The verdict on a run.
//!
//! The word "converged" cannot be built from residuals alone, and [`verdict`] is the only place
//! it is decided: a run is converged only when at least one quantity is monitored, every
//! monitored quantity has settled, the balances are ok or not applicable, and the residual
//! check holds. A run with no monitored quantity is not judged, whatever its residuals say.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::balance::{Balance, BalanceStatus};
use crate::series::{QuantityState, Status};
use crate::settings::Settings;

/// The reason when the log of the solver shows an error it printed or a number that is not finite.
pub const SOLVER_FAILED: &str = "solver_failed";
/// The reason when the run did not go to its end: it failed, was stopped or was lost.
pub const NOT_FINISHED: &str = "not_finished";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerdictState {
    Converged,
    NotConverged,
    NotJudged,
}

/// The verdict with everything it was made from.
///
/// `reasons` are stable words: `no_monitored_quantity`, `not_settled:<name>`, `mass_imbalance`,
/// `energy_imbalance`, `residuals_above_limit`, and the two a caller adds with
/// [`Verdict::blocked_by`]: [`SOLVER_FAILED`] and [`NOT_FINISHED`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    pub state: VerdictState,
    pub reasons: Vec<String>,
    pub quantities: Vec<QuantityState>,
    pub mass: Balance,
    pub energy: Balance,
    pub residuals_ok: bool,
}

impl Verdict {
    /// The verdict with one more reason a run cannot be called converged for. A converged verdict
    /// becomes not converged; one that was not judged stays so. A reason already there is not
    /// added again.
    #[must_use]
    pub fn blocked_by(mut self, reason: &str) -> Self {
        if !self.reasons.iter().any(|r| r == reason) {
            self.reasons.push(reason.to_owned());
        }
        if self.state == VerdictState::Converged {
            self.state = VerdictState::NotConverged;
        }
        self
    }
}

/// Decides whether a run is converged, not converged, or cannot be judged.
///
/// `residuals_ok` is a necessary check only. The reasons come in this order: quantities not
/// settled (by name), mass, energy, residuals.
pub fn verdict(
    quantities: Vec<QuantityState>,
    mass: Balance,
    energy: Balance,
    residuals_ok: bool,
    _settings: &Settings,
) -> Verdict {
    let mut reasons = Vec::new();
    let state = if quantities.is_empty() {
        reasons.push("no_monitored_quantity".to_string());
        VerdictState::NotJudged
    } else {
        let mut open: Vec<&str> = quantities
            .iter()
            .filter(|q| q.status != Status::Settled)
            .map(|q| q.name.as_str())
            .collect();
        open.sort_unstable();
        reasons.extend(open.into_iter().map(|name| format!("not_settled:{name}")));
        if mass.status == BalanceStatus::Failed {
            reasons.push("mass_imbalance".to_string());
        }
        if energy.status == BalanceStatus::Failed {
            reasons.push("energy_imbalance".to_string());
        }
        if !residuals_ok {
            reasons.push("residuals_above_limit".to_string());
        }
        if reasons.is_empty() {
            VerdictState::Converged
        } else {
            VerdictState::NotConverged
        }
    };
    Verdict {
        state,
        reasons,
        quantities,
        mass,
        energy,
        residuals_ok,
    }
}
