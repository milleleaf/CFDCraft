use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A setting is outside the range it may take.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettleError {
    #[error("the setting {setting} is out of range: {reason}")]
    OutOfRange {
        setting: &'static str,
        reason: &'static str,
    },
}

/// The tolerances that decide whether a monitored quantity has settled and whether the balances close.
///
/// Every member is a setting, not a constant; the defaults are the numbers a run is judged by when
/// the study says nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    /// The share of the samples, taken from the end, that the statistics use.
    pub tail_fraction: f64,
    /// The fewest iteration samples a tail may hold; a shorter series is too early to judge.
    pub tail_floor: u64,
    /// A quantity is settled when its spread over the tail is below this share of its scale.
    pub spread_rel: f64,
    /// A quantity is settled when the fitted line moves less than this share of its scale across the tail.
    pub drift_rel: f64,
    /// The fewest write-time samples a series needs before it is judged.
    pub line_min_samples: u64,
    /// The scale of a quantity is at least this share of its range over the whole run.
    pub floor_fraction: f64,
    /// The mass imbalance, as a share of the inlet flux, below which the balance closes.
    pub mass_tol: f64,
    /// The energy imbalance, as a share of its reference, below which the balance closes.
    pub energy_tol: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            tail_fraction: 0.20,
            tail_floor: 50,
            spread_rel: 0.01,
            drift_rel: 0.001,
            line_min_samples: 5,
            floor_fraction: 1e-3,
            mass_tol: 1e-3,
            energy_tol: 1e-2,
        }
    }
}

fn unit(v: f64) -> bool {
    v > 0.0 && v <= 1.0
}

fn need(setting: &'static str, ok: bool, reason: &'static str) -> Result<(), SettleError> {
    if ok {
        Ok(())
    } else {
        Err(SettleError::OutOfRange { setting, reason })
    }
}

impl Settings {
    /// Refuses a member outside its range.
    pub fn check(&self) -> Result<(), SettleError> {
        need(
            "tail_fraction",
            unit(self.tail_fraction),
            "above 0 and at most 1",
        )?;
        need(
            "tail_floor",
            (5..=100_000).contains(&self.tail_floor),
            "from 5 to 100000",
        )?;
        need("spread_rel", unit(self.spread_rel), "above 0 and at most 1")?;
        need("drift_rel", unit(self.drift_rel), "above 0 and at most 1")?;
        need(
            "line_min_samples",
            (3..=1000).contains(&self.line_min_samples),
            "from 3 to 1000",
        )?;
        need(
            "floor_fraction",
            (0.0..=1.0).contains(&self.floor_fraction),
            "from 0 to 1",
        )?;
        need("mass_tol", unit(self.mass_tol), "above 0 and at most 1")?;
        need("energy_tol", unit(self.energy_tol), "above 0 and at most 1")?;
        Ok(())
    }
}
