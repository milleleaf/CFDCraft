//! Log text builders shared by the test files. Every log built here is synthetic; the lines have
//! the shapes of a real simpleFoam log. Numbers are written in exponent form with twelve digits
//! after the point, so the text is the same on every platform and tiny residuals survive.

#![allow(dead_code)] // each test file uses a different part of this module

use std::fmt::Write as _;

/// One solver line.
pub fn solve_line(solver: &str, field: &str, initial: f64, iterations: u32) -> String {
    format!(
        "{solver}:  Solving for {field}, Initial residual = {initial:.12e}, Final residual = {:.12e}, No Iterations {iterations}\n",
        initial * 1e-6
    )
}

/// One continuity line.
pub fn continuity_line(local: f64) -> String {
    format!(
        "time step continuity errors : sum local = {local:.12e}, global = {:.12e}, cumulative = {:.12e}\n",
        local * -1e-3,
        local * 1e-1
    )
}

/// One whole iteration: time, three solves, continuity, clock.
pub fn iteration(time: u64, ux: f64, uy: f64, p: f64, local: f64) -> String {
    let mut text = format!("Time = {time}\n\n");
    text.push_str(&solve_line("smoothSolver", "Ux", ux, 3));
    text.push_str(&solve_line("smoothSolver", "Uy", uy, 3));
    text.push_str(&solve_line("DICPCG", "p", p, 20));
    text.push_str(&continuity_line(local));
    let clock = time as f64 * 0.5;
    let _ = writeln!(text, "ExecutionTime = {clock:.2} s  ClockTime = {time} s\n");
    text
}

/// The first residual of Ux in [`cavity_log`].
pub const UX0: f64 = 0.5;
/// The first residual of Uy in [`cavity_log`].
pub const UY0: f64 = 0.4;
/// The first residual of p in [`cavity_log`].
pub const P0: f64 = 0.3;

/// Residuals at iteration `time` of a log that starts at `from` and falls by `scale` each step.
pub fn fall(base: f64, from: u64, time: u64, scale: f64) -> f64 {
    base * scale.powi((time - from) as i32)
}

/// Iterations `from..=to` of a healthy cavity run, no `End` line.
pub fn cavity_log(from: u64, to: u64, scale: f64) -> String {
    let mut text = String::new();
    for time in from..=to {
        text.push_str(&iteration(
            time,
            fall(UX0, from, time, scale),
            fall(UY0, from, time, scale),
            fall(P0, from, time, scale),
            fall(1e-3, from, time, scale),
        ));
    }
    text
}

/// The same, finished with `End`.
pub fn cavity_log_with_end(from: u64, to: u64, scale: f64) -> String {
    let mut text = cavity_log(from, to, scale);
    text.push_str("End\n\n");
    text
}

/// One iteration per value of Ux; Uy and p stay flat, and so does continuity.
pub fn ux_log(ux: &[f64]) -> String {
    let mut text = String::new();
    for (i, value) in ux.iter().enumerate() {
        text.push_str(&iteration(i as u64 + 1, *value, 1e-3, 1e-3, 1e-9));
    }
    text
}

/// One iteration per value of the local continuity error; the residuals stay flat.
pub fn local_log(locals: &[f64]) -> String {
    let mut text = String::new();
    for (i, local) in locals.iter().enumerate() {
        text.push_str(&iteration(i as u64 + 1, 1e-3, 1e-3, 1e-3, *local));
    }
    text
}

/// True when `a` and `b` agree to the digits the log text carries.
pub fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1e-300)
}
