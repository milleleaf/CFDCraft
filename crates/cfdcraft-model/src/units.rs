use std::cell::Cell;

use crate::{Quantity, Unit};

thread_local! {
    static CONVERSIONS: Cell<usize> = const { Cell::new(0) };
}

/// Convert once to SI base units. The resolver owns dimensional validation.
pub fn to_si(quantity: &Quantity) -> f64 {
    CONVERSIONS.with(|count| count.set(count.get() + 1));
    match quantity.unit {
        // Scale the authored shortest decimal before the single rounding to
        // binary SI. Binary multiplication makes 19.1 mm differ from 0.0191 m.
        Unit::Millimetre if quantity.value.is_finite() => format!("{}e-3", quantity.value)
            .parse()
            .expect("finite decimal with a base-ten exponent"),
        Unit::Millimetre => quantity.value,
        Unit::Rpm => quantity.value * (std::f64::consts::TAU / 60.0),
        _ => quantity.value,
    }
}

/// Thread-local instrumentation for the conversion-once regression check.
#[doc(hidden)]
pub fn conversion_count() -> usize {
    CONVERSIONS.with(Cell::get)
}

#[doc(hidden)]
pub fn reset_conversion_count() {
    CONVERSIONS.with(|count| count.set(0));
}
