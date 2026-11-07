use std::collections::BTreeMap;

use cfdcraft_model::{
    ResolvedStudy, Study, inlet_turbulence, resolve,
    roundtrip::{
        CaseReader, Expected, Fact, FactValue, ReadError, SourceFile, check_roundtrip,
        check_roundtrip_with, expected_facts, shape_delegated_pointers, uncovered_leaves,
        uncovered_leaves_with,
    },
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

fn leaves(study: &ResolvedStudy) -> Value {
    serde_json::to_value(study).unwrap()
}

/// A reader that reports exactly the facts handed to it.
struct Canned(BTreeMap<String, Fact>);

impl CaseReader for Canned {
    fn read(&self, _: &[SourceFile]) -> Result<BTreeMap<String, Fact>, ReadError> {
        Ok(self.0.clone())
    }
}

fn fact(value: FactValue) -> Fact {
    Fact {
        value,
        value_spans: vec![],
        entry_span: None,
    }
}

/// The facts a perfect case holds: everything expected plus the extras.
fn perfect(study: &ResolvedStudy, extra: &[Expected]) -> BTreeMap<String, Fact> {
    expected_facts(study)
        .into_iter()
        .chain(extra.iter().cloned())
        .map(|e| (e.key, fact(e.value)))
        .collect()
}

fn extra(key: &str, text: &str) -> Expected {
    Expected {
        key: key.into(),
        value: FactValue::Text(text.into()),
        source_pointers: vec![],
        setting: key.into(),
    }
}

fn find<'a>(facts: &'a [Expected], key: &str) -> &'a Expected {
    facts
        .iter()
        .find(|e| e.key == key)
        .unwrap_or_else(|| panic!("no expected fact {key}"))
}

fn has(facts: &[Expected], key: &str) -> bool {
    facts.iter().any(|e| e.key == key)
}

fn text(word: &str) -> FactValue {
    FactValue::Text(word.into())
}

/// The cavity as a transient laminar run with the given time step, at the given level.
fn transient_cavity(delta_t: f64, level: u32) -> Value {
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["solver"]["end_time"] = json!({"value": 10, "unit": "s"});
    v["solver"]["write_interval"] = json!({"value": 1, "unit": "s"});
    v["mesh"]["level"] = level.into();
    v["physics"] =
        json!({"time": {"mode": "transient", "delta_t": {"value": delta_t, "unit": "s"}}});
    v
}

/// The cavity with the given physics, solved from the physics.
fn cavity_with_physics(physics: Value) -> Value {
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["physics"] = physics;
    v
}

fn buoyancy() -> Value {
    json!({
        "gravity": 9.81,
        "beta": 0.003,
        "reference_temperature": kelvin(300.0),
        "prandtl": 0.71,
        "turbulent_prandtl": 0.85
    })
}

/// A closed cavity heated from the left and cooled from the right, with buoyancy on.
fn heated(physics: Value) -> Value {
    let mut v = cavity_with_physics(physics);
    v["boundary_conditions"] = json!({
        "hot": {"faces": ["XMin"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": kelvin(300.5)}},
        "cold": {"faces": ["XMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": kelvin(299.5)}},
        "adiabatic": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "adiabatic"}}
    });
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

fn sorted(items: &[&str], probes: u32) -> Vec<String> {
    let mut out: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
    for n in 1..=probes {
        for member in ["field", "x", "y"] {
            out.push(format!("/monitors/probe/{n}/{member}"));
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------------------------
// The application, the turbulence model, the initialisation and the time step.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_old_studies_expect_the_same_facts() {
    let common = [
        "/domain/length_x",
        "/domain/length_y",
        "/domain/length_z",
        "/domain/open",
        "/domain/origin_x",
        "/domain/origin_y",
        "/domain/origin_z",
        "/fluid/kinematic_viscosity",
        "/mesh/cells_x",
        "/mesh/cells_y",
        "/mesh/cells_z",
        "/mesh/grading",
        "/monitors/centreline_u_sample_count/horizontal",
        "/monitors/centreline_u_sample_count/vertical",
        "/solver/application",
        "/solver/end_time",
        "/solver/init",
        "/solver/turbulence",
        "/solver/write_interval",
        "/boundary_conditions/by_face/ZMax/kind",
        "/boundary_conditions/by_face/ZMin/kind",
    ];
    let cavity_faces = [
        "/boundary_conditions/by_face/XMax/kind",
        "/boundary_conditions/by_face/XMin/kind",
        "/boundary_conditions/by_face/YMax/kind",
        "/boundary_conditions/by_face/YMax/velocity",
        "/boundary_conditions/by_face/YMin/kind",
    ];
    let channel_faces = [
        "/boundary_conditions/by_face/XMax/kind",
        "/boundary_conditions/by_face/XMin/kind",
        "/boundary_conditions/by_face/XMin/velocity",
        "/boundary_conditions/by_face/YMax/kind",
        "/boundary_conditions/by_face/YMin/kind",
    ];
    for (json, faces) in [(CAVITY, cavity_faces), (CHANNEL, channel_faces)] {
        let study = resolved(&value(json));
        let facts = expected_facts(&study);
        let keys: Vec<&str> = facts.iter().map(|e| e.key.as_str()).collect();
        let all: Vec<&str> = common.iter().chain(faces.iter()).copied().collect();
        // Neither study lists a monitor, so both are watched at the five default probes.
        assert_eq!(keys, sorted(&all, 5));
        assert_eq!(
            find(&facts, "/solver/application").value,
            text("simpleFoam")
        );
        assert_eq!(find(&facts, "/solver/turbulence").value, text("laminar"));
        assert_eq!(find(&facts, "/solver/init").value, text("uniform"));
        assert_eq!(find(&facts, "/mesh/grading").value, text("1 1 1"));
        assert!(!has(&facts, "/solver/time/delta_t"));
        assert!(!facts.iter().any(|e| e.key.starts_with("/physics/")));
    }
}

#[test]
fn a_transient_study_expects_its_application_and_time_step() {
    for (level, step) in [(0, 0.001), (1, 0.001 / 2.0)] {
        let study = resolved(&transient_cavity(0.001, level));
        let facts = expected_facts(&study);
        assert_eq!(
            find(&facts, "/solver/application").value,
            text("pimpleFoam")
        );
        assert_eq!(
            find(&facts, "/solver/time/delta_t").value,
            FactValue::Number(study.effective_delta_t().unwrap())
        );
        assert_eq!(
            find(&facts, "/solver/time/delta_t").value,
            FactValue::Number(step)
        );
        assert_eq!(
            find(&facts, "/solver/time/write_control").value,
            text("runTime")
        );
        assert_eq!(find(&facts, "/solver/init").value, text("uniform"));
        assert_eq!(find(&facts, "/solver/time/delta_t").setting, "time step");
        let missed = uncovered_leaves(&leaves(&study), &facts);
        assert_eq!(missed, Vec::<String>::new());
    }
    let steady = expected_facts(&resolved(&value(CAVITY)));
    assert!(!steady.iter().any(|e| e.key.starts_with("/solver/time/")));
}

#[test]
fn the_four_solvers_and_the_four_models_have_their_words() {
    let transient = json!({"time": {"mode": "transient", "delta_t": {"value": 0.01, "unit": "s"}}});
    let mut buoyant_transient = json!({"buoyancy": buoyancy()});
    buoyant_transient["time"] = transient["time"].clone();
    for (v, word) in [
        (value(CAVITY), "simpleFoam"),
        (cavity_with_physics(transient), "pimpleFoam"),
        (
            heated(json!({"buoyancy": buoyancy()})),
            "buoyantBoussinesqSimpleFoam",
        ),
        (heated(buoyant_transient), "buoyantBoussinesqPimpleFoam"),
    ] {
        let facts = expected_facts(&resolved(&v));
        assert_eq!(find(&facts, "/solver/application").value, text(word));
    }
    for (model, word) in [
        ("laminar", "laminar"),
        ("k_omega_sst", "kOmegaSST"),
        ("realizable_ke", "realizableKE"),
        ("rng_k_epsilon", "RNGkEpsilon"),
    ] {
        let physics =
            json!({"turbulence": {"model": model, "intensity": 0.05, "viscosity_ratio": 10}});
        let study = resolved(&cavity_with_physics(physics));
        let facts = expected_facts(&study);
        assert_eq!(
            find(&facts, "/solver/turbulence").value,
            text(word),
            "{model}"
        );
        let missed = uncovered_leaves(&leaves(&study), &facts);
        assert_eq!(missed, Vec::<String>::new(), "{model}");
    }
    let potential = resolved(&cavity_with_physics(json!({"init": "potential"})));
    let facts = expected_facts(&potential);
    assert_eq!(find(&facts, "/solver/init").value, text("potential"));
}

#[test]
fn check_roundtrip_with_merges_extra_facts() {
    let study = resolved(&value(CAVITY));
    let patch = extra("/monitors/force/1/patch", "lid");
    let extras = [patch.clone()];
    let files: [SourceFile; 0] = [];

    // A case that holds the extra passes.
    let whole = Canned(perfect(&study, &extras));
    assert_eq!(
        check_roundtrip_with(&whole, &files, &study, &extras),
        vec![]
    );
    // Without the extra in the list, the same case holds a fact nobody accounts for.
    let found = check_roundtrip(&whole, &files, &study);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, "CFD-RT-001");
    assert!(found[0].message.contains("does not account for"));

    // A missing extra and a different extra are reported like any other fact.
    let mut facts = perfect(&study, &extras);
    facts.remove("/monitors/force/1/patch");
    let found = check_roundtrip_with(&Canned(facts), &files, &study, &extras);
    assert_eq!(found.len(), 1);
    assert!(found[0].message.contains("was not found"));
    let mut facts = perfect(&study, &extras);
    facts.insert(patch.key.clone(), fact(text("walls")));
    let found = check_roundtrip_with(&Canned(facts), &files, &study, &extras);
    assert_eq!(found.len(), 1);
    assert!(found[0].message.contains("is written as walls"));

    // An extra that repeats a key of the study is a fault of the caller, named by its key.
    let clash = [extra("/solver/application", "simpleFoam")];
    let found = check_roundtrip_with(&Canned(perfect(&study, &[])), &files, &study, &clash);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, "CFD-RT-001");
    assert!(found[0].message.contains("/solver/application"));

    // Without extras the two entry points agree, on a clean case and on a broken one.
    let mut broken = perfect(&study, &[]);
    broken.get_mut("/mesh/cells_x").unwrap().value = FactValue::Count(1);
    let reader = Canned(broken);
    assert_eq!(
        check_roundtrip(&reader, &files, &study),
        check_roundtrip_with(&reader, &files, &study, &[])
    );
    assert_eq!(check_roundtrip(&reader, &files, &study).len(), 1);
}

#[test]
fn the_leaf_coverage_counts_the_extras() {
    let cavity = resolved(&value(CAVITY));
    let facts = expected_facts(&cavity);
    assert_eq!(
        uncovered_leaves_with(&leaves(&cavity), &facts, &[]),
        Vec::<String>::new()
    );

    let step = resolved(&step());
    let facts = expected_facts(&step);
    let missed = uncovered_leaves_with(&leaves(&step), &facts, &[]);
    assert!(
        missed.iter().any(|p| p.starts_with("/domain/shape/")),
        "{missed:?}"
    );
    assert!(!missed.iter().any(|p| p == "/domain/open"));
    // `uncovered_leaves` is the same check without extras.
    assert_eq!(missed, uncovered_leaves(&leaves(&step), &facts));

    let mut covering = extra("/mesh/vertices", "x");
    covering.source_pointers = vec!["/domain/shape".into()];
    let missed = uncovered_leaves_with(&leaves(&step), &facts, &[covering]);
    assert!(!missed.iter().any(|p| p.starts_with("/domain/shape")));
}

/// An open box that sets every member the physics and the monitors offer: a parabolic inlet, a
/// heated floor, an insulated ceiling, buoyancy, a turbulence model, a transient run started from
/// potential flow at mesh level 1, a force monitor, a probe on the floor and a heat-flux surface.
fn full() -> Value {
    let mut v = value(CHANNEL);
    v["solver"]["solver"] = "FromPhysics".into();
    v["solver"]["end_time"] = json!({"value": 10, "unit": "s"});
    v["solver"]["write_interval"] = json!({"value": 1, "unit": "s"});
    v["mesh"]["level"] = 1.into();
    v["boundary_conditions"] = json!({
        "inlet": {"faces": ["XMin"], "kind": "VelocityInlet",
            "velocity": {"value": 0.1, "unit": "m/s"}, "profile": "parabolic"},
        "outlet": {"faces": ["XMax"], "kind": "PressureOutlet"},
        "floor": {"faces": ["YMin"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": kelvin(301.0)}},
        "ceiling": {"faces": ["YMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "adiabatic"}}
    });
    v["physics"] = json!({
        "time": {"mode": "transient", "delta_t": {"value": 0.01, "unit": "s"}},
        "turbulence": {"model": "k_omega_sst", "intensity": 0.05, "viscosity_ratio": 10},
        "buoyancy": buoyancy(),
        "init": "potential"
    });
    v["monitors"]["forces"] = json!({"drag": {
        "boundary": "floor",
        "reference_velocity": {"value": 0.1, "unit": "m/s"},
        "reference_length": m(0.1)
    }});
    v["monitors"]["probes"] = json!({"wall_p": {
        "x": m(0.5), "y": m(0.0), "field": "pressure", "boundary": "floor"
    }});
    v["monitors"]["surfaces"] =
        json!({"floor_flux": {"boundary": "floor", "quantity": "heat_flux"}});
    v
}

// ---------------------------------------------------------------------------------------------
// Buoyancy, inflow values, thermal and profile conditions, and the new monitors.
// ---------------------------------------------------------------------------------------------

#[test]
fn buoyancy_values_are_expected() {
    let study = resolved(&full());
    let facts = expected_facts(&study);
    for (member, wanted) in [
        ("gravity", 9.81),
        ("beta", 0.003),
        ("reference_temperature", 300.0),
        ("prandtl", 0.71),
        ("turbulent_prandtl", 0.85),
    ] {
        let key = format!("/physics/buoyancy/{member}");
        assert_eq!(find(&facts, &key).value, FactValue::Number(wanted), "{key}");
        assert_eq!(find(&facts, &key).source_pointers, [key.as_str()]);
    }
    assert_eq!(
        find(&facts, "/solver/application").value,
        text("buoyantBoussinesqPimpleFoam")
    );
    let plain = expected_facts(&resolved(&value(CHANNEL)));
    assert!(!plain.iter().any(|e| e.key.starts_with("/physics/")));
}

#[test]
fn inlet_turbulence_values_are_expected() {
    // The inlet speed is 0.1 m/s and the viscosity 0.001 m2/s.
    let wanted = inlet_turbulence(0.05, 10.0, 0.1, 0.001).unwrap();
    let study = resolved(&full());
    let facts = expected_facts(&study);
    assert_eq!(
        find(&facts, "/physics/turbulence/k").value,
        FactValue::Number(wanted.k)
    );
    assert_eq!(
        find(&facts, "/physics/turbulence/omega").value,
        FactValue::Number(wanted.omega)
    );
    assert!(!has(&facts, "/physics/turbulence/epsilon"));
    assert_eq!(
        find(&facts, "/physics/turbulence/k").setting,
        "inlet turbulent kinetic energy"
    );
    // The speed came from the inlet, so that leaf is among the ones the values cover.
    assert!(
        find(&facts, "/physics/turbulence/k")
            .source_pointers
            .contains(&"/boundary_conditions/inlet/velocity".to_owned())
    );

    for model in ["realizable_ke", "rng_k_epsilon"] {
        let mut v = full();
        v["physics"]["turbulence"]["model"] = model.into();
        let facts = expected_facts(&resolved(&v));
        assert_eq!(
            find(&facts, "/physics/turbulence/epsilon").value,
            FactValue::Number(wanted.epsilon),
            "{model}"
        );
        assert!(!has(&facts, "/physics/turbulence/omega"), "{model}");
        assert!(has(&facts, "/physics/turbulence/k"), "{model}");
    }

    // An explicit reference velocity is the speed used.
    let mut v = full();
    v["physics"]["turbulence"]["reference_velocity"] = json!({"value": 2, "unit": "m/s"});
    let study = resolved(&v);
    let facts = expected_facts(&study);
    let wanted = inlet_turbulence(0.05, 10.0, 2.0, 0.001).unwrap();
    assert_eq!(
        find(&facts, "/physics/turbulence/k").value,
        FactValue::Number(wanted.k)
    );
    assert!(
        find(&facts, "/physics/turbulence/k")
            .source_pointers
            .contains(&"/physics/turbulence/reference_velocity".to_owned())
    );
    let missed = uncovered_leaves(&leaves(&study), &facts);
    assert_eq!(missed, Vec::<String>::new());

    // A laminar study has no inflow values.
    let mut v = full();
    v["physics"]["turbulence"] = json!({"model": "laminar"});
    let facts = expected_facts(&resolved(&v));
    assert!(
        !facts
            .iter()
            .any(|e| e.key.starts_with("/physics/turbulence/"))
    );
}

#[test]
fn box_faces_expect_their_thermal_and_profile_conditions() {
    let facts = expected_facts(&resolved(&full()));
    let at = |face: &str, member: &str| format!("/boundary_conditions/by_face/{face}/{member}");
    assert_eq!(find(&facts, &at("YMin", "thermal")).value, text("fixed"));
    assert_eq!(
        find(&facts, &at("YMin", "temperature")).value,
        FactValue::Number(301.0)
    );
    assert_eq!(
        find(&facts, &at("YMax", "thermal")).value,
        text("adiabatic")
    );
    assert!(!has(&facts, &at("YMax", "temperature")));
    assert_eq!(
        find(&facts, &at("XMin", "profile")).value,
        text("parabolic")
    );
    // Faces without the member expect none of it.
    for key in [
        at("XMin", "thermal"),
        at("XMax", "thermal"),
        at("XMax", "profile"),
        at("YMin", "profile"),
        at("XMin", "temperature"),
    ] {
        assert!(!has(&facts, &key), "{key}");
    }
    assert_eq!(
        find(&facts, &at("YMin", "thermal")).source_pointers,
        ["/boundary_conditions/floor/thermal"]
    );
    assert_eq!(
        find(&facts, &at("XMin", "profile")).source_pointers,
        ["/boundary_conditions/inlet/profile"]
    );

    let mut uniform = value(CHANNEL);
    uniform["boundary_conditions"]["inlet"]["profile"] = "uniform".into();
    let resolved_uniform = resolved(&uniform);
    let facts = expected_facts(&resolved_uniform);
    // An explicit uniform profile writes the same bytes as none, so it is not an expected fact;
    // its leaf is still covered (by the kind fact), so nothing about it goes unaccounted for.
    assert!(!has(&facts, &at("XMin", "profile")));
    assert!(
        find(&facts, &at("XMin", "kind"))
            .source_pointers
            .iter()
            .any(|p| p == "/boundary_conditions/inlet/profile")
    );
    assert!(
        !uncovered_leaves(&leaves(&resolved_uniform), &facts)
            .iter()
            .any(|p| p.ends_with("/profile"))
    );

    // In a buoyant study a wall that states no thermal condition is adiabatic by the writer's
    // default; an inlet or an outlet does not claim it.
    let mut bare = full();
    bare["boundary_conditions"]["ceiling"]
        .as_object_mut()
        .unwrap()
        .remove("thermal");
    let facts = expected_facts(&resolved(&bare));
    assert_eq!(
        find(&facts, &at("YMax", "thermal")).value,
        text("adiabatic")
    );
    assert!(!has(&facts, &at("XMin", "thermal")));
    assert!(!has(&facts, &at("XMax", "thermal")));

    // Studies that state none of these have none of these keys.
    for json in [CAVITY, CHANNEL] {
        let facts = expected_facts(&resolved(&value(json)));
        assert!(!facts.iter().any(|e| {
            ["/thermal", "/temperature", "/profile"]
                .iter()
                .any(|end| e.key.ends_with(end))
        }));
    }
}

#[test]
fn force_and_boundary_probe_and_heat_flux_facts() {
    let facts = expected_facts(&resolved(&full()));
    assert_eq!(
        find(&facts, "/monitors/force/1/reference_velocity").value,
        FactValue::Number(0.1)
    );
    assert_eq!(
        find(&facts, "/monitors/force/1/reference_length").value,
        FactValue::Number(0.1)
    );
    assert_eq!(find(&facts, "/monitors/force/1/lift_axis").value, text("y"));
    assert_eq!(
        find(&facts, "/monitors/force/1/lift_axis").setting,
        "force 1 lift axis"
    );
    assert_eq!(
        find(&facts, "/monitors/force/1/reference_length").source_pointers,
        ["/monitors/forces"]
    );
    // The patch a force sits on belongs to the backend.
    assert!(!has(&facts, "/monitors/force/1/patch"));
    assert!(!has(&facts, "/monitors/probe/1/patch"));

    assert_eq!(
        find(&facts, "/monitors/probe/1/x").value,
        FactValue::Number(0.5)
    );
    assert_eq!(
        find(&facts, "/monitors/probe/1/y").value,
        FactValue::Number(0.0)
    );
    assert_eq!(
        find(&facts, "/monitors/probe/1/field").value,
        text("pressure")
    );
    assert_eq!(
        find(&facts, "/monitors/probe/1/on_boundary").value,
        FactValue::Flag(true)
    );
    assert_eq!(
        find(&facts, "/monitors/surface/1/quantity").value,
        text("heat_flux")
    );

    // A probe in the flow has no boundary flag.
    let mut v = value(CAVITY);
    v["monitors"]["probes"] = json!({"mid": {"x": m(0.05), "y": m(0.05), "field": "velocity"}});
    let facts = expected_facts(&resolved(&v));
    assert!(!facts.iter().any(|e| e.key.ends_with("/on_boundary")));
    assert!(!facts.iter().any(|e| e.key.starts_with("/monitors/force/")));
}

#[test]
fn every_resolved_leaf_is_covered() {
    let study = resolved(&full());
    let facts = expected_facts(&study);
    let delegated = [
        extra("/monitors/force/1/patch", "floor"),
        extra("/monitors/probe/1/patch", "floor"),
    ];
    assert_eq!(shape_delegated_pointers(&study), Vec::<String>::new());
    assert_eq!(
        uncovered_leaves_with(&leaves(&study), &facts, &delegated),
        Vec::<String>::new()
    );
    assert_eq!(
        uncovered_leaves(&leaves(&study), &facts),
        Vec::<String>::new()
    );
    // The level is covered too: the case holds the refined counts and the halved step.
    assert_eq!(leaves(&study)["mesh"]["level"], json!(1));
    // And a case that holds all of it passes the check.
    let files: [SourceFile; 0] = [];
    let reader = Canned(perfect(&study, &delegated));
    assert_eq!(
        check_roundtrip_with(&reader, &files, &study, &delegated),
        vec![]
    );
}

#[test]
fn a_changed_value_is_reported_by_name() {
    let study = resolved(&full());
    let mut facts = perfect(&study, &[]);
    facts.get_mut("/physics/buoyancy/beta").unwrap().value = FactValue::Number(0.004);
    let found = check_roundtrip_with(&Canned(facts), &[], &study, &[]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, "CFD-RT-001");
    assert_eq!(found[0].setting, "buoyancy expansion coefficient");
    assert_eq!(found[0].pointer, "/physics/buoyancy/beta");
    assert!(found[0].message.contains("buoyancy expansion coefficient"));

    let mut facts = perfect(&study, &[]);
    facts.remove("/physics/turbulence/omega");
    let found = check_roundtrip_with(&Canned(facts), &[], &study, &[]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].setting, "inlet specific dissipation rate");
}

/// A cylinder channel with its own wall.
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

// ---------------------------------------------------------------------------------------------
// A shaped study delegates its geometry and per-patch conditions.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_shaped_study_expects_no_box_geometry() {
    for json in [step(), cylinder()] {
        let facts = expected_facts(&resolved(&json));
        for key in [
            "/mesh/cells_x",
            "/mesh/cells_y",
            "/mesh/cells_z",
            "/mesh/grading",
            "/domain/length_x",
            "/domain/length_y",
            "/domain/length_z",
            "/domain/origin_x",
            "/domain/origin_y",
            "/domain/origin_z",
        ] {
            assert!(!has(&facts, key), "{key}");
        }
        assert!(!facts.iter().any(|e| e.key.contains("by_face")));
        for key in [
            "/domain/open",
            "/fluid/kinematic_viscosity",
            "/solver/end_time",
            "/solver/application",
            "/solver/init",
            "/monitors/probe/1/x",
            "/monitors/centreline_u_sample_count/vertical",
        ] {
            assert!(has(&facts, key), "{key}");
        }
    }
}

#[test]
fn the_delegated_leaves_are_listed() {
    let study = resolved(&step());
    let listed = shape_delegated_pointers(&study);
    for pointer in [
        "/domain/length_x",
        "/domain/length_y",
        "/domain/length_z",
        "/domain/shape",
        "/mesh/cells_x",
        "/mesh/cells_y",
        "/mesh/cells_z",
        "/boundary_conditions/inlet/kind",
        "/boundary_conditions/inlet/faces",
        "/boundary_conditions/inlet/velocity",
        "/boundary_conditions/outlet/kind",
        "/boundary_conditions/floor/faces",
        "/boundary_conditions/centreline/kind",
    ] {
        assert!(listed.iter().any(|p| p == pointer), "{pointer}");
    }
    assert!(!listed.iter().any(|p| p == "/domain/open"));
    assert!(!listed.iter().any(|p| p == "/mesh/level"));
    assert!(
        !listed
            .iter()
            .any(|p| p.ends_with("/profile") || p.ends_with("/thermal"))
    );

    // A level, a profile and a thermal condition each add their leaf.
    let mut v = cylinder();
    v["mesh"]["level"] = 1.into();
    v["boundary_conditions"]["inlet"]["profile"] = "parabolic".into();
    let listed = shape_delegated_pointers(&resolved(&v));
    assert!(listed.iter().any(|p| p == "/mesh/level"));
    assert!(
        listed
            .iter()
            .any(|p| p == "/boundary_conditions/inlet/profile")
    );
    assert!(
        listed
            .iter()
            .any(|p| p == "/boundary_conditions/cylinder/faces")
    );

    // A box delegates nothing.
    assert_eq!(
        shape_delegated_pointers(&resolved(&value(CAVITY))),
        Vec::<String>::new()
    );
    assert_eq!(
        shape_delegated_pointers(&resolved(&full())),
        Vec::<String>::new()
    );
}

#[test]
fn the_uncovered_check_demands_the_extras() {
    for json in [step(), cylinder()] {
        let study = resolved(&json);
        let facts = expected_facts(&study);
        let json = leaves(&study);
        let delegated = shape_delegated_pointers(&study);

        let missed = uncovered_leaves_with(&json, &facts, &[]);
        for pointer in &delegated {
            assert!(
                missed.iter().any(|leaf| leaf == pointer
                    || leaf
                        .strip_prefix(pointer.as_str())
                        .is_some_and(|r| r.starts_with('/'))),
                "{pointer} is not reported: {missed:?}"
            );
        }

        let mut covering = extra("/mesh/vertices", "0");
        covering.source_pointers = delegated.clone();
        assert_eq!(
            uncovered_leaves_with(&json, &facts, std::slice::from_ref(&covering)),
            Vec::<String>::new()
        );

        // Leaving one pointer out lists exactly that leaf.
        covering.source_pointers = delegated
            .into_iter()
            .filter(|p| p != "/mesh/cells_y")
            .collect();
        assert_eq!(
            uncovered_leaves_with(&json, &facts, &[covering]),
            ["/mesh/cells_y"]
        );
    }
}

#[test]
fn a_box_with_wall_grading_expects_its_grading_text() {
    for level in [0u32, 1] {
        let mut v = value(CAVITY);
        v["mesh"]["cells_x"] = 40.into();
        v["mesh"]["cells_y"] = 40.into();
        v["mesh"]["level"] = level.into();
        v["mesh"]["grading"] = json!({"x": 1.05, "y": 1.05});
        let study = resolved(&v);
        let facts = expected_facts(&study);

        // The effective counts, and the growth of the last cell over the first in each half.
        let cells = 40u64 << level;
        assert_eq!(find(&facts, "/mesh/cells_x").value, FactValue::Count(cells));
        assert_eq!(find(&facts, "/mesh/cells_y").value, FactValue::Count(cells));
        let ratio = 1.05_f64.powf(1.0 / f64::from(1u32 << level));
        let growth = ratio.powf((cells / 2 - 1) as f64);
        let shrink = 1.0 / growth;
        let axis = format!("((0.5 0.5 {growth}) (0.5 0.5 {shrink}))");
        assert_eq!(
            find(&facts, "/mesh/grading").value,
            FactValue::Text(format!("{axis} {axis} 1")),
            "level {level}"
        );
        assert_eq!(find(&facts, "/mesh/grading").setting, "mesh grading");
        let missed = uncovered_leaves(&leaves(&study), &facts);
        assert_eq!(missed, Vec::<String>::new(), "level {level}");
    }

    // Only x graded: the other axes stay even.
    let mut v = value(CAVITY);
    v["mesh"]["cells_x"] = 40.into();
    v["mesh"]["cells_y"] = 40.into();
    v["mesh"]["grading"] = json!({"x": 1.05});
    let facts = expected_facts(&resolved(&v));
    let FactValue::Text(text) = &find(&facts, "/mesh/grading").value else {
        panic!("the grading is text");
    };
    assert!(text.starts_with("((0.5 0.5 "), "{text}");
    assert!(text.ends_with(") 1 1"), "{text}");
}
