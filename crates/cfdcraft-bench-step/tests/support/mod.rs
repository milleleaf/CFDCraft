//! Shared test helpers: studies as JSON text, results in memory and sample-line text.
#![allow(dead_code)]

use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use std::collections::BTreeMap;

/// Files in memory, with the refusals every implementation owes.
#[derive(Default)]
pub struct MapFiles(pub BTreeMap<String, String>);

fn safe(rel: &str) -> bool {
    !rel.starts_with('/') && !rel.split('/').any(|part| part == "..")
}

impl ResultFiles for MapFiles {
    fn read(&self, rel: &str) -> Option<String> {
        safe(rel).then(|| self.0.get(rel).cloned()).flatten()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        if !safe(rel_dir) {
            return Vec::new();
        }
        let dir = rel_dir.trim_end_matches('/');
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let mut names: Vec<String> = self
            .0
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

pub fn q(value: f64, unit: &str) -> String {
    format!(r#"{{"value":{value},"unit":"{unit}"}}"#)
}

/// The labels of the lines the turbulent step reads.
pub const ALL_LINES: [&str; 5] = [
    "floor",
    "profile_x1",
    "profile_x4",
    "profile_x6",
    "profile_x10",
];

/// A backward-step study as JSON text. Dimensions follow the step height `h`: the domain is 180
/// step heights long and `height_h` high, the step is 130 heights from the inlet, and the first
/// 20 heights of the inlet are a symmetry section.
pub struct Ds {
    pub h: f64,
    pub height_h: f64,
    pub speed: f64,
    pub nu: f64,
    pub profile: Option<&'static str>,
    pub lines: Vec<&'static str>,
    pub turbulent: bool,
    /// The height of the floor line above the floor, in step heights.
    pub floor_height_h: f64,
    /// How far every profile line lies downstream of its station, in step heights.
    pub station_offset_h: f64,
}

impl Default for Ds {
    fn default() -> Self {
        Self {
            h: 0.0127,
            height_h: 9.0,
            speed: 44.2,
            nu: 1.56e-5,
            profile: None,
            lines: ALL_LINES.to_vec(),
            turbulent: true,
            floor_height_h: 0.01,
            station_offset_h: 0.0,
        }
    }
}

impl Ds {
    pub fn json(&self) -> String {
        let h = self.h;
        let (length_x, length_y, up) = (180.0 * h, self.height_h * h, 130.0 * h);
        let mut lines = Vec::new();
        for label in &self.lines {
            let line = if *label == "floor" {
                let y = self.floor_height_h * h;
                format!(
                    r#""floor":{{"x0":{},"y0":{},"x1":{},"y1":{},"samples":101,"field":"velocity"}}"#,
                    q(up, "m"),
                    q(y, "m"),
                    q(length_x, "m"),
                    q(y, "m")
                )
            } else {
                let k: f64 = label.trim_start_matches("profile_x").parse().unwrap();
                let x = up + (k + self.station_offset_h) * h;
                format!(
                    r#""{label}":{{"x0":{},"y0":{},"x1":{},"y1":{},"samples":101,"field":"velocity"}}"#,
                    q(x, "m"),
                    q(0.0, "m"),
                    q(x, "m"),
                    q(length_y, "m")
                )
            };
            lines.push(line);
        }
        let profile = self
            .profile
            .map(|p| format!(r#","profile":"{p}""#))
            .unwrap_or_default();
        let (solver, physics) = if self.turbulent {
            (
                "FromPhysics",
                r#","physics":{"turbulence":{"model":"k_omega_sst","intensity":0.00061,"viscosity_ratio":0.000385}}"#,
            )
        } else {
            ("SimpleLaminar", "")
        };
        format!(
            r#"{{"schema_version":1,"domain":{{"length_x":{lx},"length_y":{ly},"length_z":{lz},"open":true,"shape":{{"kind":"backward_step","step_height":{sh},"upstream_length":{ul},"startup_length":{su}}}}},"fluid":{{"kinematic_viscosity":{nu}}},"boundary_conditions":{{"inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{v}{profile}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"floor_walls":{{"faces":["YMin"],"kind":"NoSlipWall"}},"top_wall":{{"faces":["YMax"],"kind":"NoSlipWall"}}}},"mesh":{{"cells_x":220,"cells_y":114,"cells_z":1}},"solver":{{"solver":"{solver}","end_time":{et},"write_interval":{wi},"convergence":{{"p":1e-06,"U":1e-06}}}},"monitors":{{"centreline_u_sample_count":64,"lines":{{{ls}}}}}{physics}}}"#,
            lx = q(length_x, "m"),
            ly = q(length_y, "m"),
            lz = q(h, "m"),
            sh = q(h, "m"),
            ul = q(up, "m"),
            su = q(20.0 * h, "m"),
            nu = q(self.nu, "m2/s"),
            v = q(self.speed, "m/s"),
            et = q(2000.0, "s"),
            wi = q(100.0, "s"),
            ls = lines.join(","),
        )
    }

    pub fn resolved(&self) -> ResolvedStudy {
        resolve(&Study::from_json(&self.json()).unwrap()).unwrap()
    }
}

/// The position of the line with this label among the resolved study's lines, counted from 1.
pub fn line_number(r: &ResolvedStudy, label: &str) -> usize {
    r.monitors
        .lines
        .iter()
        .position(|l| l.label == label)
        .unwrap()
        + 1
}

/// The path of a line's sample file in a time folder.
pub fn line_path(r: &ResolvedStudy, label: &str, folder: &str) -> String {
    format!(
        "postProcessing/line_{}/{folder}/line_U.xy",
        line_number(r, label)
    )
}

/// The floor line of a step run: Ux grows linearly with distance and changes sign `crossing`
/// step heights from the step, as 4 columns (distance, Ux, Uy, Uz).
pub fn floor_text(r: &ResolvedStudy, label: &str, h: f64, crossing: f64) -> String {
    let line = r.monitors.lines.iter().find(|l| l.label == label).unwrap();
    let length = line.x1 - line.x0;
    (0..=100)
        .map(|i| {
            let d = f64::from(i) * length / 100.0;
            format!(
                "{d:.12} {:.12} 0.000000000000 0.000000000000\n",
                d / h - crossing
            )
        })
        .collect()
}

/// A tree holding only the floor line of a step run, in the folder `2000`.
pub fn floor_tree(r: &ResolvedStudy, h: f64, crossing: f64) -> MapFiles {
    let mut files = MapFiles::default();
    files.0.insert(
        line_path(r, "floor", "2000"),
        floor_text(r, "floor", h, crossing),
    );
    files
}

/// The profile line at a station: the published profile with `shift` added to every velocity, as
/// 4 columns (distance from the bottom, Ux, Uy, Uz). Heights and speeds follow `h` and `uref`.
pub fn profile_text(
    h: f64,
    uref: f64,
    station: &cfdcraft_bench_step::Station,
    shift: f64,
) -> String {
    station
        .y_over_h
        .iter()
        .zip(&station.u_over_uref)
        .map(|(y, u)| {
            format!(
                "{:.12} {:.12} 0.000000000000 0.000000000000\n",
                y * h,
                (u + shift) * uref
            )
        })
        .collect()
}

/// A tree with the floor line and the four profile lines of a step run.
pub fn full_tree(r: &ResolvedStudy, h: f64, uref: f64, crossing: f64, shift: f64) -> MapFiles {
    let mut files = floor_tree(r, h, crossing);
    let profiles = cfdcraft_bench_step::DsProfiles::load().unwrap();
    for station in &profiles.stations {
        let label = format!("profile_x{}", station.x_over_h);
        files.0.insert(
            line_path(r, &label, "2000"),
            profile_text(h, uref, station, shift),
        );
    }
    files
}

/// A laminar step study as JSON text, with the geometry of the experiment: a step of `step` metres
/// in a channel `height` metres high downstream (4.9 mm and 10.1 mm by default), a parabolic inlet
/// at unit mean speed and a viscosity that gives the Reynolds number `re` (the speed times twice
/// the inlet height over the viscosity).
pub struct Ar {
    pub re: f64,
    pub step: f64,
    pub height: f64,
    pub profile: Option<&'static str>,
    pub turbulent: bool,
    pub lines: Vec<&'static str>,
}

impl Default for Ar {
    fn default() -> Self {
        Self {
            re: 200.0,
            step: 0.0049,
            height: 0.0101,
            profile: Some("parabolic"),
            turbulent: false,
            lines: vec!["floor"],
        }
    }
}

impl Ar {
    pub fn json(&self) -> String {
        let (s, ly) = (self.step, self.height);
        let (up, lx) = (0.1, 0.3);
        let nu = 2.0 * (ly - s) / self.re;
        let y = 0.01 * s;
        let lines = self
            .lines
            .iter()
            .map(|label| {
                format!(
                    r#""{label}":{{"x0":{},"y0":{},"x1":{},"y1":{},"samples":101,"field":"velocity"}}"#,
                    q(up, "m"),
                    q(y, "m"),
                    q(lx, "m"),
                    q(y, "m")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let profile = self
            .profile
            .map(|p| format!(r#","profile":"{p}""#))
            .unwrap_or_default();
        let (solver, physics) = if self.turbulent {
            (
                "FromPhysics",
                r#","physics":{"turbulence":{"model":"k_omega_sst","intensity":0.05,"viscosity_ratio":10.0}}"#,
            )
        } else {
            ("SimpleLaminar", "")
        };
        format!(
            r#"{{"schema_version":1,"domain":{{"length_x":{lx},"length_y":{ly},"length_z":{lz},"open":true,"shape":{{"kind":"backward_step","step_height":{sh},"upstream_length":{ul},"startup_length":{su}}}}},"fluid":{{"kinematic_viscosity":{nu}}},"boundary_conditions":{{"inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{v}{profile}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"floor_walls":{{"faces":["YMin"],"kind":"NoSlipWall"}},"top_wall":{{"faces":["YMax"],"kind":"NoSlipWall"}}}},"mesh":{{"cells_x":218,"cells_y":114,"cells_z":1}},"solver":{{"solver":"{solver}","end_time":{et},"write_interval":{wi},"convergence":{{"p":1e-06,"U":1e-06}}}},"monitors":{{"centreline_u_sample_count":64,"lines":{{{lines}}}}}{physics}}}"#,
            lx = q(lx, "m"),
            ly = q(ly, "m"),
            lz = q(ly, "m"),
            sh = q(s, "m"),
            ul = q(up, "m"),
            su = q(0.0, "m"),
            nu = q(nu, "m2/s"),
            v = q(1.0, "m/s"),
            et = q(2000.0, "s"),
            wi = q(100.0, "s"),
        )
    }

    pub fn resolved(&self) -> ResolvedStudy {
        resolve(&Study::from_json(&self.json()).unwrap()).unwrap()
    }
}
