use cfdcraft_model::{Study, Unit, canonical_hash, resolve};

fn study() -> Study {
    Study::from_json(include_str!("fixtures/cavity_v0.json")).unwrap()
}

#[test]
fn canonical_hash_ignores_key_order_whitespace_and_reparse() {
    let input = study();
    let expected = canonical_hash(&resolve(&input).unwrap());
    let value = serde_json::to_value(&input).unwrap();
    for json in [
        serde_json::to_string(&value).unwrap(),
        serde_json::to_string_pretty(&value).unwrap(),
        serde_json::to_string_pretty(&input).unwrap(),
    ] {
        let parsed = Study::from_json(&json).unwrap();
        assert_eq!(canonical_hash(&resolve(&parsed).unwrap()), expected);
    }
    assert_eq!(expected.len(), 64);
    assert!(
        expected
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    );
}

#[test]
fn equivalent_units_have_identical_hashes() {
    let mut input = study();
    let expected = canonical_hash(&resolve(&input).unwrap());
    input.domain.length_x.value = 100.0;
    input.domain.length_x.unit = Unit::Millimetre;
    assert_eq!(canonical_hash(&resolve(&input).unwrap()), expected);
}

#[test]
fn every_physical_mutation_changes_the_hash() {
    let original = study();
    let expected = canonical_hash(&resolve(&original).unwrap());
    for mutate in 0..7 {
        let mut input = original.clone();
        match mutate {
            0 => input.boundary_conditions.lid_velocity.value *= 2.0,
            1 => input.fluid.kinematic_viscosity.value *= 2.0,
            2 => input.mesh.cells_x += 1,
            3 => input.domain.length_y.value *= 2.0,
            4 => input.solver.end_time.value *= 2.0,
            5 => input.monitors.centreline_u_sample_count += 1,
            _ => input.solver.convergence.p *= 2.0,
        }
        assert_ne!(
            canonical_hash(&resolve(&input).unwrap()),
            expected,
            "mutation {mutate}"
        );
    }
}

#[test]
fn ui_camera_and_view_cannot_enter_the_recipe() {
    let schema =
        serde_json::to_string(&schemars::schema_for!(cfdcraft_model::ResolvedStudy)).unwrap();
    for excluded in ["camera", "view", "ui_state"] {
        assert!(!schema.contains(&format!("\"{excluded}\"")));
    }
    let mut wrapper = serde_json::json!({"study":study(),"ui_state":{"camera":[1,2,3]}});
    let before: Study = serde_json::from_value(wrapper["study"].clone()).unwrap();
    wrapper["ui_state"]["camera"] = serde_json::json!([4, 5, 6]);
    let after: Study = serde_json::from_value(wrapper["study"].clone()).unwrap();
    assert_eq!(
        canonical_hash(&resolve(&before).unwrap()),
        canonical_hash(&resolve(&after).unwrap())
    );
}

#[test]
fn canonical_json_normalises_negative_zero() {
    assert_eq!(serde_jcs::to_string(&-0.0_f64).unwrap(), "0");
    assert_eq!(serde_jcs::to_string(&1.0_f64).unwrap(), "1");
}
