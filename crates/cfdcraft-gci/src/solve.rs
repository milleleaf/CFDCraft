//! The observed order of convergence, found by a bracketed bisection.

/// Lowest order tried; also the point of the existence test.
const LOW: f64 = 1e-9;
/// Highest order tried.
const HIGH: f64 = 60.0;
/// The bracket is narrowed until it is below this width.
const WIDTH: f64 = 1e-12;
/// A root is accepted only when the residual there is below this.
const RESIDUAL: f64 = 1e-9;

/// Solve `p ln r21 = ln R + ln((r21^p - s) / (r32^p - s))` for the observed order `p`.
///
/// `r21` and `r32` are the refinement ratios (fine over medium, medium over coarse), `ratio_abs`
/// is `|e32 / e21|` and `s` is the sign of `e32 / e21`. A positive root exists exactly when the
/// function is negative at the lowest order tried; otherwise the three values do not converge in
/// the asymptotic sense and the answer is `None`. A bisection is used on purpose: a fixed-point
/// iteration can fail to converge, a bracket cannot.
pub fn observed_order(r21: f64, r32: f64, ratio_abs: f64, s: f64) -> Option<f64> {
    if !(ratio_abs.is_finite() && ratio_abs > 0.0) {
        return None;
    }
    let g = |p: f64| p * r21.ln() - ratio_abs.ln() - ((r21.powf(p) - s) / (r32.powf(p) - s)).ln();
    let (mut lo, mut hi) = (LOW, HIGH);
    let at_low = g(lo);
    let at_high = g(hi);
    if !(at_low.is_finite() && at_low < 0.0 && at_high.is_finite() && at_high > 0.0) {
        return None;
    }
    for _ in 0..200 {
        if hi - lo <= WIDTH {
            break;
        }
        let mid = 0.5 * (lo + hi);
        let at_mid = g(mid);
        if !at_mid.is_finite() {
            return None;
        }
        if at_mid < 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let p = 0.5 * (lo + hi);
    let residual = g(p);
    (residual.is_finite() && residual.abs() < RESIDUAL).then_some(p)
}
