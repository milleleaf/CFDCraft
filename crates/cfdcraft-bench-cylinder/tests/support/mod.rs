//! A study of the benchmark, and a results tree held in memory.

#![allow(dead_code)] // each test file uses a different part of this module

use std::collections::BTreeMap;

use cfdcraft_bench_cylinder::fixture::{self, Run};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{ResolvedStudy, Study, resolve};

/// The set-up of the benchmark: a 2.2 m by 0.41 m channel, a cylinder 0.1 m across at (0.2, 0.2),
/// a parabolic inflow of mean speed 1 m/s, transient laminar flow in steps of 1 ms, the force
/// monitor on the cylinder and a pressure probe on each side of it.
const STUDY: &str = r#"{
  "schema_version": 1,
  "domain": {
    "length_x": {"value": 2.2, "unit": "m"},
    "length_y": {"value": 0.41, "unit": "m"},
    "length_z": {"value": 0.01, "unit": "m"},
    "open": true,
    "shape": {
      "kind": "cylinder_channel",
      "diameter": {"value": 0.1, "unit": "m"},
      "centre_x": {"value": 0.2, "unit": "m"},
      "centre_y": {"value": 0.2, "unit": "m"}
    }
  },
  "fluid": {"kinematic_viscosity": {"value": 0.001, "unit": "m2/s"}},
  "boundary_conditions": {
    "inlet": {
      "faces": ["XMin"],
      "kind": "VelocityInlet",
      "velocity": {"value": 1.0, "unit": "m/s"},
      "profile": "parabolic"
    },
    "outlet": {"faces": ["XMax"], "kind": "PressureOutlet"},
    "channel_walls": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall"},
    "cylinder": {"faces": ["Obstacle"], "kind": "NoSlipWall"}
  },
  "mesh": {"cells_x": 114, "cells_y": 44, "cells_z": 1},
  "solver": {
    "solver": "FromPhysics",
    "end_time": {"value": 10, "unit": "s"},
    "write_interval": {"value": 1, "unit": "s"},
    "convergence": {"p": 1e-06, "U": 1e-06}
  },
  "physics": {"time": {"mode": "transient", "delta_t": {"value": 0.001, "unit": "s"}}},
  "monitors": {
    "centreline_u_sample_count": 32,
    "probes": {
      "front": {
        "x": {"value": 0.15, "unit": "m"}, "y": {"value": 0.2, "unit": "m"},
        "field": "pressure", "boundary": "cylinder"
      },
      "rear": {
        "x": {"value": 0.25, "unit": "m"}, "y": {"value": 0.2, "unit": "m"},
        "field": "pressure", "boundary": "cylinder"
      }
    },
    "forces": {
      "cylinder_forces": {
        "boundary": "cylinder",
        "reference_velocity": {"value": 1, "unit": "m/s"},
        "reference_length": {"value": 0.1, "unit": "m"}
      }
    }
  }
}"#;

/// The resolved benchmark study.
pub fn study() -> ResolvedStudy {
    let study = Study::from_json(STUDY).unwrap_or_else(|e| panic!("{e}"));
    resolve(&study).unwrap_or_else(|e| panic!("{e}"))
}

/// A results tree in memory.
#[derive(Default)]
pub struct Tree(pub BTreeMap<String, String>);

impl Tree {
    pub fn of(study: &ResolvedStudy, run: &Run) -> Self {
        Self(fixture::tree(study, run).into_iter().collect())
    }

    pub fn put(&mut self, path: &str, text: String) {
        self.0.insert(path.to_owned(), text);
    }

    pub fn remove_prefix(&mut self, prefix: &str) {
        self.0.retain(|path, _| !path.starts_with(prefix));
    }
}

impl ResultFiles for Tree {
    fn read(&self, rel: &str) -> Option<String> {
        self.0.get(rel).cloned()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        let prefix = format!("{rel_dir}/");
        let mut names: Vec<String> = self
            .0
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.dedup();
        names
    }
}

/// Within a relative tolerance.
pub fn close(found: f64, wanted: f64, relative: f64) -> bool {
    (found - wanted).abs() <= relative * wanted.abs()
}
