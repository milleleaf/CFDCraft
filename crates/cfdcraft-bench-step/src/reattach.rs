//! Where a separated flow reattaches along a line just above the floor.

/// The reattachment length of a separation bubble, from samples of the streamwise velocity.
///
/// `points` are (x, Ux) in order along the line, with x in metres in the study's frame, and
/// `step_x` is the x of the step. The reattachment is the last place where the velocity changes
/// from negative to positive, found by linear interpolation between the two samples around it, so
/// a corner eddy next to the step (positive, then negative) cannot be taken for it. The result is
/// measured from the step, in metres; the caller divides by the step height.
///
/// `None` when the velocity is never negative, or is still negative at the last sample.
pub fn reattachment_length(points: &[(f64, f64)], step_x: f64) -> Option<f64> {
    points
        .windows(2)
        .rfind(|w| w[0].1 < 0.0 && w[1].1 >= 0.0)
        .map(|w| {
            let ((x0, u0), (x1, u1)) = (w[0], w[1]);
            x0 + (x1 - x0) * (-u0) / (u1 - u0) - step_x
        })
}
