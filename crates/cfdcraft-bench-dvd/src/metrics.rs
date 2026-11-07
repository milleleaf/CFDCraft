//! The numbers a heated-cavity run is judged by: the hot-wall Nusselt number, the two velocity
//! maxima with their positions, and the imbalance between the heat flows of the two walls.
//!
//! Sign convention: the heat-flux file holds the area integral of the temperature gradient along
//! x over a wall. The hot wall is on the left and the temperature falls into the fluid, so the
//! integral is negative, and `Nu = -(L / dT) x integral / area` is positive for a run that carries
//! heat across the cavity. A positive integral gives a negative Nusselt number, which fails.
//! Velocities are scaled to the paper's units by `L x Pr / nu` and positions by `L`.

use cfdcraft_gate::{Mismatch, ResultFiles};
use cfdcraft_model::{
    BoundaryKind, Face, MonitorField, ResolvedStudy, ResolvedThermal, SurfaceQuantity,
};
use cfdcraft_monitors::{LineFile, Value};

use crate::data::{PRANDTL, rayleigh};
use crate::files::{Refusal, read_line, read_surface};

/// The column of the heat-flux file that holds the gradient integral.
const FLUX_COLUMN: &str = "areaIntegrate(gradT)";
/// How far, in the cavity side, a monitor line may be from where it must be.
const LINE_SLACK: f64 = 1e-6;

/// Everything the comparison needs from a study, and the function-object names of the monitors it
/// reads. Built only for a study that is a heated square cavity.
#[derive(Debug, Clone, PartialEq)]
pub struct Setup {
    /// The cavity side in metres.
    pub length: f64,
    /// The temperature difference of the two walls in kelvin.
    pub delta_t: f64,
    pub nu: f64,
    pub prandtl: f64,
    pub rayleigh: f64,
    /// The label of the hot wall in the study.
    pub hot_boundary: String,
    pub hot_fo: String,
    pub cold_fo: String,
    pub vertical_fo: String,
    pub horizontal_fo: String,
}

fn mismatch(what: &str, found: impl Into<String>, expected: &str) -> Mismatch {
    Mismatch {
        what: what.to_owned(),
        found: found.into(),
        expected: expected.to_owned(),
    }
}

/// The wall on one vertical face: alone on that face, a no-slip wall at a fixed temperature.
fn wall_on(study: &ResolvedStudy, face: Face) -> Option<(&str, f64)> {
    study.boundary_conditions.iter().find_map(|(label, b)| {
        if b.faces != [face] || b.kind != BoundaryKind::NoSlipWall {
            return None;
        }
        match b.thermal {
            Some(ResolvedThermal::FixedTemperature { value }) => Some((label.as_str(), value)),
            _ => None,
        }
    })
}

impl Setup {
    /// Whether the study is a closed square cavity, hot on the left and cold on the right,
    /// insulated above and below, with Boussinesq buoyancy at the benchmark's Prandtl number and
    /// the four monitors the comparison reads. Every difference is listed.
    pub fn from_study(study: &ResolvedStudy) -> Result<Setup, Vec<Mismatch>> {
        let mut found: Vec<Mismatch> = Vec::new();
        let d = &study.domain;
        if d.open || d.shape.is_some() {
            found.push(mismatch("domain", "open or shaped", "a closed box"));
        }
        let length = d.length_x;
        if (d.length_x - d.length_y).abs() > 1e-9 * d.length_x {
            found.push(mismatch(
                "cavity",
                format!("{} m by {} m", d.length_x, d.length_y),
                "a square",
            ));
        }
        let buoyancy = study.physics.as_ref().and_then(|p| p.buoyancy);
        let prandtl = buoyancy.map_or(0.0, |b| b.prandtl);
        match buoyancy {
            None => found.push(mismatch("buoyancy", "none", "Boussinesq buoyancy")),
            Some(b) => {
                if (b.prandtl - PRANDTL).abs() > 0.01 * PRANDTL {
                    found.push(mismatch(
                        "Prandtl number",
                        b.prandtl.to_string(),
                        "0.71 within 1 %",
                    ));
                }
            }
        }
        let hot = wall_on(study, Face::XMin);
        let cold = wall_on(study, Face::XMax);
        match (hot, cold) {
            (Some((_, th)), Some((_, tc))) if th > tc => {}
            (Some((_, th)), Some((_, tc))) => found.push(mismatch(
                "hot wall",
                format!("{th} K on the left, {tc} K on the right"),
                "the hotter wall on the left (XMin)",
            )),
            _ => found.push(mismatch(
                "side walls",
                "no no-slip wall at a fixed temperature on each of XMin and XMax",
                "a hot wall on XMin and a cold wall on XMax, each alone on its face",
            )),
        }
        for (label, b) in &study.boundary_conditions {
            let side = b.faces == [Face::XMin] || b.faces == [Face::XMax];
            if side {
                continue;
            }
            let horizontal =
                !b.faces.is_empty() && b.faces.iter().all(|f| matches!(f, Face::YMin | Face::YMax));
            let insulated = matches!(b.thermal, None | Some(ResolvedThermal::Adiabatic));
            if !(horizontal && b.kind == BoundaryKind::NoSlipWall && insulated) {
                found.push(mismatch(
                    &format!("boundary {label}"),
                    "not an insulated no-slip wall on YMin or YMax",
                    "an insulated no-slip wall on YMin and YMax",
                ));
            }
        }
        let plan = study.monitor_plan();
        let surface = |name: &str, boundary: Option<&str>| {
            plan.surfaces
                .iter()
                .find(|s| s.label.as_deref() == Some(name))
                .filter(|s| {
                    s.quantity == SurfaceQuantity::HeatFlux
                        && boundary.is_some_and(|b| s.boundary == b)
                })
                .map(|s| s.fo.clone())
        };
        let hot_label = hot.map(|(label, _)| label);
        let cold_label = cold.map(|(label, _)| label);
        let hot_fo = surface("hot_flux", hot_label);
        let cold_fo = surface("cold_flux", cold_label);
        if hot_fo.is_none() {
            found.push(mismatch(
                "surface hot_flux",
                "missing or not a heat flux of the hot wall",
                "a heat-flux surface on the hot wall",
            ));
        }
        if cold_fo.is_none() {
            found.push(mismatch(
                "surface cold_flux",
                "missing or not a heat flux of the cold wall",
                "a heat-flux surface on the cold wall",
            ));
        }
        // A mid-plane line of velocity from one wall to the other. `vertical` lines have a
        // fixed x.
        let line = |name: &str, vertical: bool| {
            let slack = LINE_SLACK * length;
            plan.lines
                .iter()
                .find(|l| l.label.as_deref() == Some(name))
                .filter(|l| {
                    let (fixed0, fixed1, from, to) = if vertical {
                        (l.x0, l.x1, l.y0, l.y1)
                    } else {
                        (l.y0, l.y1, l.x0, l.x1)
                    };
                    l.field == MonitorField::Velocity
                        && (fixed0 - length / 2.0).abs() <= slack
                        && (fixed1 - length / 2.0).abs() <= slack
                        && from.abs() <= slack
                        && (to - length).abs() <= slack
                })
                .map(|l| l.fo.clone())
        };
        let vertical_fo = line("mid_vertical", true);
        let horizontal_fo = line("mid_horizontal", false);
        if vertical_fo.is_none() {
            found.push(mismatch(
                "line mid_vertical",
                "missing or not along the vertical mid-plane",
                "a velocity line from the floor to the ceiling at half the width",
            ));
        }
        if horizontal_fo.is_none() {
            found.push(mismatch(
                "line mid_horizontal",
                "missing or not along the horizontal mid-plane",
                "a velocity line from the left wall to the right wall at half the height",
            ));
        }
        match (
            hot,
            cold,
            buoyancy,
            hot_fo,
            cold_fo,
            vertical_fo,
            horizontal_fo,
        ) {
            (
                Some((hot_label, th)),
                Some((_, tc)),
                Some(b),
                Some(hot_fo),
                Some(cold_fo),
                Some(vertical_fo),
                Some(horizontal_fo),
            ) if found.is_empty() => {
                let delta_t = th - tc;
                let nu = study.fluid.kinematic_viscosity;
                Ok(Setup {
                    length,
                    delta_t,
                    nu,
                    prandtl,
                    rayleigh: rayleigh(b.gravity, b.beta, delta_t, length, nu, prandtl),
                    hot_boundary: hot_label.to_owned(),
                    hot_fo,
                    cold_fo,
                    vertical_fo,
                    horizontal_fo,
                })
            }
            _ => Err(found),
        }
    }

    /// The factor from a velocity in m/s to the paper's units.
    pub fn velocity_scale(&self) -> f64 {
        self.length * self.prandtl / self.nu
    }
}

/// The numbers of a run, in the paper's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    pub nu_hot: f64,
    pub u_max: f64,
    /// Height, in cavity sides, of the largest horizontal velocity on the vertical mid-plane.
    pub u_max_y: f64,
    pub w_max: f64,
    /// Distance from the hot wall, in cavity sides, of the largest vertical velocity on the
    /// horizontal mid-plane.
    pub w_max_x: f64,
    /// `|hot - cold| / |hot|` of the two heat flows.
    pub energy_imbalance: f64,
}

fn unusable(what: impl Into<String>) -> Refusal {
    Refusal::Unusable { what: what.into() }
}

/// The largest sample, placed between samples by a parabola through it and its two neighbours
/// when the parabola opens downward and peaks between them. A largest sample at either end is
/// taken as it is. `what` names the file for the refusal.
pub fn peak(what: &str, samples: &[(f64, f64)]) -> Result<(f64, f64), Refusal> {
    if samples.len() < 3 {
        return Err(unusable(format!("{what} holds fewer than three samples")));
    }
    if samples.windows(2).any(|w| w[1].0 <= w[0].0) {
        return Err(unusable(format!("{what} does not run in increasing order")));
    }
    let Some((at, &(x, y))) = samples
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
    else {
        return Err(unusable(format!("{what} holds no samples")));
    };
    if at == 0 || at + 1 == samples.len() {
        return Ok((x, y));
    }
    let (x0, y0) = samples[at - 1];
    let (x2, y2) = samples[at + 1];
    let d01 = (y - y0) / (x - x0);
    let d12 = (y2 - y) / (x2 - x);
    let a = (d12 - d01) / (x2 - x0);
    if a < 0.0 {
        let vertex = (x0 + x - d01 / a) / 2.0;
        if (x0..=x2).contains(&vertex) {
            let top = y0 + d01 * (vertex - x0) + a * (vertex - x0) * (vertex - x);
            return Ok((vertex, top));
        }
    }
    Ok((x, y))
}

/// The positions and the velocity component of a sampled line. A four-column file holds the
/// distance from the start of the line, then the three components; a six-column file holds x, y
/// and z, then the three components. `axis` is the coordinate that varies along the line (0 for
/// x, 1 for y) and `component` the velocity component to read (0 for Ux, 1 for Uy). The monitor
/// lines start at zero, so the distance is the position.
fn line_samples(
    what: &str,
    file: &LineFile,
    axis: usize,
    component: usize,
) -> Result<Vec<(f64, f64)>, Refusal> {
    let (at, value_at) = match file.columns {
        4 => (0, 1 + component),
        6 => (axis, 3 + component),
        n => return Err(unusable(format!("{what} has {n} columns, expected 4 or 6"))),
    };
    Ok(file.rows.iter().map(|r| (r[at], r[value_at])).collect())
}

fn flux(files: &dyn ResultFiles, fo: &str) -> Result<(f64, f64), Refusal> {
    let (file, path) = read_surface(files, fo)?;
    let column = file
        .columns
        .iter()
        .position(|c| c == FLUX_COLUMN)
        .ok_or_else(|| unusable(format!("{path} has no {FLUX_COLUMN} column")))?;
    let last = file
        .rows
        .last()
        .ok_or_else(|| unusable(format!("{path} holds no rows")))?;
    let integral = match last.values.get(column) {
        Some(Value::Vector(v)) => v[0],
        _ => return Err(unusable(format!("{path} has no vector in {FLUX_COLUMN}"))),
    };
    if !(file.area.is_finite() && file.area > 0.0) {
        return Err(unusable(format!("{path} has no usable area")));
    }
    Ok((integral, file.area))
}

/// Read a finished run for the setup the study gave.
pub fn measure_with(files: &dyn ResultFiles, setup: &Setup) -> Result<Measured, Refusal> {
    let (hot, hot_area) = flux(files, &setup.hot_fo)?;
    let (cold, _) = flux(files, &setup.cold_fo)?;
    if hot == 0.0 {
        return Err(unusable("the hot wall carries no heat flow"));
    }
    let nu_hot = -(setup.length / setup.delta_t) * (hot / hot_area);
    let energy_imbalance = (hot - cold).abs() / hot.abs();
    let scale = setup.velocity_scale();
    let (vertical, vertical_path) = read_line(files, &setup.vertical_fo)?;
    let (u_y, u_max) = peak(
        &vertical_path,
        &line_samples(&vertical_path, &vertical, 1, 0)?,
    )?;
    let (horizontal, horizontal_path) = read_line(files, &setup.horizontal_fo)?;
    let (w_x, w_max) = peak(
        &horizontal_path,
        &line_samples(&horizontal_path, &horizontal, 0, 1)?,
    )?;
    let measured = Measured {
        nu_hot,
        u_max: u_max * scale,
        u_max_y: u_y / setup.length,
        w_max: w_max * scale,
        w_max_x: w_x / setup.length,
        energy_imbalance,
    };
    let numbers = [
        measured.nu_hot,
        measured.u_max,
        measured.u_max_y,
        measured.w_max,
        measured.w_max_x,
        measured.energy_imbalance,
    ];
    if numbers.iter().any(|n| !n.is_finite()) {
        return Err(unusable("a measured number is not finite"));
    }
    Ok(measured)
}

/// Read a finished run for a study. A study that is not the heated cavity is refused.
pub fn measure(files: &dyn ResultFiles, study: &ResolvedStudy) -> Result<Measured, Refusal> {
    let setup = Setup::from_study(study).map_err(|found| {
        let list: Vec<String> = found.iter().map(ToString::to_string).collect();
        unusable(format!(
            "the study is not the heated cavity: {}",
            list.join("; ")
        ))
    })?;
    measure_with(files, &setup)
}
