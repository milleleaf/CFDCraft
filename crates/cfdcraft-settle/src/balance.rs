use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// The flux through one patch at the end of the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchFlux {
    pub patch: String,
    /// Whether the patch is an inlet.
    pub inlet: bool,
    /// The signed sum of the flux through the patch.
    pub sum: f64,
    /// The sum of the magnitudes, kept for display.
    pub sum_mag: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BalanceStatus {
    /// The imbalance is within the tolerance.
    Ok,
    /// The imbalance is over the tolerance, or there is nothing to compare it with.
    Failed,
    /// There is nothing to balance, as in a closed domain. Never a pass.
    NotApplicable,
}

/// One balance: how far it is from closing, and what that is measured against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Balance {
    pub status: BalanceStatus,
    /// The imbalance as a share of the reference.
    pub imbalance: Option<f64>,
    pub reference: Option<f64>,
}

impl Balance {
    fn not_applicable() -> Balance {
        Balance {
            status: BalanceStatus::NotApplicable,
            imbalance: None,
            reference: None,
        }
    }

    fn failed(reference: Option<f64>) -> Balance {
        Balance {
            status: BalanceStatus::Failed,
            imbalance: None,
            reference,
        }
    }

    fn measured(net: f64, reference: f64, tol: f64) -> Balance {
        let imbalance = net.abs() / reference;
        if !imbalance.is_finite() {
            return Balance::failed(Some(reference));
        }
        Balance {
            status: if imbalance < tol {
                BalanceStatus::Ok
            } else {
                BalanceStatus::Failed
            },
            imbalance: Some(imbalance),
            reference: Some(reference),
        }
    }
}

/// Whether the mass entering the domain leaves it.
///
/// The reference is the flux through the inlets. A domain with no inlet has nothing to compare
/// with and is `NotApplicable`. An inlet that carries no flux cannot close and fails.
pub fn mass_balance(patches: &[PatchFlux], s: &Settings) -> Balance {
    if !patches.iter().any(|p| p.inlet) {
        return Balance::not_applicable();
    }
    let reference: f64 = patches
        .iter()
        .filter(|p| p.inlet)
        .map(|p| p.sum.abs())
        .sum();
    let net: f64 = patches.iter().map(|p| p.sum).sum();
    if !reference.is_finite() || !net.is_finite() || reference <= 0.0 {
        return Balance::failed(Some(reference).filter(|r| r.is_finite()));
    }
    Balance::measured(net, reference, s.mass_tol)
}

/// Whether the energy balance closes, given the net heat flow and what it is measured against,
/// or `None` where energy is not solved.
pub fn energy_balance(energy: Option<(f64, f64)>, s: &Settings) -> Balance {
    let Some((net, reference)) = energy else {
        return Balance::not_applicable();
    };
    let reference = reference.abs();
    if !net.is_finite() || !reference.is_finite() || reference <= 0.0 {
        return Balance::failed(Some(reference).filter(|r| r.is_finite()));
    }
    Balance::measured(net, reference, s.energy_tol)
}
