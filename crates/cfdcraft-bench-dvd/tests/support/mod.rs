#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use cfdcraft_bench_dvd::{PRANDTL, rayleigh};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{ResolvedStudy, Study, resolve};

pub const GRAVITY: f64 = 9.81;
pub const BETA: f64 = 3e-3;

/// A result tree in memory.
#[derive(Default)]
pub struct Tree(pub BTreeMap<String, String>);

impl Tree {
    pub fn from_pairs(pairs: Vec<(String, String)>) -> Tree {
        Tree(pairs.into_iter().collect())
    }

    pub fn put(&mut self, rel: &str, text: &str) {
        self.0.insert(rel.to_owned(), text.to_owned());
    }
}

fn refused(rel: &str) -> bool {
    rel.starts_with('/') || rel.split('/').any(|part| part == "..")
}

impl ResultFiles for Tree {
    fn read(&self, rel: &str) -> Option<String> {
        if refused(rel) {
            return None;
        }
        self.0.get(rel).cloned()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        if refused(rel_dir) {
            return Vec::new();
        }
        let prefix = format!("{rel_dir}/");
        let names: BTreeSet<String> = self
            .0
            .keys()
            .filter_map(|key| key.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.into_iter().collect()
    }
}

/// What a study may differ in. The default is a heated cavity at Ra 1e4 that matches.
#[derive(Debug, Clone)]
pub struct Case {
    pub length_x: f64,
    pub length_y: f64,
    pub ra: f64,
    pub prandtl: f64,
    /// Buoyancy, the thermal conditions and the walls of the heated cavity. Without it the study
    /// is a plain lid-driven cavity, which cannot carry heat-flux surfaces.
    pub heated: bool,
    /// Put the hot wall on XMax and the cold one on XMin.
    pub swapped: bool,
    pub hot_flux: bool,
    pub cold_flux: bool,
    pub mid_vertical: bool,
    pub mid_horizontal: bool,
    /// A transient run with a step of five seconds.
    pub transient: bool,
}

impl Default for Case {
    fn default() -> Self {
        Self {
            length_x: 1.0,
            length_y: 1.0,
            ra: 1.0e4,
            prandtl: PRANDTL,
            heated: true,
            swapped: false,
            hot_flux: true,
            cold_flux: true,
            mid_vertical: true,
            mid_horizontal: true,
            transient: false,
        }
    }
}

const TEMPLATE: &str = r#"{
  "schema_version": 1,
  "domain": {
    "length_x": {"value": @LX@, "unit": "m"},
    "length_y": {"value": @LY@, "unit": "m"},
    "length_z": {"value": 0.1, "unit": "m"},
    "open": false
  },
  "fluid": {"kinematic_viscosity": {"value": @NU@, "unit": "m2/s"}},
  "boundary_conditions": {@BOUNDARIES@},
  "mesh": {"cells_x": 40, "cells_y": 40, "cells_z": 1},
  "solver": {
    "solver": "@SOLVER@",
    "end_time": {"value": 5000, "unit": "s"},
    "write_interval": {"value": 500, "unit": "s"},
    "convergence": {"p": 1e-06, "U": 1e-06}
  },
  @PHYSICS@
  "monitors": {"centreline_u_sample_count": 64@SURFACES@@LINES@}
}"#;

fn kelvin(v: f64) -> String {
    format!(r#"{{"value": {v}, "unit": "K"}}"#)
}

/// The study as JSON text. The viscosity is the one that gives the case's Rayleigh number for a
/// temperature difference of one kelvin.
pub fn study_json(c: &Case) -> String {
    let nu = (GRAVITY * BETA * c.length_x.powi(3) * c.prandtl / c.ra).sqrt();
    let (hot_face, cold_face) = if c.swapped {
        ("XMax", "XMin")
    } else {
        ("XMin", "XMax")
    };
    let boundaries = if c.heated {
        format!(
            r#""hot": {{"faces": ["{hot_face}"], "kind": "NoSlipWall", "thermal": {{"kind": "fixed_temperature", "value": {hot}}}}},
    "cold": {{"faces": ["{cold_face}"], "kind": "NoSlipWall", "thermal": {{"kind": "fixed_temperature", "value": {cold}}}}},
    "adiabatic": {{"faces": ["YMin", "YMax"], "kind": "NoSlipWall", "thermal": {{"kind": "adiabatic"}}}}"#,
            hot = kelvin(300.5),
            cold = kelvin(299.5),
        )
    } else {
        r#""lid": {"faces": ["YMax"], "kind": "MovingWall", "velocity": {"value": 1, "unit": "m/s"}},
    "walls": {"faces": ["XMin", "XMax", "YMin"], "kind": "NoSlipWall"}"#
            .to_owned()
    };
    let physics = if c.heated {
        let time = if c.transient {
            r#""time": {"mode": "transient", "delta_t": {"value": 5, "unit": "s"}}, "#
        } else {
            ""
        };
        format!(
            r#""physics": {{{time}"buoyancy": {{"gravity": {GRAVITY}, "beta": {BETA}, "reference_temperature": {reference}, "prandtl": {pr}, "turbulent_prandtl": 0.85}}}},"#,
            reference = kelvin(300.0),
            pr = c.prandtl,
        )
    } else {
        String::new()
    };
    let mut surfaces = Vec::new();
    if c.heated && c.cold_flux {
        surfaces.push(r#""cold_flux": {"boundary": "cold", "quantity": "heat_flux"}"#);
    }
    if c.heated && c.hot_flux {
        surfaces.push(r#""hot_flux": {"boundary": "hot", "quantity": "heat_flux"}"#);
    }
    let surfaces = if surfaces.is_empty() {
        String::new()
    } else {
        format!(r#", "surfaces": {{{}}}"#, surfaces.join(", "))
    };
    let metres = |v: f64| format!(r#"{{"value": {v}, "unit": "m"}}"#);
    let (lx, ly) = (c.length_x, c.length_y);
    let mut lines = Vec::new();
    if c.mid_horizontal {
        lines.push(format!(
            r#""mid_horizontal": {{"x0": {}, "y0": {}, "x1": {}, "y1": {}, "samples": 41, "field": "velocity"}}"#,
            metres(0.0),
            metres(ly / 2.0),
            metres(lx),
            metres(ly / 2.0)
        ));
    }
    if c.mid_vertical {
        lines.push(format!(
            r#""mid_vertical": {{"x0": {}, "y0": {}, "x1": {}, "y1": {}, "samples": 41, "field": "velocity"}}"#,
            metres(lx / 2.0),
            metres(0.0),
            metres(lx / 2.0),
            metres(ly)
        ));
    }
    let lines = if lines.is_empty() {
        String::new()
    } else {
        format!(r#", "lines": {{{}}}"#, lines.join(", "))
    };
    TEMPLATE
        .replace("@LX@", &lx.to_string())
        .replace("@LY@", &ly.to_string())
        .replace("@NU@", &nu.to_string())
        .replace("@BOUNDARIES@", &boundaries)
        .replace(
            "@SOLVER@",
            if c.heated {
                "FromPhysics"
            } else {
                "SimpleLaminar"
            },
        )
        .replace("@PHYSICS@", &physics)
        .replace("@SURFACES@", &surfaces)
        .replace("@LINES@", &lines)
}

pub fn study(c: &Case) -> ResolvedStudy {
    let text = study_json(c);
    let parsed = Study::from_json(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    resolve(&parsed).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

/// The Rayleigh number of the study's own numbers, for a check of the builder.
pub fn rayleigh_of(c: &Case) -> f64 {
    let nu = (GRAVITY * BETA * c.length_x.powi(3) * c.prandtl / c.ra).sqrt();
    rayleigh(GRAVITY, BETA, 1.0, c.length_x, nu, c.prandtl)
}
