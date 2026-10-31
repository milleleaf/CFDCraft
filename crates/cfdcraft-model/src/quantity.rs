use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Quantity {
    pub value: f64,
    pub unit: Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Unit {
    #[serde(rename = "m")]
    Metre,
    #[serde(rename = "mm")]
    Millimetre,
    #[serde(rename = "m/s")]
    MetrePerSecond,
    #[serde(rename = "m2/s", alias = "m²/s")]
    SquareMetrePerSecond,
    #[serde(rename = "Pa")]
    Pascal,
    #[serde(rename = "K")]
    Kelvin,
    #[serde(rename = "s")]
    Second,
    #[serde(rename = "1")]
    Dimensionless,
    #[serde(rename = "rpm")]
    Rpm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dimension {
    Length,
    Velocity,
    KinematicViscosity,
    Pressure,
    Temperature,
    Time,
    Dimensionless,
    AngularVelocity,
}

impl Unit {
    pub(crate) fn dimension(self) -> Dimension {
        match self {
            Self::Metre | Self::Millimetre => Dimension::Length,
            Self::MetrePerSecond => Dimension::Velocity,
            Self::SquareMetrePerSecond => Dimension::KinematicViscosity,
            Self::Pascal => Dimension::Pressure,
            Self::Kelvin => Dimension::Temperature,
            Self::Second => Dimension::Time,
            Self::Dimensionless => Dimension::Dimensionless,
            Self::Rpm => Dimension::AngularVelocity,
        }
    }
}
