//! The reader recovers the physics of a study from the written bytes: the program, the turbulence
//! model, the time step, the start-up field, the buoyancy values, the inflow values, the thermal
//! and profile conditions, and the force, boundary-probe and heat-flux monitors. Every study goes
//! through `resolve` and `compile`, and the facts the reader returns are compared with the facts
//! the model expects.

mod field_studies;
mod plan_studies;

use cfdcraft_backend_esi::{
    Dialect, compile,
    reader::{EsiReader, source_files},
    shaped_expected_facts,
};
use cfdcraft_model::{
    ResolvedStudy, inlet_turbulence,
    roundtrip::{
        CaseReader, Fact, FactValue, SourceFile, check_roundtrip, check_roundtrip_with,
        expected_facts,
    },
};
use plan_studies::{TRANSIENT_ONLY, cavity, heated};
use std::collections::BTreeMap;

const CHANNEL_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn read(study: &ResolvedStudy) -> (Vec<SourceFile>, BTreeMap<String, Fact>) {
    let tree = compile(study, Dialect::EsiV2512).unwrap();
    let files = source_files(&tree);
    let facts = EsiReader.read(&files).unwrap();
    (files, facts)
}

fn value(facts: &BTreeMap<String, Fact>, key: &str) -> FactValue {
    facts
        .get(key)
        .unwrap_or_else(|| panic!("the reader found no fact {key}"))
        .value
        .clone()
}

fn word(text: &str) -> FactValue {
    FactValue::Text(text.into())
}

fn text<'a>(files: &'a [SourceFile], path: &str) -> &'a str {
    std::str::from_utf8(&files.iter().find(|f| f.path == path).unwrap().bytes).unwrap()
}

/// The facts the model expects and the reader finds are the same keys, with the extras of the
/// backend added to the model's.
fn same_keys(study: &ResolvedStudy, facts: &BTreeMap<String, Fact>) {
    let mut wanted: Vec<String> = expected_facts(study)
        .into_iter()
        .chain(shaped_expected_facts(study))
        .map(|e| e.key)
        .collect();
    wanted.sort();
    let got: Vec<String> = facts.keys().cloned().collect();
    assert_eq!(got, wanted);
}

#[test]
fn a_transient_box_study_passes_the_round_trip() {
    let study = cavity(TRANSIENT_ONLY, "8", "0.5", 1);
    let (files, facts) = read(&study);
    assert_eq!(check_roundtrip(&EsiReader, &files, &study), vec![]);
    same_keys(&study, &facts);
    assert_eq!(value(&facts, "/solver/application"), word("pimpleFoam"));
    assert_eq!(value(&facts, "/solver/time/write_control"), word("runTime"));
    assert_eq!(value(&facts, "/solver/init"), word("uniform"));
    // The step is the file's own `deltaT`, and the level halves it.
    let step = study.effective_delta_t().unwrap();
    assert!((step - 0.0005).abs() < 1e-15, "{step}");
    let written: f64 = text(&files, "system/controlDict")
        .lines()
        .find_map(|l| l.trim().strip_prefix("deltaT "))
        .unwrap()
        .trim_end_matches(';')
        .parse()
        .unwrap();
    assert_eq!(
        value(&facts, "/solver/time/delta_t"),
        FactValue::Number(written)
    );
    assert_eq!(written, step);

    // A steady study has no time facts.
    let steady = cavity("{}", "5000", "500", 0);
    let (_, facts) = read(&steady);
    assert!(!facts.keys().any(|k| k.starts_with("/solver/time/")));
    assert_eq!(value(&facts, "/solver/application"), word("simpleFoam"));
}

#[test]
fn a_potential_study_reads_potential() {
    let study = cavity(r#"{"init": "potential"}"#, "5000", "500", 0);
    let (files, facts) = read(&study);
    assert_eq!(check_roundtrip(&EsiReader, &files, &study), vec![]);
    assert_eq!(value(&facts, "/solver/init"), word("potential"));
    let (_, facts) = read(&cavity("{}", "5000", "500", 0));
    assert_eq!(value(&facts, "/solver/init"), word("uniform"));
}

#[test]
fn a_buoyant_box_study_passes() {
    let study = heated("", "1000", "100");
    let (files, facts) = read(&study);
    assert_eq!(check_roundtrip(&EsiReader, &files, &study), vec![]);
    same_keys(&study, &facts);
    assert_eq!(
        value(&facts, "/solver/application"),
        word("buoyantBoussinesqSimpleFoam")
    );
    for (member, wanted) in [
        ("gravity", 9.81),
        ("beta", 0.003),
        ("reference_temperature", 300.0),
        ("prandtl", 0.71),
        ("turbulent_prandtl", 0.85),
    ] {
        assert_eq!(
            value(&facts, &format!("/physics/buoyancy/{member}")),
            FactValue::Number(wanted),
            "{member}"
        );
    }
    // The hot wall is on the left and the cold one on the right; the other two are insulated.
    let face = |name: &str, member: &str| {
        value(
            &facts,
            &format!("/boundary_conditions/by_face/{name}/{member}"),
        )
    };
    assert_eq!(face("XMin", "thermal"), word("fixed"));
    assert_eq!(face("XMin", "temperature"), FactValue::Number(300.5));
    assert_eq!(face("XMax", "thermal"), word("fixed"));
    assert_eq!(face("XMax", "temperature"), FactValue::Number(299.5));
    for name in ["YMin", "YMax"] {
        assert_eq!(face(name, "thermal"), word("adiabatic"), "{name}");
        assert!(!facts.contains_key(&format!("/boundary_conditions/by_face/{name}/temperature")));
    }
    // Every wall is held by the flux condition of the pressure without its hydrostatic part.
    assert_eq!(face("XMin", "kind"), word("no-slip wall"));
}

#[test]
fn a_turbulent_box_study_passes() {
    let physics = |model: &str| {
        format!(
            r#"{{"turbulence": {{"model": "{model}", "intensity": 0.05, "viscosity_ratio": 10.0, "reference_velocity": {{"value": 0.1, "unit": "m/s"}}}}}}"#
        )
    };
    for (model, written, second) in [
        ("k_omega_sst", "kOmegaSST", "omega"),
        ("realizable_ke", "realizableKE", "epsilon"),
        ("rng_k_epsilon", "RNGkEpsilon", "epsilon"),
    ] {
        let study = cavity(&physics(model), "5000", "500", 0);
        let (files, facts) = read(&study);
        assert_eq!(
            check_roundtrip(&EsiReader, &files, &study),
            vec![],
            "{model}"
        );
        same_keys(&study, &facts);
        assert_eq!(
            value(&facts, "/solver/turbulence"),
            word(written),
            "{model}"
        );
        let inflow = inlet_turbulence(0.05, 10.0, 0.1, study.fluid.kinematic_viscosity).unwrap();
        assert_eq!(
            value(&facts, "/physics/turbulence/k"),
            FactValue::Number(inflow.k),
            "{model}"
        );
        let (name, number) = if second == "omega" {
            ("omega", inflow.omega)
        } else {
            ("epsilon", inflow.epsilon)
        };
        assert_eq!(
            value(&facts, &format!("/physics/turbulence/{name}")),
            FactValue::Number(number),
            "{model}"
        );
        let other = if second == "omega" {
            "epsilon"
        } else {
            "omega"
        };
        assert!(!facts.contains_key(&format!("/physics/turbulence/{other}")));
    }
    // A laminar study has no inflow values.
    let (_, facts) = read(&cavity("{}", "5000", "500", 0));
    assert!(!facts.keys().any(|k| k.starts_with("/physics/turbulence/")));
    assert_eq!(value(&facts, "/solver/turbulence"), word("laminar"));
}

#[test]
fn a_parabolic_inlet_reads_back() {
    let json = CHANNEL_V1.replacen(
        "\"kind\": \"VelocityInlet\",",
        "\"kind\": \"VelocityInlet\",\n      \"profile\": \"parabolic\",",
        1,
    );
    assert!(json.contains("parabolic"));
    let study = field_studies::resolved(&json);
    let (files, facts) = read(&study);
    assert_eq!(check_roundtrip(&EsiReader, &files, &study), vec![]);
    assert_eq!(
        value(&facts, "/boundary_conditions/by_face/XMin/profile"),
        word("parabolic")
    );
    // The mean speed is recovered from the polynomial, to the digit the study states.
    assert_eq!(
        value(&facts, "/boundary_conditions/by_face/XMin/velocity"),
        FactValue::Number(0.1)
    );
    // An ordinary inlet holds a fixed value, which carries no profile fact.
    let plain = field_studies::resolved(CHANNEL_V1);
    let (files, facts) = read(&plain);
    assert_eq!(check_roundtrip(&EsiReader, &files, &plain), vec![]);
    assert!(!facts.keys().any(|k| k.ends_with("/profile")));
}

const TRANSIENT: &str = r#"{"time":{"mode":"transient","delta_t":{"value":0.001,"unit":"s"}}}"#;
const PARABOLIC: &str = r#","profile":"parabolic""#;
const MEMBERS: &str = r#","forces":{"drag":{"boundary":"cylinder","reference_velocity":{"value":1,"unit":"m/s"},"reference_length":{"value":0.1,"unit":"m"}}},"probes":{"front":{"x":{"value":0.15,"unit":"m"},"y":{"value":0.2,"unit":"m"},"field":"pressure","boundary":"cylinder"},"rear":{"x":{"value":0.25,"unit":"m"},"y":{"value":0.2,"unit":"m"},"field":"pressure","boundary":"cylinder"}}"#;

fn with_members(json: &str, members: &str) -> String {
    json.replacen(
        r#""centreline_u_sample_count":64"#,
        &format!(r#""centreline_u_sample_count":64{members}"#),
        1,
    )
}

#[test]
fn the_new_monitors_read_back() {
    // A force and two probes on the cylinder: the patch they read is the backend's to name.
    let json = with_members(
        &field_studies::cylinder_json(TRANSIENT, "8", "0.5", PARABOLIC),
        MEMBERS,
    );
    let study = field_studies::resolved(&json);
    let (files, facts) = read(&study);
    let extras = shaped_expected_facts(&study);
    assert_eq!(
        check_roundtrip_with(&EsiReader, &files, &study, &extras),
        vec![]
    );
    assert!(
        !check_roundtrip(&EsiReader, &files, &study).is_empty(),
        "the geometry of a shaped study is left to the extras"
    );
    assert_eq!(
        value(&facts, "/monitors/force/1/reference_velocity"),
        FactValue::Number(1.0)
    );
    assert_eq!(
        value(&facts, "/monitors/force/1/reference_length"),
        FactValue::Number(0.1)
    );
    assert_eq!(value(&facts, "/monitors/force/1/lift_axis"), word("y"));
    assert_eq!(value(&facts, "/monitors/force/1/patch"), word("obstacle"));
    for n in 1..=2 {
        assert_eq!(
            value(&facts, &format!("/monitors/probe/{n}/on_boundary")),
            FactValue::Flag(true)
        );
        assert_eq!(
            value(&facts, &format!("/monitors/probe/{n}/patch")),
            word("obstacle")
        );
        assert_eq!(
            value(&facts, &format!("/monitors/probe/{n}/field")),
            word("pressure")
        );
    }
    assert_eq!(
        value(&facts, "/monitors/probe/1/x"),
        FactValue::Number(0.15)
    );
    assert_eq!(
        value(&facts, "/monitors/probe/2/x"),
        FactValue::Number(0.25)
    );

    // A probe in the flow is not a boundary probe.
    let (_, facts) = read(&cavity("{}", "5000", "500", 0));
    assert!(!facts.keys().any(|k| k.ends_with("/on_boundary")));

    // The heat through the hot and the cold wall.
    let heat = with_members(
        &field_studies::heated_json("", "1000", "100"),
        r#","surfaces":{"cold_flux":{"boundary":"cold","quantity":"heat_flux"},"hot_flux":{"boundary":"hot","quantity":"heat_flux"}}"#,
    );
    let study = field_studies::resolved(&heat);
    let (files, facts) = read(&study);
    assert_eq!(check_roundtrip(&EsiReader, &files, &study), vec![]);
    for n in 1..=2 {
        assert_eq!(
            value(&facts, &format!("/monitors/surface/{n}/quantity")),
            word("heat_flux")
        );
        assert_eq!(
            value(&facts, &format!("/monitors/surface/{n}/kind")),
            word("no-slip wall")
        );
    }
}
