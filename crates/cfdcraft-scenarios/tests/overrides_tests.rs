use cfdcraft_model::{Study, study::Convergence};
use cfdcraft_scenarios::{
    ApplyError, Override, Scenario, ScenarioError, apply_overrides, effective_study,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn base() -> Study {
    Study::from_json(CAVITY).unwrap()
}

fn base_json() -> Value {
    serde_json::from_str(CAVITY).unwrap()
}

fn set(setting: &str, value: Value) -> Override {
    Override::Set {
        setting: setting.to_string(),
        value,
    }
}

fn remove(setting: &str) -> Override {
    Override::Remove {
        setting: setting.to_string(),
    }
}

fn scenario(id: &str, parent: &str, overrides: Vec<Override>) -> Scenario {
    Scenario {
        id: id.to_string(),
        parent: Some(parent.to_string()),
        name: None,
        pinned: false,
        notes: None,
        overrides,
        origin: None,
    }
}

fn study_of(value: Value) -> Study {
    serde_json::from_value(value).unwrap()
}

const LID: &str = "/boundary_conditions/lid/velocity/value";

#[test]
fn a_variant_changes_only_what_it_overrides() {
    let study = base();
    let list = [scenario("fast-lid", "base", vec![set(LID, json!(0.2))])];
    let got = effective_study(&study, &list, "fast-lid").unwrap();
    let mut expected = base_json();
    expected["boundary_conditions"]["lid"]["velocity"]["value"] = json!(0.2);
    assert_eq!(got, study_of(expected));
    assert_ne!(got, study);
    assert_eq!(study, base());
}

#[test]
fn a_variant_of_a_variant_applies_root_to_leaf() {
    let study = base();
    let list = [
        scenario("fine", "base", vec![set("/mesh/cells_x", json!(256))]),
        scenario("fine-fast", "fine", vec![set(LID, json!(0.3))]),
    ];
    let mut cells_only = base_json();
    cells_only["mesh"]["cells_x"] = json!(256);
    let mut both = cells_only.clone();
    both["boundary_conditions"]["lid"]["velocity"]["value"] = json!(0.3);
    assert_eq!(
        effective_study(&study, &list, "fine").unwrap(),
        study_of(cells_only)
    );
    assert_eq!(
        effective_study(&study, &list, "fine-fast").unwrap(),
        study_of(both)
    );
}

#[test]
fn a_later_override_of_one_setting_wins() {
    let study = base();
    let list = [
        scenario(
            "a",
            "base",
            vec![set(LID, json!(0.2)), set(LID, json!(0.4))],
        ),
        scenario("b", "a", vec![set(LID, json!(0.3))]),
    ];
    let lid = |s: &Study| s.boundary_conditions["lid"].velocity.unwrap().value;
    assert_eq!(lid(&effective_study(&study, &list, "a").unwrap()), 0.4);
    assert_eq!(lid(&effective_study(&study, &list, "b").unwrap()), 0.3);
}

#[test]
fn removing_an_optional_member_restores_its_default() {
    let study = base();
    let looser = json!({"p": 0.001, "U": 0.001});
    let list = [
        scenario("loose", "base", vec![set("/solver/convergence", looser)]),
        scenario("back", "loose", vec![remove("/solver/convergence")]),
    ];
    let loose = effective_study(&study, &list, "loose").unwrap();
    assert_eq!(loose.solver.convergence.p, 0.001);
    let back = effective_study(&study, &list, "back").unwrap();
    assert_eq!(back.solver.convergence, Convergence::default());
}

#[test]
fn a_setting_the_study_does_not_have_is_refused() {
    let list = [scenario(
        "dense",
        "base",
        vec![set("/fluid/density", json!(1.2))],
    )];
    let error = effective_study(&base(), &list, "dense").unwrap_err();
    match error {
        ScenarioError::Apply {
            scenario,
            source: ApplyError::NotAStudy { .. },
        } => assert_eq!(scenario, "dense"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_pointer_into_a_missing_parent_is_refused() {
    let error = apply_overrides(
        &base_json(),
        &[set("/boundary_conditions/nope/kind", json!("NoSlipWall"))],
    )
    .unwrap_err();
    assert_eq!(
        error,
        ApplyError::MissingParent {
            setting: "/boundary_conditions/nope/kind".to_string()
        }
    );
}

#[test]
fn a_malformed_pointer_is_refused() {
    for setting in [
        "lid",
        "",
        "/a//b",
        "/a/",
        "/a/~2",
        "/boundary_conditions/lid/faces/0",
    ] {
        let error = apply_overrides(&base_json(), &[set(setting, json!(1))]).unwrap_err();
        assert!(
            matches!(
                error,
                ApplyError::BadSetting { .. } | ApplyError::NotAnObject { .. }
            ),
            "{setting:?}: {error:?}"
        );
    }
    let long = format!("/{}", "a".repeat(200));
    assert!(matches!(
        apply_overrides(&base_json(), &[set(&long, json!(1))]),
        Err(ApplyError::BadSetting { .. })
    ));
    let deep = "/a".repeat(13);
    assert!(matches!(
        apply_overrides(&base_json(), &[set(&deep, json!(1))]),
        Err(ApplyError::BadSetting { .. })
    ));
}

#[test]
fn escapes_in_a_pointer_are_decoded() {
    let plain = json!({"a/b": 1, "a~b": 2});
    let got = apply_overrides(&plain, &[set("/a~1b", json!(10)), set("/a~0b", json!(20))]).unwrap();
    assert_eq!(got, json!({"a/b": 10, "a~b": 20}));
}

#[test]
fn a_value_of_the_wrong_type_is_refused() {
    let list = [scenario(
        "words",
        "base",
        vec![set("/mesh/cells_x", json!("forty"))],
    )];
    let error = effective_study(&base(), &list, "words").unwrap_err();
    assert!(matches!(
        error,
        ScenarioError::Apply {
            source: ApplyError::NotAStudy { .. },
            ..
        }
    ));
}

#[test]
fn removing_what_is_not_there_is_refused() {
    let error = apply_overrides(&base_json(), &[remove("/solver/nothing")]).unwrap_err();
    assert_eq!(
        error,
        ApplyError::NothingToRemove {
            setting: "/solver/nothing".to_string()
        }
    );
}

#[test]
fn the_first_raw_override_of_a_study_without_any_is_added_under_a_new_map() {
    let study = json!({"domain": {"length_x": 1}});
    let out = apply_overrides(
        &study,
        &[Override::Set {
            setting: "/raw_overrides/blk".to_owned(),
            value: json!({"file": "system/blockMeshDict"}),
        }],
    )
    .unwrap();
    assert_eq!(out["raw_overrides"]["blk"]["file"], "system/blockMeshDict");
    // the input is untouched, and other missing parents are still refused
    assert!(study.get("raw_overrides").is_none());
    let refused = apply_overrides(
        &study,
        &[Override::Set {
            setting: "/fluid/density".to_owned(),
            value: json!(1),
        }],
    );
    assert!(matches!(refused, Err(ApplyError::MissingParent { .. })));
}
