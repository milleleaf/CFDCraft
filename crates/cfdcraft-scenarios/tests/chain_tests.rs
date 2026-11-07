use cfdcraft_model::Study;
use cfdcraft_scenarios::{
    BASE, MAX_DEPTH, MAX_NAME, MAX_NOTES, MAX_OVERRIDES, MAX_SCENARIOS, Origin, Override, Scenario,
    ScenarioError, chain, check_scenarios, effective_study,
};
use serde_json::json;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

/// A scenario with this id and parent and nothing else.
fn sc(id: &str, parent: Option<&str>) -> Scenario {
    Scenario {
        id: id.to_string(),
        parent: parent.map(str::to_string),
        name: None,
        pinned: false,
        notes: None,
        overrides: Vec::new(),
        origin: None,
    }
}

fn first_level(id: &str) -> Scenario {
    sc(id, Some(BASE))
}

fn cells(setting: &str) -> Override {
    Override::Set {
        setting: setting.to_string(),
        value: json!(64),
    }
}

#[test]
fn a_first_level_variant_names_base_as_its_parent() {
    assert_eq!(check_scenarios(&[first_level("fast")]), Ok(()));
    assert_eq!(check_scenarios(&[]), Ok(()));
}

#[test]
fn a_parent_must_come_before_its_children() {
    let a = first_level("a");
    let b = sc("b", Some("a"));
    assert_eq!(
        check_scenarios(&[b.clone(), a.clone()]),
        Err(ScenarioError::ParentNotEarlier {
            scenario: "b".to_string(),
            parent: "a".to_string()
        })
    );
    assert_eq!(check_scenarios(&[a, b]), Ok(()));
    assert!(matches!(
        check_scenarios(&[sc("loop", Some("loop"))]),
        Err(ScenarioError::ParentNotEarlier { .. })
    ));
}

#[test]
fn an_unknown_parent_is_refused() {
    assert_eq!(
        check_scenarios(&[sc("b", Some("nope"))]),
        Err(ScenarioError::UnknownParent {
            scenario: "b".to_string(),
            parent: "nope".to_string()
        })
    );
    assert!(matches!(
        check_scenarios(&[sc("b", None)]),
        Err(ScenarioError::UnknownParent { .. })
    ));
}

#[test]
fn duplicate_ids_are_refused() {
    assert_eq!(
        check_scenarios(&[first_level("a"), first_level("a")]),
        Err(ScenarioError::Duplicate {
            scenario: "a".to_string()
        })
    );
}

#[test]
fn the_ids_of_variants_are_plain_names_and_base_is_reserved() {
    let long = "a".repeat(41);
    for id in ["Fast Lid", "", long.as_str(), "-a"] {
        assert!(
            matches!(
                check_scenarios(&[first_level(id)]),
                Err(ScenarioError::BadId { .. })
            ),
            "{id:?}"
        );
    }
    assert_eq!(check_scenarios(&[first_level(&"a".repeat(40))]), Ok(()));
    let mut base = sc(BASE, None);
    base.overrides = vec![cells("/mesh/cells_x")];
    assert!(matches!(
        check_scenarios(&[base]),
        Err(ScenarioError::BaseMisused { .. })
    ));
}

#[test]
fn the_base_record_holds_no_parent_and_no_overrides() {
    let mut base = sc(BASE, None);
    base.name = Some("Cavity".to_string());
    base.pinned = true;
    base.notes = Some("the reference case".to_string());
    assert_eq!(check_scenarios(std::slice::from_ref(&base)), Ok(()));

    let mut with_parent = base.clone();
    with_parent.parent = Some("a".to_string());
    assert!(matches!(
        check_scenarios(&[with_parent]),
        Err(ScenarioError::BaseMisused { .. })
    ));

    let mut with_override = base.clone();
    with_override.overrides = vec![cells("/mesh/cells_x")];
    assert!(matches!(
        check_scenarios(&[with_override]),
        Err(ScenarioError::BaseMisused { .. })
    ));

    let mut with_origin = base;
    with_origin.origin = Some(Origin {
        sweep: "s".to_string(),
        index: 0,
    });
    assert!(matches!(
        check_scenarios(&[with_origin]),
        Err(ScenarioError::BaseMisused { .. })
    ));
}

/// `count` variants, each the child of the one before, the first a child of the base.
fn deep_chain(count: usize) -> Vec<Scenario> {
    (1..=count)
        .map(|n| {
            let parent = if n == 1 {
                BASE.to_string()
            } else {
                format!("s{}", n - 1)
            };
            sc(&format!("s{n}"), Some(parent.as_str()))
        })
        .collect()
}

#[test]
fn depth_count_and_override_limits_are_enforced() {
    assert_eq!(check_scenarios(&deep_chain(MAX_DEPTH)), Ok(()));
    assert_eq!(
        check_scenarios(&deep_chain(MAX_DEPTH + 1)),
        Err(ScenarioError::TooDeep {
            scenario: format!("s{}", MAX_DEPTH + 1)
        })
    );

    let many = |count: usize| -> Vec<Scenario> {
        (0..count).map(|n| first_level(&format!("v{n}"))).collect()
    };
    assert_eq!(check_scenarios(&many(MAX_SCENARIOS)), Ok(()));
    assert_eq!(
        check_scenarios(&many(MAX_SCENARIOS + 1)),
        Err(ScenarioError::TooMany {
            count: MAX_SCENARIOS + 1
        })
    );

    let with_overrides = |count: usize| {
        let mut s = first_level("heavy");
        s.overrides = (0..count).map(|_| cells("/mesh/cells_x")).collect();
        s
    };
    assert_eq!(check_scenarios(&[with_overrides(MAX_OVERRIDES)]), Ok(()));
    assert_eq!(
        check_scenarios(&[with_overrides(MAX_OVERRIDES + 1)]),
        Err(ScenarioError::TooManyOverrides {
            scenario: "heavy".to_string()
        })
    );
}

#[test]
fn names_and_notes_are_bounded_and_clean() {
    let named = |name: &str| {
        let mut s = first_level("v");
        s.name = Some(name.to_string());
        check_scenarios(&[s])
    };
    assert_eq!(named(&"n".repeat(MAX_NAME)), Ok(()));
    // Characters are counted, not bytes.
    assert_eq!(named(&"é".repeat(MAX_NAME)), Ok(()));
    for bad in [
        "n".repeat(MAX_NAME + 1),
        String::new(),
        "two\nlines".to_string(),
        "tab\there".to_string(),
        "bell\u{7}".to_string(),
    ] {
        assert_eq!(
            named(&bad),
            Err(ScenarioError::BadName {
                scenario: "v".to_string()
            }),
            "{bad:?}"
        );
    }

    let noted = |notes: &str| {
        let mut s = first_level("v");
        s.notes = Some(notes.to_string());
        check_scenarios(&[s])
    };
    assert_eq!(noted(&"n".repeat(MAX_NOTES)), Ok(()));
    assert_eq!(noted("first line\nsecond line"), Ok(()));
    for bad in [
        "n".repeat(MAX_NOTES + 1),
        "tab\there".to_string(),
        "bell\u{7}".to_string(),
    ] {
        assert_eq!(
            noted(&bad),
            Err(ScenarioError::BadNotes {
                scenario: "v".to_string()
            }),
            "{bad:?}"
        );
    }
}

#[test]
fn a_setting_in_a_stored_override_must_be_well_formed() {
    let mut s = first_level("v");
    s.overrides = vec![cells("lid")];
    assert!(matches!(
        check_scenarios(&[s]),
        Err(ScenarioError::BadSetting { .. })
    ));
}

#[test]
fn chain_lists_ancestors_root_first() {
    let list = [
        sc(BASE, None),
        first_level("a"),
        sc("b", Some("a")),
        sc("c", Some("b")),
        first_level("other"),
    ];
    let ids = |id: &str| -> Vec<String> {
        chain(&list, id)
            .unwrap()
            .iter()
            .map(|s| s.id.clone())
            .collect()
    };
    assert_eq!(ids("c"), ["a", "b", "c"]);
    assert_eq!(ids("a"), ["a"]);
    assert_eq!(ids("other"), ["other"]);
    assert!(ids(BASE).is_empty());
}

#[test]
fn effective_study_of_base_is_the_base() {
    let base = Study::from_json(CAVITY).unwrap();
    assert_eq!(effective_study(&base, &[], BASE).unwrap(), base);
    // A record for the base study with a label changes nothing about the study.
    let mut labelled = sc(BASE, None);
    labelled.name = Some("Cavity".to_string());
    assert_eq!(effective_study(&base, &[labelled], BASE).unwrap(), base);
}

#[test]
fn an_unknown_scenario_is_refused() {
    let base = Study::from_json(CAVITY).unwrap();
    assert_eq!(
        effective_study(&base, &[first_level("a")], "nope"),
        Err(ScenarioError::Unknown {
            scenario: "nope".to_string()
        })
    );
    assert!(matches!(
        chain(&[first_level("a")], "nope"),
        Err(ScenarioError::Unknown { .. })
    ));
}
