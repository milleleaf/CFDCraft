//! Values and the full factorial.

use cfdcraft_sweep::{
    MAX_VALUES, Parameter, Sweep, SweepError, Target, Values, expand, point_count, points,
};

fn list(items: &[f64]) -> Values {
    Values::List {
        items: items.to_vec(),
    }
}

fn quantity(name: &str, setting: &str, values: Values) -> Parameter {
    Parameter {
        name: name.to_owned(),
        target: Target::Quantity {
            setting: setting.to_owned(),
            unit: "m/s".to_owned(),
        },
        values,
    }
}

fn cells(values: Values) -> Parameter {
    Parameter {
        name: "cells".to_owned(),
        target: Target::Count {
            setting: "/mesh/cells_x".to_owned(),
        },
        values,
    }
}

fn lid_and_cells() -> Sweep {
    Sweep {
        id: "lid".to_owned(),
        base: "base".to_owned(),
        parameters: vec![
            quantity(
                "lid_speed",
                "/boundary_conditions/lid/velocity",
                list(&[0.1, 0.2]),
            ),
            cells(list(&[20.0, 40.0, 80.0])),
        ],
    }
}

#[test]
fn a_list_gives_its_items_in_order() {
    assert_eq!(
        expand(&list(&[0.1, 0.5, 2.0])).unwrap(),
        vec![0.1, 0.5, 2.0]
    );
}

#[test]
fn a_range_includes_its_stop_when_the_steps_land_on_it() {
    let got = expand(&Values::Range {
        start: 0.1,
        stop: 0.5,
        step: 0.1,
    })
    .unwrap();
    assert_eq!(got, vec![0.1, 0.2, 0.3, 0.4, 0.5]);
    let got = expand(&Values::Range {
        start: 1.0,
        stop: 10.0,
        step: 4.0,
    })
    .unwrap();
    assert_eq!(got, vec![1.0, 5.0, 9.0]);
}

#[test]
fn steps_split_the_interval_evenly() {
    let got = expand(&Values::Steps {
        start: 0.0,
        stop: 1.0,
        count: 5,
    })
    .unwrap();
    assert_eq!(got, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    let got = expand(&Values::Steps {
        start: 2.0,
        stop: 3.0,
        count: 2,
    })
    .unwrap();
    assert_eq!(got, vec![2.0, 3.0]);
}

#[test]
fn a_range_that_runs_the_wrong_way_or_has_no_step_is_refused() {
    let range = |start: f64, stop: f64, step: f64| Values::Range { start, stop, step };
    let steps = |count: u32| Values::Steps {
        start: 0.0,
        stop: 1.0,
        count,
    };
    let refused = [
        range(0.0, 1.0, 0.0),
        range(0.0, 1.0, -0.1),
        range(1.0, 0.0, 0.1),
        range(0.0, f64::NAN, 0.1),
        range(0.0, f64::INFINITY, 0.1),
        range(f64::NEG_INFINITY, 1.0, 0.1),
        range(0.0, 1.0, f64::NAN),
        steps(0),
        steps(1),
        list(&[]),
        list(&[1.0, f64::NAN]),
        Values::List {
            items: vec![1.0; MAX_VALUES + 1],
        },
        range(0.0, 1.0e9, 1.0),
        range(0.0, 1.0, 1.0e-300),
        range(-1.0e308, 1.0e308, 1.0),
        steps(u32::MAX),
    ];
    for values in refused {
        let err = expand(&values).unwrap_err();
        assert!(
            matches!(
                err,
                SweepError::BadValues { .. } | SweepError::TooManyValues { .. }
            ),
            "{values:?} gave {err:?}"
        );
    }
    // Exactly the limit is allowed.
    let edge = Values::Range {
        start: 0.0,
        stop: 999.0,
        step: 1.0,
    };
    assert_eq!(expand(&edge).unwrap().len(), MAX_VALUES);
}

#[test]
fn two_parameters_give_their_full_factorial() {
    let got: Vec<Vec<f64>> = points(&lid_and_cells())
        .unwrap()
        .into_iter()
        .map(|p| p.values)
        .collect();
    assert_eq!(
        got,
        vec![
            vec![0.1, 20.0],
            vec![0.1, 40.0],
            vec![0.1, 80.0],
            vec![0.2, 20.0],
            vec![0.2, 40.0],
            vec![0.2, 80.0],
        ]
    );
    let first = &points(&lid_and_cells()).unwrap()[0];
    assert_eq!(first.assignments.len(), 2);
    assert_eq!(
        first.assignments[0].setting,
        "/boundary_conditions/lid/velocity"
    );
    assert_eq!(
        first.assignments[0].value,
        serde_json::json!({ "value": 0.1, "unit": "m/s" })
    );
    assert_eq!(first.assignments[1].value, serde_json::json!(20));
}

#[test]
fn points_have_deterministic_ids_and_labels() {
    let sweep = lid_and_cells();
    let got = points(&sweep).unwrap();
    let ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "lid-001", "lid-002", "lid-003", "lid-004", "lid-005", "lid-006"
        ]
    );
    assert_eq!(got[0].label, "lid_speed 0.1, cells 20");
    assert_eq!(got[3].label, "lid_speed 0.2, cells 20");
    assert_eq!(got[5].index, 5);
    assert_eq!(got, points(&sweep).unwrap());
    assert_eq!(point_count(&sweep).unwrap(), 6);
}

fn many(count: usize, values: Values) -> Sweep {
    Sweep {
        id: "big".to_owned(),
        base: "base".to_owned(),
        parameters: (0..count)
            .map(|i| quantity(&format!("p{i}"), &format!("/s/{i}"), values.clone()))
            .collect(),
    }
}

#[test]
fn the_point_limit_is_checked_before_any_point_is_built() {
    let ten = list(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    let three = many(3, ten.clone());
    let expected = SweepError::TooManyPoints {
        count: 1000,
        max: 200,
    };
    assert_eq!(point_count(&three).unwrap_err(), expected);
    assert_eq!(points(&three).unwrap_err(), expected);

    assert_eq!(
        point_count(&many(10, ten)).unwrap_err(),
        SweepError::TooManyParameters { count: 10, max: 8 }
    );

    // 1000 values in each of 8 parameters: the product does not fit a count, and nothing is built.
    let thousand = Values::Steps {
        start: 0.0,
        stop: 1.0,
        count: 1000,
    };
    let huge = many(8, thousand);
    assert!(matches!(
        point_count(&huge).unwrap_err(),
        SweepError::TooManyPoints { max: 200, .. }
    ));
    assert!(matches!(
        points(&huge).unwrap_err(),
        SweepError::TooManyPoints { max: 200, .. }
    ));
}
