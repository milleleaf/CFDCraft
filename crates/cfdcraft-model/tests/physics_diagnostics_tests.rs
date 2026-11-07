use cfdcraft_model::{
    Capabilities, Diagnostic, Quantity, Severity, Study, TimeMode, Unit, diagnose,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("fixtures/channel_v1.json");

fn value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn study(v: &Value) -> Study {
    Study::from_json(&v.to_string()).unwrap()
}

fn errors(study: &Study) -> Vec<Diagnostic> {
    diagnose(study, &Capabilities::unrestricted())
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn found(v: &Value) -> Vec<Diagnostic> {
    errors(&study(v))
}

fn has(list: &[Diagnostic], code: &str, pointer: &str) -> bool {
    list.iter().any(|d| d.code == code && d.pointer == pointer)
}

fn lacks(list: &[Diagnostic], code: &str) -> bool {
    !list.iter().any(|d| d.code == code)
}

fn m(v: f64) -> Value {
    json!({"value": v, "unit": "m"})
}

fn seconds(v: f64) -> Value {
    json!({"value": v, "unit": "s"})
}

fn kelvin(v: f64) -> Value {
    json!({"value": v, "unit": "K"})
}

/// The cavity run as a transient study of 10 s in steps of 1 ms.
fn transient() -> Value {
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["solver"]["end_time"] = seconds(10.0);
    v["solver"]["write_interval"] = seconds(1.0);
    v["physics"] = json!({"time": {"mode": "transient", "delta_t": seconds(0.001)}});
    v
}

/// A closed cavity with no moving wall: nothing drives the flow.
fn still() -> Value {
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["boundary_conditions"] = json!({
        "walls": {"faces": ["XMin", "XMax", "YMin", "YMax"], "kind": "NoSlipWall"}
    });
    v
}

/// The lid-driven cavity with a RAS model and sound inflow values.
fn turbulent() -> Value {
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["physics"] = json!({"turbulence": {
        "model": "k_omega_sst", "intensity": 0.05, "viscosity_ratio": 10.0
    }});
    v
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

fn channel() -> Value {
    let mut v = value(CHANNEL);
    v["solver"]["solver"] = "FromPhysics".into();
    v
}

/// A cylinder channel: 0.1 m across at (0.2, 0.2) in 2.2 m by 0.41 m, with its own wall.
fn cylinder() -> Value {
    let mut v = channel();
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

fn step(startup: f64) -> Value {
    let mut v = channel();
    v["domain"]["length_x"] = m(2.286);
    v["domain"]["length_y"] = m(0.1143);
    v["domain"]["shape"] = json!({
        "kind": "backward_step",
        "step_height": {"value": 12.7, "unit": "mm"},
        "upstream_length": m(1.651),
        "startup_length": m(startup),
    });
    let cells_x: u64 = if startup > 0.0 { 220 } else { 218 };
    v["mesh"]["cells_x"] = cells_x.into();
    v["mesh"]["cells_y"] = 114.into();
    v
}

fn force(boundary: &str) -> Value {
    json!({
        "boundary": boundary,
        "reference_velocity": {"value": 1, "unit": "m/s"},
        "reference_length": m(0.1)
    })
}

#[test]
fn the_bases_are_sound() {
    for (name, v) in [
        ("transient", transient()),
        ("turbulent", turbulent()),
        ("heated", heated()),
        ("cylinder", cylinder()),
        ("step", step(0.254)),
        ("still", still()),
    ] {
        assert!(found(&v).is_empty(), "{name}: {:?}", found(&v));
    }
}

#[test]
fn transient_needs_a_usable_step() {
    let step_pointer = "/physics/time/delta_t";
    for bad in [0.0, -0.001, 10.0, 20.0] {
        let mut v = transient();
        v["physics"]["time"]["delta_t"] = seconds(bad);
        assert!(has(&found(&v), "CFD-PHYS-001", step_pointer), "{bad}");
    }
    let mut not_a_number = study(&transient());
    not_a_number.physics.as_mut().unwrap().time = TimeMode::Transient {
        delta_t: Quantity {
            value: f64::NAN,
            unit: Unit::Second,
        },
    };
    assert!(has(&errors(&not_a_number), "CFD-PHYS-001", step_pointer));
    // The end time is not a whole multiple of the write interval.
    let mut v = transient();
    v["solver"]["write_interval"] = seconds(3.0);
    assert!(has(&found(&v), "CFD-PHYS-001", "/solver/write_interval"));
    // 10 000 s in steps of 1 ms is 10 million steps.
    let mut v = transient();
    v["solver"]["end_time"] = seconds(10_000.0);
    assert!(has(&found(&v), "CFD-PHYS-001", step_pointer));
    // 3 million steps at level 0 become 6 million at level 1, because the step halves.
    let mut v = transient();
    v["solver"]["end_time"] = seconds(3000.0);
    v["solver"]["write_interval"] = seconds(1000.0);
    assert!(lacks(&found(&v), "CFD-PHYS-001"));
    v["mesh"]["level"] = 1.into();
    assert!(has(&found(&v), "CFD-PHYS-001", step_pointer));
    // Times of a transient run need not be whole seconds.
    let mut v = transient();
    v["solver"]["end_time"] = seconds(2.5);
    v["solver"]["write_interval"] = seconds(0.5);
    assert!(found(&v).is_empty(), "{:?}", found(&v));
}

#[test]
fn turbulence_needs_inlet_values_and_a_reference_velocity() {
    let mut v = turbulent();
    v["physics"]["turbulence"]
        .as_object_mut()
        .unwrap()
        .remove("intensity");
    assert!(has(
        &found(&v),
        "CFD-PHYS-002",
        "/physics/turbulence/intensity"
    ));
    for bad in [0.0, 1.5, -0.1] {
        let mut v = turbulent();
        v["physics"]["turbulence"]["intensity"] = bad.into();
        assert!(
            has(&found(&v), "CFD-PHYS-002", "/physics/turbulence/intensity"),
            "{bad}"
        );
    }
    let mut v = turbulent();
    v["physics"]["turbulence"]
        .as_object_mut()
        .unwrap()
        .remove("viscosity_ratio");
    assert!(has(
        &found(&v),
        "CFD-PHYS-002",
        "/physics/turbulence/viscosity_ratio"
    ));
    let mut v = turbulent();
    v["physics"]["turbulence"]["viscosity_ratio"] = json!(0.0);
    assert!(has(
        &found(&v),
        "CFD-PHYS-002",
        "/physics/turbulence/viscosity_ratio"
    ));
    // A closed cavity with no moving wall has no speed to build the inflow values from.
    let reference = "/physics/turbulence/reference_velocity";
    let mut v = still();
    v["physics"] = turbulent()["physics"].clone();
    assert!(has(&found(&v), "CFD-PHYS-002", reference));
    v["physics"]["turbulence"]["reference_velocity"] = json!({"value": 1, "unit": "m/s"});
    assert!(found(&v).is_empty(), "{:?}", found(&v));
    v["physics"]["turbulence"]["reference_velocity"] = json!({"value": 0, "unit": "m/s"});
    assert!(has(&found(&v), "CFD-PHYS-002", reference));
    // A laminar run needs none of it.
    let mut v = still();
    v["physics"] = json!({"turbulence": {"model": "laminar"}});
    assert!(found(&v).is_empty(), "{:?}", found(&v));
}

#[test]
fn buoyancy_values_are_checked() {
    for member in ["gravity", "beta", "prandtl", "turbulent_prandtl"] {
        for bad in [0.0, -1.0] {
            let mut v = heated();
            v["physics"]["buoyancy"][member] = bad.into();
            let pointer = format!("/physics/buoyancy/{member}");
            assert!(has(&found(&v), "CFD-PHYS-003", &pointer), "{member} {bad}");
        }
    }
    let mut v = heated();
    v["physics"]["buoyancy"]["reference_temperature"] = kelvin(0.0);
    assert!(has(
        &found(&v),
        "CFD-PHYS-003",
        "/physics/buoyancy/reference_temperature"
    ));
    // Beta times the 1 K between the walls is 0.2, above the 0.1 the model holds for.
    let mut v = heated();
    v["physics"]["buoyancy"]["beta"] = json!(0.2);
    let list = found(&v);
    let d = list
        .iter()
        .find(|d| d.code == "CFD-PHYS-003" && d.pointer == "/physics/buoyancy/beta")
        .expect("the product is refused");
    assert!(d.message.contains("0.2"), "{}", d.message);
}

#[test]
fn thermal_conditions_need_buoyancy_and_walls() {
    // A thermal condition with buoyancy off.
    let mut v = heated();
    v.as_object_mut().unwrap().remove("physics");
    assert!(has(
        &found(&v),
        "CFD-PHYS-004",
        "/boundary_conditions/hot/thermal"
    ));
    // Buoyancy with no wall at a fixed temperature.
    let mut v = heated();
    v["boundary_conditions"]["hot"]["thermal"] = json!({"kind": "adiabatic"});
    v["boundary_conditions"]["cold"]["thermal"] = json!({"kind": "adiabatic"});
    assert!(has(&found(&v), "CFD-PHYS-004", "/physics/buoyancy"));
    // A thermal condition on an inlet.
    let mut v = channel();
    v["boundary_conditions"]["floor"]["thermal"] =
        json!({"kind": "fixed_temperature", "value": kelvin(300.0)});
    v["boundary_conditions"]["inlet"]["thermal"] = json!({"kind": "adiabatic"});
    v["physics"] = heated()["physics"].clone();
    let list = found(&v);
    assert!(has(
        &list,
        "CFD-PHYS-004",
        "/boundary_conditions/inlet/thermal"
    ));
    assert!(!has(
        &list,
        "CFD-PHYS-004",
        "/boundary_conditions/floor/thermal"
    ));
    // Five different fixed temperatures are more than a case can name.
    let mut v = heated();
    for (i, t) in [300.1, 300.2, 300.3].into_iter().enumerate() {
        v["boundary_conditions"][format!("extra{i}")] = json!({
            "faces": ["YMin"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": kelvin(t)}
        });
    }
    assert!(has(&found(&v), "CFD-PHYS-007", "/boundary_conditions"));
    // Four are fine.
    v["boundary_conditions"]
        .as_object_mut()
        .unwrap()
        .remove("extra2");
    assert!(lacks(&found(&v), "CFD-PHYS-007"));
}

#[test]
fn potential_flow_needs_something_to_drive_it() {
    let mut v = heated();
    v["physics"]["init"] = "potential".into();
    assert!(has(&found(&v), "CFD-PHYS-005", "/physics/init"));
    let mut v = still();
    v["physics"] = json!({"init": "potential"});
    assert!(has(&found(&v), "CFD-PHYS-005", "/physics/init"));
    // An inlet drives it, and so does the lid of a closed cavity.
    let mut v = channel();
    v["physics"] = json!({"init": "potential"});
    assert!(found(&v).is_empty(), "{:?}", found(&v));
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["physics"] = json!({"init": "potential"});
    assert!(found(&v).is_empty(), "{:?}", found(&v));
}

#[test]
fn simple_laminar_conflicts_with_other_physics() {
    let cases = [
        ("transient", transient()),
        ("turbulent", turbulent()),
        ("buoyant", heated()),
        ("potential", {
            let mut v = channel();
            v["physics"] = json!({"init": "potential"});
            v
        }),
    ];
    for (name, mut v) in cases {
        v["solver"]["solver"] = "SimpleLaminar".into();
        assert!(has(&found(&v), "CFD-PHYS-006", "/solver/solver"), "{name}");
        v["solver"]["solver"] = "FromPhysics".into();
        assert!(lacks(&found(&v), "CFD-PHYS-006"), "{name}");
    }
    // A study that states no physics is the steady laminar run the kind stands for.
    assert!(lacks(&found(&value(CAVITY)), "CFD-PHYS-006"));
}

#[test]
fn the_profile_belongs_on_an_x_inlet() {
    let mut v = channel();
    v["boundary_conditions"]["inlet"]["profile"] = "parabolic".into();
    assert!(lacks(&found(&v), "CFD-BC-007"));
    let mut v = channel();
    v["boundary_conditions"]["floor"]["profile"] = "parabolic".into();
    assert!(has(
        &found(&v),
        "CFD-BC-007",
        "/boundary_conditions/floor/profile"
    ));
    // An inlet on the bottom face, and an inlet that covers two faces.
    let mut v = channel();
    v["boundary_conditions"]["inlet"]["faces"] = json!(["YMin"]);
    v["boundary_conditions"]["inlet"]["profile"] = "parabolic".into();
    assert!(has(
        &found(&v),
        "CFD-BC-007",
        "/boundary_conditions/inlet/profile"
    ));
    let mut v = channel();
    v["boundary_conditions"]["inlet"]["faces"] = json!(["XMin", "YMin"]);
    v["boundary_conditions"]["inlet"]["profile"] = "parabolic".into();
    assert!(has(
        &found(&v),
        "CFD-BC-007",
        "/boundary_conditions/inlet/profile"
    ));
}

#[test]
fn force_probe_and_heat_flux_monitors_are_checked() {
    let forces = "/monitors/forces/cyl";
    let mut v = cylinder();
    v["monitors"]["forces"] = json!({"cyl": force("cylinder")});
    assert!(lacks(&found(&v), "CFD-MON-007"));
    for (boundary, name) in [("nowhere", "unknown"), ("inlet", "an inlet")] {
        let mut v = cylinder();
        v["monitors"]["forces"] = json!({"cyl": force(boundary)});
        assert!(
            has(&found(&v), "CFD-MON-007", &format!("{forces}/boundary")),
            "{name}"
        );
    }
    for (member, bad) in [
        ("reference_velocity", json!({"value": 0, "unit": "m/s"})),
        ("reference_length", m(-1.0)),
    ] {
        let mut v = cylinder();
        v["monitors"]["forces"] = json!({"cyl": force("cylinder")});
        v["monitors"]["forces"]["cyl"][member] = bad;
        assert!(
            has(&found(&v), "CFD-MON-007", &format!("{forces}/{member}")),
            "{member}"
        );
    }
    let mut v = cylinder();
    v["monitors"]["forces"] = json!({
        "a": force("cylinder"), "b": force("cylinder"), "c": force("cylinder"),
        "d": force("cylinder"), "e": force("cylinder")
    });
    assert!(has(&found(&v), "CFD-MON-007", "/monitors/forces"));

    // A probe on a boundary: unknown, off the circle, on it, and on a box face.
    let probe = |x: f64, y: f64, boundary: &str| json!({"x": m(x), "y": m(y), "field": "pressure", "boundary": boundary});
    let mut v = cylinder();
    v["monitors"]["probes"] = json!({"p": probe(0.25, 0.2, "nowhere")});
    assert!(has(
        &found(&v),
        "CFD-MON-008",
        "/monitors/probes/p/boundary"
    ));
    v["monitors"]["probes"] = json!({"p": probe(0.3, 0.2, "cylinder")});
    assert!(has(
        &found(&v),
        "CFD-MON-008",
        "/monitors/probes/p/boundary"
    ));
    v["monitors"]["probes"] = json!({"p": probe(0.25, 0.2, "cylinder")});
    assert!(lacks(&found(&v), "CFD-MON-008"));
    v["monitors"]["probes"] = json!({"p": probe(1.0, 0.0, "floor")});
    assert!(lacks(&found(&v), "CFD-MON-008"));
    v["monitors"]["probes"] = json!({"p": probe(1.0, 0.01, "floor")});
    assert!(has(
        &found(&v),
        "CFD-MON-008",
        "/monitors/probes/p/boundary"
    ));

    // A heat flux needs a thermal condition on its boundary.
    let mut v = value(CAVITY);
    v["monitors"]["surfaces"] = json!({"s": {"boundary": "lid", "quantity": "heat_flux"}});
    assert!(has(
        &found(&v),
        "CFD-MON-009",
        "/monitors/surfaces/s/boundary"
    ));
    let mut v = heated();
    v["monitors"]["surfaces"] = json!({"s": {"boundary": "hot", "quantity": "heat_flux"}});
    assert!(lacks(&found(&v), "CFD-MON-009"));
}

#[test]
fn the_shape_totals_are_checked() {
    let list = found(&step(0.254));
    assert!(list.is_empty(), "{list:?}");
    let mut v = step(0.254);
    v["mesh"]["cells_x"] = 100.into();
    let list = found(&v);
    let d = list
        .iter()
        .find(|d| d.code == "CFD-SHAPE-004" && d.pointer == "/mesh/cells_x")
        .expect("the x total is refused");
    let said = format!("{} {}", d.message, d.hint.as_deref().unwrap_or_default());
    assert!(said.contains("220") && said.contains("114"), "{said}");
    let mut v = step(0.254);
    v["mesh"]["cells_y"] = 20.into();
    assert!(has(&found(&v), "CFD-SHAPE-004", "/mesh/cells_y"));
    // Without a start-up section the x line has 218 cells.
    let mut v = step(0.0);
    v["mesh"]["cells_x"] = 220.into();
    let list = found(&v);
    let d = list
        .iter()
        .find(|d| d.code == "CFD-SHAPE-004")
        .expect("the x total is refused");
    assert!(d.hint.as_deref().unwrap_or_default().contains("218"));
    // The cylinder channel has its own totals, and the level refines them.
    let mut v = cylinder();
    v["mesh"]["cells_x"] = 100.into();
    assert!(has(&found(&v), "CFD-SHAPE-004", "/mesh/cells_x"));
    let mut v = cylinder();
    v["mesh"]["level"] = 1.into();
    assert!(lacks(&found(&v), "CFD-SHAPE-004"));
    // A box takes any counts.
    assert!(lacks(&found(&value(CAVITY)), "CFD-SHAPE-004"));
}
