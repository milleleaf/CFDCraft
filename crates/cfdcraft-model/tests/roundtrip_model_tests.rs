use std::collections::BTreeMap;

use cfdcraft_model::{
    ResolvedStudy, Study,
    diagnostics::setting_label,
    resolve,
    roundtrip::{
        CaseReader, EVIDENCE_ONLY, Fact, FactValue, ReadError, SourceFile, check_roundtrip,
        expected_facts, uncovered_leaves,
    },
};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("fixtures/channel_v1.json");

fn resolved(json: &str) -> ResolvedStudy {
    resolve(&Study::from_json(json).unwrap()).unwrap()
}

fn leaves(study: &ResolvedStudy) -> serde_json::Value {
    serde_json::to_value(study).unwrap()
}

/// A reader that reports exactly the facts handed to it, so the checker is tested on its own.
struct Canned(Result<BTreeMap<String, Fact>, ReadError>);

impl CaseReader for Canned {
    fn read(&self, _: &[SourceFile]) -> Result<BTreeMap<String, Fact>, ReadError> {
        self.0.clone()
    }
}

fn perfect(study: &ResolvedStudy) -> BTreeMap<String, Fact> {
    expected_facts(study)
        .into_iter()
        .map(|e| {
            (
                e.key,
                Fact {
                    value: e.value,
                    value_spans: vec![],
                    entry_span: None,
                },
            )
        })
        .collect()
}

#[test]
fn every_leaf_of_the_resolved_study_is_a_recovered_fact_or_on_the_evidence_only_list() {
    for json in [CAVITY, CHANNEL] {
        let study = resolved(json);
        let missed = uncovered_leaves(&leaves(&study), &expected_facts(&study));
        assert_eq!(missed, Vec::<String>::new());
    }
    assert_eq!(
        EVIDENCE_ONLY,
        ["/schema_version", "/solver/convergence", "/monitors/settle"]
    );
}

#[test]
fn a_new_resolved_field_is_reported_as_uncovered() {
    let study = resolved(CAVITY);
    let mut json = leaves(&study);
    json["mesh"]["refinement"] = serde_json::json!(3);
    json["boundary_conditions"]["lid"]["roughness"] = serde_json::json!(0.5);
    json["solver"]["convergence"]["continuity"] = serde_json::json!(1e-6);
    let missed = uncovered_leaves(&json, &expected_facts(&study));
    assert_eq!(
        missed,
        ["/boundary_conditions/lid/roughness", "/mesh/refinement"]
    );
}

#[test]
fn the_checker_accepts_exactly_what_the_study_requires() {
    let study = resolved(CHANNEL);
    assert_eq!(
        check_roundtrip(&Canned(Ok(perfect(&study))), &[], &study),
        vec![]
    );
}

#[test]
fn negative_zero_equals_zero_and_nothing_near_it_does() {
    let study = resolved(CHANNEL);
    let mut facts = perfect(&study);
    facts.get_mut("/domain/origin_x").unwrap().value = FactValue::Number(-0.0);
    assert_eq!(
        check_roundtrip(&Canned(Ok(facts.clone())), &[], &study),
        vec![]
    );
    facts.get_mut("/domain/origin_x").unwrap().value = FactValue::Number(f64::MIN_POSITIVE);
    let found = check_roundtrip(&Canned(Ok(facts)), &[], &study);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].setting, "domain origin in x");
}

#[test]
fn a_value_of_the_wrong_type_is_a_mismatch() {
    let study = resolved(CHANNEL);
    let mut facts = perfect(&study);
    facts.get_mut("/mesh/cells_x").unwrap().value = FactValue::Number(100.0);
    let found = check_roundtrip(&Canned(Ok(facts)), &[], &study);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].pointer, "/mesh/cells_x");
    assert_eq!(found[0].setting, "cell count in x");
}

#[test]
fn an_extra_fact_and_a_read_error_both_fail_closed() {
    let study = resolved(CHANNEL);
    let mut facts = perfect(&study);
    facts.insert(
        "/mesh/refinement".into(),
        Fact {
            value: FactValue::Count(3),
            value_spans: vec![],
            entry_span: None,
        },
    );
    let found = check_roundtrip(&Canned(Ok(facts)), &[], &study);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, "CFD-RT-001");
    assert!(found[0].message.contains("does not account for"));

    let broken = ReadError {
        file: "system/controlDict".into(),
        line: 7,
        message: "a brace is never closed".into(),
    };
    let found = check_roundtrip(&Canned(Err(broken)), &[], &study);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, "CFD-RT-002");
    let at = found[0].location.as_ref().unwrap();
    assert_eq!((at.file.as_str(), at.line), ("system/controlDict", 7));
}

#[test]
fn settings_are_named_in_the_words_the_diagnostics_use() {
    for (pointer, words) in [
        ("/fluid/kinematic_viscosity", "kinematic viscosity"),
        ("/domain/length_y", "domain length in y"),
        ("/domain/open", "open-domain flag"),
        ("/mesh/cells_z", "cell count in z"),
        ("/solver/end_time", "end time"),
        ("/solver/write_interval", "write interval"),
        (
            "/monitors/centreline_u_sample_count",
            "centreline sample count",
        ),
        ("/boundary_conditions/inlet/velocity", "inlet velocity"),
        ("/boundary_conditions/inlet/kind", "inlet boundary kind"),
        ("/boundary_conditions/lid/faces", "lid faces"),
        ("/somewhere/else", "/somewhere/else"),
    ] {
        assert_eq!(setting_label(pointer), words, "{pointer}");
    }
}

/// The cavity with probes, a line, surfaces and settle tolerances listed.
fn cavity_with_monitors() -> ResolvedStudy {
    let mut value: serde_json::Value = serde_json::from_str(CAVITY).unwrap();
    value["monitors"]["probes"] = serde_json::json!({
        "centre": {"x": {"value": 0.05, "unit": "m"}, "y": {"value": 0.04, "unit": "m"}, "field": "pressure"}
    });
    value["monitors"]["lines"] = serde_json::json!({
        "mid": {
            "x0": {"value": 0.0, "unit": "m"}, "y0": {"value": 0.05, "unit": "m"},
            "x1": {"value": 0.1, "unit": "m"}, "y1": {"value": 0.05, "unit": "m"},
            "samples": 16, "field": "velocity"
        }
    });
    value["monitors"]["surfaces"] = serde_json::json!({
        "lidflux": {"boundary": "lid", "quantity": "flux"},
        "wallp": {"boundary": "walls", "quantity": "pressure_average"}
    });
    value["monitors"]["settle"] = serde_json::json!({"tail_fraction": 0.5});
    resolved(&value.to_string())
}

#[test]
fn the_default_probes_are_expected_facts_with_no_source_leaf() {
    let study = resolved(CAVITY);
    let facts = expected_facts(&study);
    let probe_facts: Vec<_> = facts
        .iter()
        .filter(|e| e.key.starts_with("/monitors/probe/"))
        .collect();
    assert_eq!(probe_facts.len(), 15);
    assert!(probe_facts.iter().all(|e| e.source_pointers.is_empty()));
    let x = facts
        .iter()
        .find(|e| e.key == "/monitors/probe/2/x")
        .unwrap();
    assert_eq!(x.value, FactValue::Number(0.025));
    assert_eq!(x.setting, "probe 2 x");
    let field = facts
        .iter()
        .find(|e| e.key == "/monitors/probe/5/field")
        .unwrap();
    assert_eq!(field.value, FactValue::Text("velocity".into()));
}

#[test]
fn listed_monitors_are_expected_facts_covered_by_their_list() {
    let study = cavity_with_monitors();
    let facts = expected_facts(&study);
    let want = |key: &str| facts.iter().find(|e| e.key == key).unwrap();
    assert_eq!(want("/monitors/probe/1/y").value, FactValue::Number(0.04));
    assert_eq!(
        want("/monitors/probe/1/field").value,
        FactValue::Text("pressure".into())
    );
    assert_eq!(want("/monitors/line/1/samples").value, FactValue::Count(16));
    assert_eq!(want("/monitors/line/1/x1").setting, "line 1 x1");
    assert_eq!(
        want("/monitors/surface/1/kind").value,
        FactValue::Text("moving wall".into())
    );
    assert_eq!(
        want("/monitors/surface/1/kind").setting,
        "surface 1 boundary kind"
    );
    assert_eq!(
        want("/monitors/surface/2/quantity").value,
        FactValue::Text("pressure_average".into())
    );
    assert_eq!(
        want("/monitors/probe/1/x").source_pointers,
        ["/monitors/probes"]
    );
    assert_eq!(
        want("/monitors/line/1/y0").source_pointers,
        ["/monitors/lines"]
    );
    assert_eq!(
        want("/monitors/surface/2/kind").source_pointers,
        ["/monitors/surfaces"]
    );
    assert!(
        !facts
            .iter()
            .any(|e| e.key.starts_with("/monitors/probe/2/"))
    );
    let missed = uncovered_leaves(&leaves(&study), &facts);
    assert_eq!(missed, Vec::<String>::new());
}

#[test]
fn a_monitor_value_that_differs_is_reported_in_the_monitors_words() {
    let study = cavity_with_monitors();
    let mut facts = perfect(&study);
    facts.get_mut("/monitors/line/1/samples").unwrap().value = FactValue::Count(8);
    facts.remove("/monitors/probe/1/x");
    let found = check_roundtrip(&Canned(Ok(facts)), &[], &study);
    let settings: Vec<_> = found.iter().map(|d| d.setting.as_str()).collect();
    assert_eq!(settings, ["line 1 samples", "probe 1 x"]);
    assert!(found.iter().all(|d| d.code == "CFD-RT-001"));
}
