//! The divergence rules: when a run should be stopped because it is blowing up.
//!
//! The rules read only the [`LogState`] the scanner builds, in a fixed order: a floating-point
//! trap or a non-finite number, a residual blow-up, a Courant number that is too large (transient
//! runs only), and a continuity error that keeps growing. The first rule that fires wins.
//!
//! A residual blow-up needs two things at once: the residual is more than `blowup_factor` times
//! its first value, and it is above `blowup_floor`. The floor is a refinement of the factor
//! rule. A run continued from a converged state starts with residuals near 1e-9, so a factor
//! alone would stop it at 1e-5, long before anything is wrong.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::scan::{FatalKind, LogState};

/// The thresholds of the rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Limits {
    /// A residual above this many times its first value may be a blow-up.
    pub blowup_factor: f64,
    /// A residual below this is never a blow-up.
    pub blowup_floor: f64,
    /// The largest Courant number a transient run may reach.
    pub courant_max: f64,
    /// The continuity error is growing when its recent mean is this many times its minimum.
    pub continuity_growth: f64,
    /// A continuity error below this is never growing.
    pub continuity_floor: f64,
    /// How many of the latest continuity values make the recent mean.
    pub continuity_window: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            blowup_factor: 1e4,
            blowup_floor: 1e2,
            courant_max: 50.0,
            continuity_growth: 10.0,
            continuity_floor: 1e-3,
            continuity_window: 10,
        }
    }
}

/// Which rule fired. The codes are stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DivergenceCode {
    /// A floating-point trap or a non-finite number.
    Trap,
    /// A residual blew up.
    ResidualBlowup,
    /// The Courant number is too large.
    Courant,
    /// The continuity error is growing.
    ContinuityGrowing,
}

impl DivergenceCode {
    /// The stable finding code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Trap => "CFD-RUN-003",
            Self::ResidualBlowup => "CFD-RUN-004",
            Self::Courant => "CFD-RUN-005",
            Self::ContinuityGrowing => "CFD-RUN-006",
        }
    }
}

/// What the first rule that fired saw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Divergence {
    pub code: DivergenceCode,
    /// The field of a residual blow-up.
    pub field: Option<String>,
    /// The value that broke the rule. A trap has no value and reports 0.
    pub value: f64,
    /// The limit it broke. A trap has no limit and reports 0.
    pub limit: f64,
}

fn trap() -> Divergence {
    Divergence {
        code: DivergenceCode::Trap,
        field: None,
        value: 0.0,
        limit: 0.0,
    }
}

/// A stored number a log can never produce: not finite, or a negative residual.
fn unusable(state: &LogState) -> bool {
    let bad_residual = state
        .first_initial
        .values()
        .chain(state.last_initial.values())
        .any(|v| !v.is_finite() || *v < 0.0);
    let bad_other = state
        .local_errors
        .iter()
        .chain(&state.min_local)
        .chain(&state.max_courant)
        .any(|v| !v.is_finite());
    bad_residual || bad_other
}

/// The first rule that fires on `state`, if any. `transient` is true for a time-stepping run;
/// the Courant rule does not apply to a steady one.
pub fn divergence(state: &LogState, limits: &Limits, transient: bool) -> Option<Divergence> {
    if state.fatal == Some(FatalKind::FloatingPoint) || state.non_finite || unusable(state) {
        return Some(trap());
    }
    for (field, value) in &state.last_initial {
        let Some(first) = state.first_initial.get(field) else {
            continue;
        };
        let limit = (limits.blowup_factor * first).max(limits.blowup_floor);
        if *value > limit {
            return Some(Divergence {
                code: DivergenceCode::ResidualBlowup,
                field: Some(field.clone()),
                value: *value,
                limit,
            });
        }
    }
    if transient
        && let Some(max) = state.max_courant
        && max > limits.courant_max
    {
        return Some(Divergence {
            code: DivergenceCode::Courant,
            field: None,
            value: max,
            limit: limits.courant_max,
        });
    }
    let window = limits.continuity_window;
    if window > 0
        && state.local_errors.len() >= window
        && let Some(min) = state.min_local
    {
        let recent = &state.local_errors[state.local_errors.len() - window..];
        let mean = recent.iter().sum::<f64>() / window as f64;
        let limit = (limits.continuity_growth * min).max(limits.continuity_floor);
        if mean >= limits.continuity_growth * min && mean > limits.continuity_floor {
            return Some(Divergence {
                code: DivergenceCode::ContinuityGrowing,
                field: None,
                value: mean,
                limit,
            });
        }
    }
    None
}
