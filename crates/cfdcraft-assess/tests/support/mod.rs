//! Synthetic solver logs and monitor files in the layouts the solver writes. Every number is
//! written with twelve decimals (or in exponent form), so the text is the same on every platform.

#![allow(dead_code)] // each test file uses a different part of this module

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cfdcraft_assess::{
    Files, LineSpec, PatchSpec, ProbeSpec, Spec, SurfaceQuantityKind, SurfaceSpec,
};
use cfdcraft_monitors::FieldKind;

/// A cavity log of `iterations` iterations whose residuals fall by 1 % each step from 1e-2. A log
/// that has ended carries the `End` line.
pub fn log(iterations: u64, ended: bool) -> String {
    let mut text = String::new();
    for i in 1..=iterations {
        let r = 1e-2 * 0.99f64.powi(i as i32);
        let _ = writeln!(text, "Time = {i}\n");
        for (solver, field) in [
            ("smoothSolver", "Ux"),
            ("smoothSolver", "Uy"),
            ("DICPCG", "p"),
        ] {
            let _ = writeln!(
                text,
                "{solver}:  Solving for {field}, Initial residual = {r:.12e}, Final residual = {:.12e}, No Iterations 2",
                r * 1e-3
            );
        }
        text.push('\n');
    }
    if ended {
        text.push_str("End\n");
    }
    text
}

/// A log whose pressure residual jumps from 1 to 1e7 on the last step.
pub fn blown_up_log() -> String {
    let mut text = String::new();
    for (i, r) in [(1u32, 1.0f64), (2, 1e7)] {
        let _ = writeln!(text, "Time = {i}\n");
        let _ = writeln!(
            text,
            "DICPCG:  Solving for p, Initial residual = {r:.12e}, Final residual = {:.12e}, No Iterations 2\n",
            r * 1e-3
        );
    }
    text
}

/// A probes file with one probe: `rows` are (time, value cell).
fn probe_file(vector: bool, rows: &[(f64, [f64; 2])]) -> String {
    let mut text = String::from(
        "# Probe 0 (0.050000000000 0.050000000000 0.005000000000)\n#       Probe             0\n#        Time\n",
    );
    for (time, [a, b]) in rows {
        if vector {
            let _ = writeln!(text, "{time:.12}   ({a:.12} {b:.12} 0.000000000000)");
        } else {
            let _ = writeln!(text, "{time:.12}   {a:.12}");
        }
    }
    text
}

/// How a monitored value moves over `n` iterations.
#[derive(Clone, Copy)]
pub enum Shape {
    /// Approaches `base` and stays there.
    Settles,
    /// Still ramping.
    Ramps,
    /// Settled mean with a jitter of 3 % of `base`.
    Jitters,
}

fn value(shape: Shape, base: f64, t: u64) -> f64 {
    let t = t as f64;
    match shape {
        Shape::Settles => base * (1.0 - (-t / 40.0).exp()),
        Shape::Ramps => base * 1e-3 * t,
        Shape::Jitters => {
            base * (1.0
                + if (t as u64).is_multiple_of(2) {
                    0.015
                } else {
                    -0.015
                })
        }
    }
}

/// A velocity probe file of `n` iterations; Ux moves by `shape` toward `base`, Uy toward `-base / 2`.
pub fn velocity_probe(n: u64, shape: Shape, base: f64) -> String {
    let rows: Vec<(f64, [f64; 2])> = (1..=n)
        .map(|t| {
            (
                t as f64,
                [value(shape, base, t), value(shape, -base / 2.0, t)],
            )
        })
        .collect();
    probe_file(true, &rows)
}

/// A pressure probe file of `n` iterations.
pub fn pressure_probe(n: u64, shape: Shape, base: f64) -> String {
    let rows: Vec<(f64, [f64; 2])> = (1..=n)
        .map(|t| (t as f64, [value(shape, base, t), 0.0]))
        .collect();
    probe_file(false, &rows)
}

/// A surface field value file with one column; `values` are paired with times 1, 2, 3...
pub fn surface_file(column: &str, values: &[f64]) -> String {
    let mut text = format!(
        "# Region type : patch inlet\n# Faces       : 20\n# Area        : 0.100000000000\n# Scale factor : 1\n#\n# Time\t{column}\n"
    );
    for (i, v) in values.iter().enumerate() {
        let _ = writeln!(text, "{:.12}\t{v:.12}", (i + 1) as f64);
    }
    text
}

/// `n` iterations of a surface quantity that settles at `base`.
pub fn settling_surface(column: &str, n: u64, base: f64) -> String {
    let values: Vec<f64> = (1..=n).map(|t| value(Shape::Settles, base, t)).collect();
    surface_file(column, &values)
}

/// A sampled velocity line of 16 points whose component means are `ux` and `uy`.
pub fn line_file(ux: f64, uy: f64) -> String {
    let mut text = String::new();
    for i in 0..16 {
        let x = f64::from(i) * 0.01;
        let _ = writeln!(
            text,
            "{x:.12} 0.000000000000 0.000000000000 {ux:.12} {uy:.12} 0.000000000000"
        );
    }
    text
}

/// `folders` write-time folders of a line that settles (time `100 * i`).
pub fn line_folders(folders: u64) -> Vec<(String, String)> {
    (1..=folders)
        .map(|i| {
            let k = (-(i as f64)).exp();
            (
                (100 * i).to_string(),
                line_file(0.3 * (1.0 + k), -0.1 * (1.0 + k)),
            )
        })
        .collect()
}

pub fn probe_spec(fo: &str, field: FieldKind) -> ProbeSpec {
    ProbeSpec {
        fo: fo.to_owned(),
        label: format!("label of {fo}"),
        field,
    }
}

pub fn line_spec(fo: &str) -> LineSpec {
    LineSpec {
        fo: fo.to_owned(),
        label: format!("label of {fo}"),
        field: FieldKind::Velocity,
    }
}

pub fn surface_spec(fo: &str, quantity: SurfaceQuantityKind) -> SurfaceSpec {
    SurfaceSpec {
        fo: fo.to_owned(),
        label: format!("label of {fo}"),
        quantity,
    }
}

pub fn patch(patch: &str, inlet: bool) -> PatchSpec {
    PatchSpec {
        fo: format!("balance_{patch}"),
        patch: patch.to_owned(),
        inlet,
    }
}

/// Add one function object's files, all in the start-time folder `0`.
pub fn put(files: &mut Files, fo: &str, text: String) {
    files
        .monitor_files
        .insert(fo.to_owned(), vec![("0".to_owned(), text)]);
}

/// The closed cavity of the first test: five velocity probes, two wall balances, no inlet.
pub fn cavity(shape_of_probe_3: Shape) -> (Spec, Files) {
    let mut spec = Spec {
        residual_limits: Some((1e-6, 1e-6)),
        ..Spec::default()
    };
    let mut files = Files {
        log: log(1500, true),
        monitor_files: BTreeMap::new(),
    };
    for i in 1..=5 {
        let fo = format!("probe_{i}");
        spec.probes.push(probe_spec(&fo, FieldKind::Velocity));
        let shape = if i == 3 {
            shape_of_probe_3
        } else {
            Shape::Settles
        };
        put(
            &mut files,
            &fo,
            velocity_probe(1500, shape, 0.2 + f64::from(i) * 0.01),
        );
    }
    for name in ["movingWall", "fixedWalls"] {
        let p = patch(name, false);
        put(
            &mut files,
            &p.fo,
            surface_file("sum(phi)", &[0.0, 0.0, 0.0]),
        );
        spec.patches.push(p);
    }
    (spec, files)
}
