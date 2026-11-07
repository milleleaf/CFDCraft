//! Studies for the solver plan tests, written as JSON text. The lid-driven cavity is the base for
//! the steady, transient and turbulent ones; a closed cavity heated from one side is the base for
//! the buoyant ones. Every study goes through `resolve`, so a combination the model refuses fails
//! the test that builds it.
#![allow(dead_code)]

use cfdcraft_model::{ResolvedStudy, Study, resolve};

const CAVITY: &str = include_str!("../../../cfdcraft-model/tests/fixtures/cavity_v1.json");

/// A closed cavity with a hot wall, a cold wall and two adiabatic walls. The temperatures are
/// written as floats so they are read as kelvin values, not counts.
const HEATED: &str = r#"{
  "schema_version": 1,
  "physics": { @HEAD@ "buoyancy": {
    "gravity": 9.81,
    "beta": 0.003,
    "reference_temperature": {"value": 300.0, "unit": "K"},
    "prandtl": 0.71,
    "turbulent_prandtl": 0.85
  } },
  "domain": {
    "length_x": {"value": 0.1, "unit": "m"},
    "length_y": {"value": 0.1, "unit": "m"},
    "length_z": {"value": 0.05, "unit": "m"},
    "open": false
  },
  "fluid": {"kinematic_viscosity": {"value": 0.0001, "unit": "m2/s"}},
  "boundary_conditions": {
    "hot": {"faces": ["XMin"], "kind": "NoSlipWall",
      "thermal": {"kind": "fixed_temperature", "value": {"value": 300.5, "unit": "K"}}},
    "cold": {"faces": ["XMax"], "kind": "NoSlipWall",
      "thermal": {"kind": "fixed_temperature", "value": {"value": 299.5, "unit": "K"}}},
    "adiabatic": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall",
      "thermal": {"kind": "adiabatic"}}
  },
  "mesh": {"cells_x": 32, "cells_y": 32, "cells_z": 1},
  "solver": {
    "solver": "FromPhysics",
    "end_time": {"value": @END@, "unit": "s"},
    "write_interval": {"value": @INTERVAL@, "unit": "s"},
    "convergence": {"p": 1e-06, "U": 1e-06}
  },
  "monitors": {"centreline_u_sample_count": 64}
}"#;

/// The physics of a transient run with a step of one millisecond at the coarsest level.
pub const TRANSIENT: &str =
    r#""time": {"mode": "transient", "delta_t": {"value": 0.001, "unit": "s"}},"#;

/// The same, as a whole physics object.
pub const TRANSIENT_ONLY: &str =
    r#"{"time": {"mode": "transient", "delta_t": {"value": 0.001, "unit": "s"}}}"#;

pub fn resolve_text(text: &str) -> ResolvedStudy {
    let study = Study::from_json(text).unwrap();
    resolve(&study).unwrap_or_else(|e| panic!("{e}"))
}

/// The lid-driven cavity as a study whose solver follows `physics` (a JSON object text), with the
/// end time and the write interval in seconds and a mesh level.
pub fn cavity(physics: &str, end: &str, interval: &str, level: u32) -> ResolvedStudy {
    let text = CAVITY
        .replace("\"SimpleLaminar\"", "\"FromPhysics\"")
        .replace("\"value\": 5000,", &format!("\"value\": {end},"))
        .replace("\"value\": 500,", &format!("\"value\": {interval},"))
        .replace(
            "\"cells_z\": 1",
            &format!("\"cells_z\": 1, \"level\": {level}"),
        )
        .replace(
            "\"schema_version\": 1,",
            &format!("\"schema_version\": 1, \"physics\": {physics},"),
        );
    resolve_text(&text)
}

/// The heated cavity; `head` is empty or holds physics members that end with a comma.
pub fn heated(head: &str, end: &str, interval: &str) -> ResolvedStudy {
    resolve_text(
        &HEATED
            .replace("@HEAD@", head)
            .replace("@END@", end)
            .replace("@INTERVAL@", interval),
    )
}
