//! Direction parameters, the checks on a whole sweep, and the stored form.

use cfdcraft_sweep::{
    Parameter, Sweep, SweepError, Target, Values, check_sweep, point_count, points,
};
use serde_json::json;

fn list(items: &[f64]) -> Values {
    Values::List {
        items: items.to_vec(),
    }
}

fn direction(name: &str, base_x: f64, base_y: f64, values: Values) -> Parameter {
    Parameter {
        name: name.to_owned(),
        target: Target::Direction {
            setting_x: "/wind/x".to_owned(),
            setting_y: "/wind/y".to_owned(),
            unit: "m/s".to_owned(),
            base_x,
            base_y,
        },
        values,
    }
}

fn sweep(parameters: Vec<Parameter>) -> Sweep {
    Sweep {
        id: "wind".to_owned(),
        base: "base".to_owned(),
        parameters,
    }
}

fn one(setting: &str, values: Values) -> Parameter {
    Parameter {
        name: "p".to_owned(),
        target: Target::Number {
            setting: setting.to_owned(),
        },
        values,
    }
}

/// The two components of a direction point, as numbers.
fn xy(p: &cfdcraft_sweep::Point) -> (f64, f64) {
    assert_eq!(p.assignments.len(), 2);
    assert_eq!(p.assignments[0].setting, "/wind/x");
    assert_eq!(p.assignments[1].setting, "/wind/y");
    for a in &p.assignments {
        assert_eq!(a.value["unit"], "m/s");
    }
    (
        p.assignments[0].value["value"].as_f64().unwrap(),
        p.assignments[1].value["value"].as_f64().unwrap(),
    )
}

#[test]
fn a_direction_turns_the_vector_by_the_angle() {
    let s = sweep(vec![direction(
        "wind_dir",
        10.0,
        0.0,
        list(&[0.0, 90.0, 180.0, 270.0]),
    )]);
    let got = points(&s).unwrap();
    let comps: Vec<(f64, f64)> = got.iter().map(xy).collect();
    assert_eq!(
        comps,
        vec![(10.0, 0.0), (0.0, 10.0), (-10.0, 0.0), (0.0, -10.0)]
    );
    for p in &got {
        for a in &p.assignments {
            assert!(
                !a.value.to_string().contains(r#""value":-0"#),
                "negative zero in {}",
                a.value
            );
        }
    }
}

#[test]
fn other_angles_use_the_sine_and_cosine() {
    let s = sweep(vec![direction("wind_dir", 10.0, 0.0, list(&[45.0]))]);
    let (x, y) = xy(&points(&s).unwrap()[0]);
    assert!((x - 7.0710678118655).abs() < 1e-12, "{x}");
    assert!((y - 7.0710678118655).abs() < 1e-12, "{y}");

    let s = sweep(vec![direction("wind_dir", 0.0, 1.0, list(&[30.0]))]);
    let (x, y) = xy(&points(&s).unwrap()[0]);
    assert!((x + 0.5).abs() < 1e-12, "{x}");
    assert!((y - 0.866025403784).abs() < 1e-12, "{y}");

    let s = sweep(vec![direction("wind_dir", 3.0, 4.0, list(&[90.0]))]);
    assert_eq!(xy(&points(&s).unwrap()[0]), (-4.0, 3.0));
}

#[test]
fn a_direction_works_with_a_range_of_angles() {
    let s = sweep(vec![direction(
        "wind_dir",
        10.0,
        0.0,
        Values::Range {
            start: 0.0,
            stop: 350.0,
            step: 10.0,
        },
    )]);
    assert_eq!(points(&s).unwrap().len(), 36);

    let s = sweep(vec![direction(
        "wind_dir",
        10.0,
        0.0,
        list(&[-90.0, 270.0, 450.0, 90.0]),
    )]);
    let got: Vec<(f64, f64)> = points(&s).unwrap().iter().map(xy).collect();
    assert_eq!(got[0], got[1]);
    assert_eq!(got[2], got[3]);
    assert_eq!(got[0], (0.0, -10.0));
    assert_eq!(got[2], (0.0, 10.0));
}

#[test]
fn a_direction_is_labelled_by_its_angle() {
    let s = sweep(vec![direction("wind_dir", 10.0, 0.0, list(&[90.0]))]);
    let p = &points(&s).unwrap()[0];
    assert_eq!(p.label, "wind_dir 90");
    assert_eq!(p.values, vec![90.0]);
}

#[test]
fn a_direction_needs_a_vector_that_can_be_turned() {
    for (bx, by) in [(0.0, 0.0), (f64::NAN, 1.0), (1.0, f64::INFINITY)] {
        let s = sweep(vec![direction("wind_dir", bx, by, list(&[0.0]))]);
        assert!(matches!(
            check_sweep(&s).unwrap_err(),
            SweepError::BadValues { .. }
        ));
    }
}

#[test]
fn a_count_must_be_a_whole_non_negative_number() {
    let count = |v: f64| {
        sweep(vec![Parameter {
            name: "cells".to_owned(),
            target: Target::Count {
                setting: "/mesh/cells_x".to_owned(),
            },
            values: list(&[v]),
        }])
    };
    for v in [2.5, -1.0, 9_007_199_254_740_994.0] {
        assert!(
            matches!(points(&count(v)).unwrap_err(), SweepError::NotACount { .. }),
            "{v}"
        );
        assert!(matches!(
            check_sweep(&count(v)).unwrap_err(),
            SweepError::NotACount { .. }
        ));
    }
    let got = points(&count(40.0)).unwrap();
    assert_eq!(got[0].assignments[0].value, json!(40));
    assert!(got[0].assignments[0].value.is_u64());
}

#[test]
fn names_ids_and_settings_are_checked() {
    let ok = || sweep(vec![one("/a", list(&[1.0]))]);
    assert!(check_sweep(&ok()).is_ok());

    let long_id = "a".repeat(37);
    for id in ["Lid", "", long_id.as_str(), "-lid", "li_d"] {
        let mut s = ok();
        s.id = id.to_owned();
        assert!(
            matches!(check_sweep(&s).unwrap_err(), SweepError::BadId { .. }),
            "{id:?}"
        );
    }
    let mut s = ok();
    s.id = "a".repeat(36);
    assert!(check_sweep(&s).is_ok());

    let long_name = "a".repeat(25);
    for name in ["", "1cells", long_name.as_str(), "_x", "Cells", "a-b"] {
        let mut s = ok();
        s.parameters[0].name = name.to_owned();
        assert!(
            matches!(check_sweep(&s).unwrap_err(), SweepError::BadName { .. }),
            "{name:?}"
        );
    }

    let mut s = sweep(vec![one("/a", list(&[1.0])), one("/b", list(&[1.0]))]);
    assert_eq!(
        check_sweep(&s).unwrap_err(),
        SweepError::DuplicateName {
            name: "p".to_owned()
        }
    );
    s.parameters[1].name = "q".to_owned();
    assert!(check_sweep(&s).is_ok());

    let long_setting = format!("/{}", "a".repeat(200));
    for setting in ["", "a", "/", long_setting.as_str(), "/a\nb"] {
        let s = sweep(vec![one(setting, list(&[1.0]))]);
        assert!(
            matches!(check_sweep(&s).unwrap_err(), SweepError::BadSetting { .. }),
            "{setting:?}"
        );
    }
    assert!(
        check_sweep(&sweep(vec![one(
            &format!("/{}", "a".repeat(199)),
            list(&[1.0])
        )]))
        .is_ok()
    );

    for unit in ["", "m\n/s"] {
        let mut s = sweep(vec![direction("d", 1.0, 0.0, list(&[0.0]))]);
        if let Target::Direction { unit: u, .. } = &mut s.parameters[0].target {
            *u = unit.to_owned();
        }
        assert!(matches!(
            check_sweep(&s).unwrap_err(),
            SweepError::BadUnit { .. }
        ));
    }
    let s = sweep(vec![Parameter {
        name: "lid".to_owned(),
        target: Target::Quantity {
            setting: "/lid".to_owned(),
            unit: String::new(),
        },
        values: list(&[1.0]),
    }]);
    assert!(matches!(
        check_sweep(&s).unwrap_err(),
        SweepError::BadUnit { .. }
    ));

    assert_eq!(
        check_sweep(&sweep(Vec::new())).unwrap_err(),
        SweepError::NoParameters
    );
    assert_eq!(point_count(&ok()).unwrap(), 1);
}

#[test]
fn the_assignments_of_a_point_set_each_setting_once() {
    let mut second = one("/a", list(&[2.0]));
    second.name = "q".to_owned();
    let s = sweep(vec![one("/a", list(&[1.0])), second]);
    assert_eq!(
        check_sweep(&s).unwrap_err(),
        SweepError::BadSetting {
            name: "q".to_owned(),
            setting: "/a".to_owned()
        }
    );
    assert!(points(&s).is_err());

    // A direction counts both of its settings.
    let mut other = one("/wind/y", list(&[2.0]));
    other.name = "q".to_owned();
    let s = sweep(vec![direction("wind_dir", 1.0, 0.0, list(&[0.0])), other]);
    assert!(matches!(
        check_sweep(&s).unwrap_err(),
        SweepError::BadSetting { .. }
    ));

    // And its two settings must differ.
    let mut s = sweep(vec![direction("wind_dir", 1.0, 0.0, list(&[0.0]))]);
    if let Target::Direction { setting_y, .. } = &mut s.parameters[0].target {
        *setting_y = "/wind/x".to_owned();
    }
    assert!(matches!(
        check_sweep(&s).unwrap_err(),
        SweepError::BadSetting { .. }
    ));
}

fn every_kind() -> Sweep {
    let quantity = |name: &str, setting: &str, values: Values| Parameter {
        name: name.to_owned(),
        target: Target::Quantity {
            setting: setting.to_owned(),
            unit: "m/s".to_owned(),
        },
        values,
    };
    Sweep {
        id: "all".to_owned(),
        base: "base".to_owned(),
        parameters: vec![
            quantity("lid", "/lid", list(&[0.1, 0.2])),
            Parameter {
                name: "cells".to_owned(),
                target: Target::Count {
                    setting: "/cells".to_owned(),
                },
                values: Values::Range {
                    start: 20.0,
                    stop: 40.0,
                    step: 10.0,
                },
            },
            Parameter {
                name: "limit".to_owned(),
                target: Target::Number {
                    setting: "/limit".to_owned(),
                },
                values: Values::Steps {
                    start: 0.001,
                    stop: 0.01,
                    count: 2,
                },
            },
            direction("wind_dir", 10.0, 0.0, list(&[0.0, 90.0])),
        ],
    }
}

#[test]
fn a_sweep_survives_a_json_round_trip() {
    let s = every_kind();
    let text = serde_json::to_string(&s).unwrap();
    let back: Sweep = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s);
    assert_eq!(points(&back).unwrap(), points(&s).unwrap());
    assert_eq!(point_count(&s).unwrap(), 2 * 3 * 2 * 2);

    // The stored shape uses the documented tags.
    let value = serde_json::to_value(&s).unwrap();
    assert_eq!(value["parameters"][0]["target"]["kind"], "quantity");
    assert_eq!(value["parameters"][0]["values"]["mode"], "list");
    assert_eq!(value["parameters"][1]["values"]["mode"], "range");
    assert_eq!(value["parameters"][2]["values"]["mode"], "steps");
    assert_eq!(value["parameters"][3]["target"]["kind"], "direction");

    // Unknown members are refused, at every level.
    let mut extra = value.clone();
    extra["surprise"] = json!(1);
    assert!(serde_json::from_value::<Sweep>(extra).is_err());
    let mut extra = value.clone();
    extra["parameters"][0]["surprise"] = json!(1);
    assert!(serde_json::from_value::<Sweep>(extra).is_err());
    let mut extra = value.clone();
    extra["parameters"][0]["target"]["surprise"] = json!(1);
    assert!(serde_json::from_value::<Sweep>(extra).is_err());
    let mut extra = value.clone();
    extra["parameters"][1]["values"]["surprise"] = json!(1);
    assert!(serde_json::from_value::<Sweep>(extra).is_err());
    let mut extra = value;
    extra["parameters"][0]["target"]["kind"] = json!("mystery");
    assert!(serde_json::from_value::<Sweep>(extra).is_err());
}
