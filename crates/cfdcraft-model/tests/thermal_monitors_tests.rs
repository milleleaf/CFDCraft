use cfdcraft_model::{
    InletProfile, ResolvedStudy, ResolvedThermal, Study, SurfaceQuantity, canonical_hash, resolve,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("fixtures/channel_v1.json");

fn value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn resolved(v: &Value) -> ResolvedStudy {
    let study = Study::from_json(&v.to_string()).unwrap();
    resolve(&study).unwrap_or_else(|e| panic!("{e}"))
}

fn m(v: f64) -> Value {
    json!({"value": v, "unit": "m"})
}

fn kelvin(v: f64) -> Value {
    json!({"value": v, "unit": "K"})
}

/// A closed cavity heated from the left and cooled from the right, with buoyancy on.
fn heated() -> Value {
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["boundary_conditions"] = json!({
        "hot": {"faces": ["XMin"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": kelvin(300.5)}},
        "cold": {"faces": ["XMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": kelvin(299.5)}},
        "adiabatic": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "adiabatic"}}
    });
    v["physics"] = json!({"buoyancy": {
        "gravity": 9.81,
        "beta": 0.003,
        "reference_temperature": kelvin(300.0),
        "prandtl": 0.71,
        "turbulent_prandtl": 0.85
    }});
    v
}

/// A cylinder channel: 0.1 m across at (0.2, 0.2) in 2.2 m by 0.41 m, with its own wall.
fn cylinder() -> Value {
    let mut v = value(CHANNEL);
    v["domain"]["length_x"] = m(2.2);
    v["domain"]["length_y"] = m(0.41);
    v["domain"]["shape"] = json!({
        "kind": "cylinder_channel",
        "diameter": m(0.1),
        "centre_x": m(0.2),
        "centre_y": m(0.2),
    });
    v["boundary_conditions"]["cylinder"] = json!({"faces": ["Obstacle"], "kind": "NoSlipWall"});
    v["mesh"]["cells_x"] = 114.into();
    v["mesh"]["cells_y"] = 44.into();
    v
}

/// The backward step of the reference experiment, with its start-up section.
fn step() -> Value {
    let mut v = value(CHANNEL);
    v["domain"]["length_x"] = m(2.286);
    v["domain"]["length_y"] = m(0.1143);
    v["domain"]["shape"] = json!({
        "kind": "backward_step",
        "step_height": {"value": 12.7, "unit": "mm"},
        "upstream_length": m(1.651),
        "startup_length": m(0.254),
    });
    v["mesh"]["cells_x"] = 220.into();
    v["mesh"]["cells_y"] = 114.into();
    v
}

#[test]
fn a_profile_and_a_thermal_condition_resolve() {
    let mut channel = value(CHANNEL);
    channel["boundary_conditions"]["inlet"]["profile"] = "parabolic".into();
    let r = resolved(&channel);
    assert_eq!(
        r.boundary_conditions["inlet"].profile,
        Some(InletProfile::Parabolic)
    );
    assert_eq!(r.boundary_conditions["floor"].profile, None);

    let r = resolved(&heated());
    assert_eq!(
        r.boundary_conditions["hot"].thermal,
        Some(ResolvedThermal::FixedTemperature { value: 300.5 })
    );
    assert_eq!(
        r.boundary_conditions["adiabatic"].thermal,
        Some(ResolvedThermal::Adiabatic)
    );
}

#[test]
fn a_force_monitor_resolves() {
    let mut v = cylinder();
    v["monitors"]["forces"] = json!({"cyl": {
        "boundary": "cylinder",
        "reference_velocity": {"value": 1, "unit": "m/s"},
        "reference_length": {"value": 100, "unit": "mm"}
    }});
    let r = resolved(&v);
    let force = &r.monitors.forces[0];
    assert_eq!(force.label, "cyl");
    assert_eq!(force.boundary, "cylinder");
    assert_eq!(force.reference_velocity, 1.0);
    assert_eq!(force.reference_length, 0.1);
    let plan = r.monitor_plan();
    assert_eq!(plan.forces.len(), 1);
    assert_eq!(plan.forces[0].fo, "force_1");
    assert_eq!(plan.forces[0].label.as_deref(), Some("cyl"));
    assert!(!plan.defaulted && plan.probes.is_empty());
}

#[test]
fn a_boundary_probe_resolves() {
    let mut v = cylinder();
    v["monitors"]["probes"] = json!({"rear": {
        "x": m(0.25), "y": m(0.2), "field": "pressure", "boundary": "cylinder"
    }});
    let r = resolved(&v);
    assert_eq!(r.monitors.probes[0].boundary.as_deref(), Some("cylinder"));
    let plan = r.monitor_plan();
    assert_eq!(plan.probes.len(), 1);
    assert_eq!(plan.probes[0].boundary.as_deref(), Some("cylinder"));
    assert!(!plan.defaulted);
    // A probe in the flow names no boundary.
    let mut v = value(CAVITY);
    v["monitors"]["probes"] = json!({"mid": {"x": m(0.05), "y": m(0.05), "field": "velocity"}});
    assert_eq!(resolved(&v).monitor_plan().probes[0].boundary, None);
}

#[test]
fn heat_flux_is_a_surface_quantity() {
    let mut v = heated();
    v["monitors"]["surfaces"] = json!({"hot_flux": {"boundary": "hot", "quantity": "heat_flux"}});
    let r = resolved(&v);
    assert_eq!(r.monitors.surfaces[0].quantity, SurfaceQuantity::HeatFlux);
    assert_eq!(
        r.monitor_plan().surfaces[0].quantity,
        SurfaceQuantity::HeatFlux
    );
}

#[test]
fn the_cell_count_and_volume_follow_the_shape() {
    let mut v = value(CAVITY);
    v["mesh"]["cells_x"] = 32.into();
    v["mesh"]["cells_y"] = 32.into();
    v["mesh"]["level"] = 2.into();
    let r = resolved(&v);
    let expected = r.mesh.cells_x * r.mesh.cells_y * r.mesh.cells_z;
    assert_eq!(expected, 128 * 128);
    assert_eq!(r.cell_count(), Some(expected));
    assert!((r.domain_volume() - 0.1 * 0.1 * 0.05).abs() < 1e-15);

    // The step lays out four blocks: (2 + 88) x 66 cells upstream and 130 x (48 + 66) downstream.
    let r = resolved(&step());
    assert_eq!(r.cell_count(), Some((2 + 88) * 66 + 130 * (48 + 66)));
    let area = 2.286 * 0.1143 - 1.651 * 0.0127;
    assert!((r.domain_volume() - area * 0.01).abs() < 1e-12);
    // No circle, so the mesh fills the outline exactly.
    assert_eq!(r.meshed_volume(), r.domain_volume());
    let mut refined = step();
    refined["mesh"]["level"] = 1.into();
    assert_eq!(
        resolved(&refined).cell_count(),
        r.cell_count().map(|c| c * 4)
    );
}

#[test]
fn old_studies_have_no_new_members() {
    let text = serde_jcs::to_string(&resolved(&value(CAVITY))).unwrap();
    for member in ["profile", "thermal", "forces", "boundary", "physics"] {
        assert!(!text.contains(&format!("\"{member}\"")), "{member}");
    }
    assert_eq!(
        canonical_hash(&resolved(&value(CAVITY))),
        "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0"
    );
}
