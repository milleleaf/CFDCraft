//! Studies for the tests of the solver-plan wiring in the runner side, written as JSON text. The
//! lid-driven cavity is the base for the transient and potential-flow ones; a closed heated
//! cavity, a cylinder channel and a backward step cover the buoyant and shaped cases. Every study
//! goes through `resolve`, so a combination the model refuses fails the test that builds it.
#![allow(dead_code)]

use cfdcraft_model::{ResolvedStudy, Study, resolve};
use serde_json::{Value, json};

pub const CAVITY: &str = include_str!("../../../cfdcraft-model/tests/fixtures/cavity_v1.json");

pub fn resolved(text: &str) -> ResolvedStudy {
    resolve(&Study::from_json(text).unwrap()).unwrap_or_else(|e| panic!("{e}"))
}

/// The cavity with `change` applied to its JSON.
fn edited(change: impl FnOnce(&mut Value)) -> String {
    let mut study: Value = serde_json::from_str(CAVITY).unwrap();
    change(&mut study);
    study.to_string()
}

/// The cavity text whose solver follows the physics given.
fn with_physics(physics: Value, end: f64, interval: f64) -> String {
    edited(|study| {
        study["solver"]["solver"] = json!("FromPhysics");
        study["solver"]["end_time"]["value"] = json!(end);
        study["solver"]["write_interval"]["value"] = json!(interval);
        study["physics"] = physics;
    })
}

/// The text of the lid-driven cavity run as a transient laminar flow to 8 s, written every 0.5 s.
pub fn transient_cavity_text() -> String {
    let physics = json!({"time": {"mode": "transient", "delta_t": {"value": 0.001, "unit": "s"}}});
    with_physics(physics, 8.0, 0.5)
}

/// The text of the lid-driven cavity started from a potential-flow solution, steady and laminar.
pub fn potential_cavity_text() -> String {
    with_physics(json!({"init": "potential"}), 5000.0, 500.0)
}

pub fn transient_cavity() -> ResolvedStudy {
    resolved(&transient_cavity_text())
}

pub fn potential_cavity() -> ResolvedStudy {
    resolved(&potential_cavity_text())
}

/// The cylinder channel (0.41 m high, 0.01 m deep, a 0.1 m cylinder at 0.2 m, 0.2 m) as a transient
/// laminar study. `monitors` is empty or starts with a comma and holds more members of the
/// `monitors` object.
pub fn cylinder_json(monitors: &str) -> String {
    format!(
        r#"{{"schema_version":1,"domain":{{"length_x":{{"value":2.2,"unit":"m"}},"length_y":{{"value":0.41,"unit":"m"}},"length_z":{{"value":0.01,"unit":"m"}},"open":true,"shape":{{"kind":"cylinder_channel","diameter":{{"value":0.1,"unit":"m"}},"centre_x":{{"value":0.2,"unit":"m"}},"centre_y":{{"value":0.2,"unit":"m"}}}}}},"fluid":{{"kinematic_viscosity":{{"value":0.001,"unit":"m2/s"}}}},"boundary_conditions":{{"inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{{"value":1.0,"unit":"m/s"}}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"channel_walls":{{"faces":["YMin","YMax"],"kind":"NoSlipWall"}},"cylinder":{{"faces":["Obstacle"],"kind":"NoSlipWall"}}}},"mesh":{{"cells_x":114,"cells_y":44,"cells_z":1}},"solver":{{"solver":"FromPhysics","end_time":{{"value":1,"unit":"s"}},"write_interval":{{"value":1,"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"physics":{{"time":{{"mode":"transient","delta_t":{{"value":0.001,"unit":"s"}}}}}},"monitors":{{"centreline_u_sample_count":64{monitors}}}}}"#
    )
}

/// A force monitor on the cylinder and two pressure probes on its front and rear points, named by
/// these labels.
pub fn cylinder_monitors(force: &str, front: &str, rear: &str) -> String {
    format!(
        r#","forces":{{"{force}":{{"boundary":"cylinder","reference_velocity":{{"value":1,"unit":"m/s"}},"reference_length":{{"value":0.1,"unit":"m"}}}}}},"probes":{{"{front}":{{"x":{{"value":0.15,"unit":"m"}},"y":{{"value":0.2,"unit":"m"}},"field":"pressure","boundary":"cylinder"}},"{rear}":{{"x":{{"value":0.25,"unit":"m"}},"y":{{"value":0.2,"unit":"m"}},"field":"pressure","boundary":"cylinder"}}}}"#
    )
}

/// The closed cavity heated from the left and cooled from the right with buoyancy on; `monitors`
/// is empty or starts with a comma.
pub fn heated_json(monitors: &str) -> String {
    format!(
        r#"{{"schema_version":1,"domain":{{"length_x":{{"value":0.1,"unit":"m"}},"length_y":{{"value":0.1,"unit":"m"}},"length_z":{{"value":0.05,"unit":"m"}},"open":false}},"fluid":{{"kinematic_viscosity":{{"value":0.0001,"unit":"m2/s"}}}},"boundary_conditions":{{"hot":{{"faces":["XMin"],"kind":"NoSlipWall","thermal":{{"kind":"fixed_temperature","value":{{"value":300.5,"unit":"K"}}}}}},"cold":{{"faces":["XMax"],"kind":"NoSlipWall","thermal":{{"kind":"fixed_temperature","value":{{"value":299.5,"unit":"K"}}}}}},"adiabatic":{{"faces":["YMin","YMax"],"kind":"NoSlipWall","thermal":{{"kind":"adiabatic"}}}}}},"mesh":{{"cells_x":128,"cells_y":128,"cells_z":1}},"solver":{{"solver":"FromPhysics","end_time":{{"value":5000,"unit":"s"}},"write_interval":{{"value":500,"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"physics":{{"buoyancy":{{"gravity":9.81,"beta":0.003,"reference_temperature":{{"value":300,"unit":"K"}},"prandtl":0.71,"turbulent_prandtl":0.85}}}},"monitors":{{"centreline_u_sample_count":64{monitors}}}}}"#
    )
}

/// Heat-flux surfaces on the cold and the hot wall, named by these labels. The order of the labels
/// decides the numbering, so keep the cold label before the hot one.
pub fn heated_surfaces(cold: &str, hot: &str) -> String {
    format!(
        r#","surfaces":{{"{cold}":{{"boundary":"cold","quantity":"heat_flux"}},"{hot}":{{"boundary":"hot","quantity":"heat_flux"}}}}"#
    )
}

/// A heat-flux surface on the hot wall alone.
pub fn hot_surface_only(hot: &str) -> String {
    format!(r#","surfaces":{{"{hot}":{{"boundary":"hot","quantity":"heat_flux"}}}}"#)
}

/// The backward step with a start-up section, 220 by 114 cells across the two reference lines,
/// steady and laminar.
pub fn step_json() -> String {
    r#"{"schema_version":1,"domain":{"length_x":{"value":2.286,"unit":"m"},"length_y":{"value":0.1143,"unit":"m"},"length_z":{"value":0.0127,"unit":"m"},"open":true,"shape":{"kind":"backward_step","step_height":{"value":12.7,"unit":"mm"},"upstream_length":{"value":1.651,"unit":"m"},"startup_length":{"value":0.254,"unit":"m"}}},"fluid":{"kinematic_viscosity":{"value":0.0000156,"unit":"m2/s"}},"boundary_conditions":{"inlet":{"faces":["XMin"],"kind":"VelocityInlet","velocity":{"value":44.2,"unit":"m/s"}},"outlet":{"faces":["XMax"],"kind":"PressureOutlet"},"floor_walls":{"faces":["YMin"],"kind":"NoSlipWall"},"top_wall":{"faces":["YMax"],"kind":"NoSlipWall"}},"mesh":{"cells_x":220,"cells_y":114,"cells_z":1},"solver":{"solver":"SimpleLaminar","end_time":{"value":100,"unit":"s"},"write_interval":{"value":100,"unit":"s"},"convergence":{"p":1e-06,"U":1e-06}},"monitors":{"centreline_u_sample_count":64}}"#
        .to_owned()
}
