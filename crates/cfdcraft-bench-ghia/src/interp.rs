use thiserror::Error;

/// Centreline coordinates and velocities, both nondimensionalized by the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub coords: Vec<f64>,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum InterpError {
    #[error("profile needs at least two points")]
    TooFewPoints,
    #[error("profile coordinate and velocity lengths differ")]
    LengthMismatch,
    #[error("profile and query values must be finite")]
    NonFinite,
    #[error("profile coordinates must be strictly increasing")]
    NotIncreasing,
    #[error("normalized benchmark profile must include both physical walls at 0 and 1")]
    MissingWalls,
}

impl Profile {
    pub fn validate(&self) -> Result<(), InterpError> {
        if self.coords.len() != self.values.len() {
            return Err(InterpError::LengthMismatch);
        }
        if self.coords.len() < 2 {
            return Err(InterpError::TooFewPoints);
        }
        if self
            .coords
            .iter()
            .chain(&self.values)
            .any(|x| !x.is_finite())
        {
            return Err(InterpError::NonFinite);
        }
        if self.coords.windows(2).any(|w| w[0] >= w[1]) {
            return Err(InterpError::NotIncreasing);
        }
        Ok(())
    }
}

/// Piecewise-linear interpolation, clamped to the nearest endpoint outside range.
///
/// This is CFDCraft's explicit contract; LidJul's bracket-index clamp instead
/// extrapolates outside range. Production profiles include the physical walls.
pub fn linear_sample(profile: &Profile, x: f64) -> Result<f64, InterpError> {
    profile.validate()?;
    if !x.is_finite() {
        return Err(InterpError::NonFinite);
    }
    let last = profile.coords.len() - 1;
    if x <= profile.coords[0] {
        return Ok(profile.values[0]);
    }
    if x >= profile.coords[last] {
        return Ok(profile.values[last]);
    }
    match profile.coords.binary_search_by(|a| a.total_cmp(&x)) {
        Ok(i) => Ok(profile.values[i]),
        Err(i) => {
            let w = (x - profile.coords[i - 1]) / (profile.coords[i] - profile.coords[i - 1]);
            let value = (1.0 - w) * profile.values[i - 1] + w * profile.values[i];
            if value.is_finite() {
                Ok(value)
            } else {
                Err(InterpError::NonFinite)
            }
        }
    }
}
