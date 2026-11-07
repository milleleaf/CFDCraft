use cfdcraft_model::{Quantity, ResolveError, Study, StudyError, Unit, resolve, units};

/// The (code, pointer) of the first error `resolve` reports.
fn first_error(study: &Study) -> (String, String) {
    match resolve(study) {
        Err(ResolveError::Diagnostics(found)) => (found[0].code.clone(), found[0].pointer.clone()),
        other => panic!("expected diagnostics, got {other:?}"),
    }
}

fn study() -> Study {
    Study::from_json(include_str!("fixtures/cavity_v1.json")).unwrap()
}

fn lid(study: &mut Study) -> &mut Quantity {
    study
        .boundary_conditions
        .get_mut("lid")
        .unwrap()
        .velocity
        .as_mut()
        .unwrap()
}

#[test]
fn millimetres_and_metres_resolve_identically() {
    let metres = study();
    let mut millimetres = metres.clone();
    for q in [
        &mut millimetres.domain.length_x,
        &mut millimetres.domain.length_y,
        &mut millimetres.domain.length_z,
    ] {
        q.value *= 1000.0;
        q.unit = Unit::Millimetre;
    }
    assert_eq!(resolve(&metres).unwrap(), resolve(&millimetres).unwrap());
}

#[test]
fn decimal_millimetres_have_identical_si_values_and_hashes() {
    for (mm, m) in [(19.1, 0.0191), (0.1, 0.0001), (1234.567, 1.234567)] {
        let mut a = study();
        let mut b = study();
        a.domain.length_x = Quantity {
            value: mm,
            unit: Unit::Millimetre,
        };
        b.domain.length_x = Quantity {
            value: m,
            unit: Unit::Metre,
        };
        let a = resolve(&a).unwrap();
        let b = resolve(&b).unwrap();
        assert_eq!(a, b, "{mm} mm vs {m} m");
        assert_eq!(
            cfdcraft_model::canonical_hash(&a),
            cfdcraft_model::canonical_hash(&b)
        );
    }
}

#[test]
fn every_quantity_is_converted_exactly_once() {
    units::reset_conversion_count();
    let resolved = resolve(&study()).unwrap();
    // Three lengths, viscosity, lid velocity, end time and write interval.
    assert_eq!(units::conversion_count(), 7);
    let nu = resolved.fluid.kinematic_viscosity;
    assert_eq!(nu, 0.0001);
    let lid_velocity = resolved.lid_velocity().unwrap();
    assert!((lid_velocity * resolved.domain.length_x / nu - 100.0).abs() < 1e-12);
    assert_eq!(resolved.mesh.cells_z, 1);
}

#[test]
fn quantity_requires_unit() {
    assert!(serde_json::from_str::<Quantity>(r#"{"value":0.1}"#).is_err());
}

#[test]
fn unsupported_schema_version_is_typed_and_names_found_value() {
    let input = include_str!("fixtures/cavity_v1.json")
        .replace("\"schema_version\": 1", "\"schema_version\": 2");
    assert!(matches!(
        Study::from_json(&input),
        Err(StudyError::UnsupportedSchemaVersion { found: 2 })
    ));
    let error = serde_json::from_str::<Study>(&input)
        .unwrap_err()
        .to_string();
    assert!(error.contains("UnsupportedSchemaVersion") && error.contains('2'));
}

#[test]
fn schema_0_input_resolves_identically_through_the_migration() {
    let old = Study::from_json(include_str!("fixtures/cavity_v0.json")).unwrap();
    assert_eq!(resolve(&old).unwrap(), resolve(&study()).unwrap());
}

#[test]
fn unknown_fields_and_unknown_units_are_rejected() {
    let mut value = serde_json::to_value(study()).unwrap();
    value["camera"] = serde_json::json!({});
    assert!(Study::from_json(&value.to_string()).is_err());
    assert!(serde_json::from_str::<Quantity>(r#"{"value":0.1,"unit":"parsecs"}"#).is_err());
    assert!(serde_json::from_str::<Quantity>(r#"{"value":0.1,"unit":"m","extra":1}"#).is_err());
}

#[test]
fn wrong_dimensions_are_rejected_instead_of_becoming_silent_si_values() {
    let mut input = study();
    input.domain.length_x.unit = Unit::Second;
    assert_eq!(
        first_error(&input),
        ("CFD-UNIT-001".to_owned(), "/domain/length_x".to_owned())
    );
    input = study();
    input.fluid.kinematic_viscosity.unit = Unit::MetrePerSecond;
    assert_eq!(
        first_error(&input),
        (
            "CFD-UNIT-001".to_owned(),
            "/fluid/kinematic_viscosity".to_owned()
        )
    );
    input = study();
    lid(&mut input).unit = Unit::Metre;
    assert_eq!(
        first_error(&input),
        (
            "CFD-UNIT-001".to_owned(),
            "/boundary_conditions/lid/velocity".to_owned()
        )
    );
}

#[test]
fn degenerate_physics_and_controls_have_explicit_errors() {
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut input = study();
        input.domain.length_x.value = invalid;
        assert!(resolve(&input).is_err(), "length {invalid}");
        input = study();
        input.fluid.kinematic_viscosity.value = invalid;
        assert!(resolve(&input).is_err(), "viscosity {invalid}");
        input = study();
        input.solver.end_time.value = invalid;
        assert!(resolve(&input).is_err(), "end time {invalid}");
        input = study();
        input.solver.write_interval.value = invalid;
        assert!(resolve(&input).is_err(), "write interval {invalid}");
    }
    for invalid in [0.0, -0.0, f64::NAN, f64::INFINITY] {
        let mut input = study();
        lid(&mut input).value = invalid;
        assert!(resolve(&input).is_err());
    }
    for axis in 0..3 {
        let mut input = study();
        match axis {
            0 => input.mesh.cells_x = 0,
            1 => input.mesh.cells_y = 0,
            _ => input.mesh.cells_z = 0,
        }
        assert!(resolve(&input).is_err());
    }
    for interval in [0.5, 300.0, 3000.0] {
        let mut input = study();
        input.solver.write_interval.value = interval;
        assert!(resolve(&input).is_err());
    }
    let mut input = study();
    input.monitors.centreline_u_sample_count = 1;
    assert!(resolve(&input).is_err());
    input = study();
    input.solver.convergence.p = f64::NAN;
    assert!(resolve(&input).is_err());
}

#[test]
fn negative_lid_velocity_is_valid_and_zero_is_not() {
    let mut input = study();
    lid(&mut input).value = -0.1;
    assert_eq!(resolve(&input).unwrap().lid_velocity(), Some(-0.1));
    lid(&mut input).value = -0.0;
    assert!(resolve(&input).is_err());
}

#[test]
fn cell_counts_keep_u64_boundary_values_without_loss() {
    let mut input = study();
    input.mesh.cells_x = 1;
    input.mesh.cells_y = u64::MAX;
    let resolved = resolve(&input).unwrap();
    assert_eq!(resolved.mesh.cells_x, 1);
    assert_eq!(resolved.mesh.cells_y, u64::MAX);
}

#[test]
fn all_unit_symbols_round_trip() {
    for unit in [
        Unit::Metre,
        Unit::Millimetre,
        Unit::MetrePerSecond,
        Unit::SquareMetrePerSecond,
        Unit::Pascal,
        Unit::Kelvin,
        Unit::Second,
        Unit::Dimensionless,
        Unit::Rpm,
    ] {
        let q = Quantity { value: 2.0, unit };
        assert_eq!(
            serde_json::from_str::<Quantity>(&serde_json::to_string(&q).unwrap()).unwrap(),
            q
        );
    }
}
