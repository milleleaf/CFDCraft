use cfdcraft_model::{
    BoundaryCondition, BoundaryKind, Face, ResolveError, Study, StudyError, canonical_hash,
    diagnostics::pointer_token, resolve, speed_from_vector, velocity_vector,
};

const V0: &str = include_str!("fixtures/cavity_v0.json");
const V1: &str = include_str!("fixtures/cavity_v1.json");

/// The hash the schema 0 recipe produced for this cavity. Historical: schema 0 documents quote it.
const SCHEMA_0_CAVITY_HASH: &str =
    "c3e9871f50783bf3584ad7d0fccc8f6e75fb974ad085cae9b0a3c2cdb2d57eb6";

fn v1() -> Study {
    Study::from_json(V1).unwrap()
}

fn hash_of(study: &Study) -> String {
    canonical_hash(&resolve(study).unwrap())
}

#[test]
fn schema_0_cavity_migrates_to_the_schema_1_cavity_with_a_note() {
    let (migrated, note) = Study::from_json_with_note(V0).unwrap();
    let note = note.expect("a schema 0 document yields a migration note");
    assert_eq!((note.from, note.to), (0, 1));
    assert!(note.summary.contains("lid") && note.summary.contains("walls"));
    let (direct, none) = Study::from_json_with_note(V1).unwrap();
    assert!(none.is_none());
    assert_eq!(
        serde_json::to_value(&migrated).unwrap(),
        serde_json::to_value(&direct).unwrap()
    );
}

#[test]
fn migrated_cavity_resolves_to_the_documented_boundaries() {
    let resolved = resolve(&Study::from_json(V0).unwrap()).unwrap();
    assert_eq!(resolved.schema_version, 1);
    assert!(!resolved.domain.open);
    assert_eq!(resolved.fluid.kinematic_viscosity, 0.0001);
    let lid = &resolved.boundary_conditions["lid"];
    assert_eq!(lid.faces, [Face::YMax]);
    assert_eq!(lid.kind, BoundaryKind::MovingWall);
    assert_eq!(lid.velocity, Some(0.1));
    let walls = &resolved.boundary_conditions["walls"];
    assert_eq!(walls.faces, [Face::XMin, Face::XMax, Face::YMin]);
    assert_eq!(walls.kind, BoundaryKind::NoSlipWall);
    assert_eq!(walls.velocity, None);
    assert_eq!(resolved.boundary_conditions.len(), 2);
    assert_eq!(resolved.lid_velocity(), Some(0.1));
}

#[test]
fn schema_0_migration_changes_the_hash_but_not_the_study() {
    let migrated = hash_of(&Study::from_json(V0).unwrap());
    assert_eq!(migrated, hash_of(&v1()));
    assert_ne!(migrated, SCHEMA_0_CAVITY_HASH);
}

#[test]
fn face_order_and_duplicates_do_not_change_the_recipe() {
    let base = v1();
    let mut shuffled = base.clone();
    shuffled.boundary_conditions.get_mut("walls").unwrap().faces =
        vec![Face::YMin, Face::XMax, Face::XMin, Face::XMax];
    assert_eq!(resolve(&base).unwrap(), resolve(&shuffled).unwrap());
    assert_eq!(hash_of(&base), hash_of(&shuffled));
}

/// The (code, pointer) of every error `resolve` reports.
fn problems(study: &Study) -> Vec<(String, String)> {
    match resolve(study) {
        Err(ResolveError::Diagnostics(found)) => {
            found.into_iter().map(|d| (d.code, d.pointer)).collect()
        }
        other => panic!("expected diagnostics, got {other:?}"),
    }
}

fn has(found: &[(String, String)], code: &str, pointer: &str) -> bool {
    found.contains(&(code.to_owned(), pointer.to_owned()))
}

#[test]
fn bad_boundary_names_and_shapes_fail_to_resolve() {
    let long = "a".repeat(33);
    for name in ["a/b", "", "1lid", "a b", "a~b", "a.b", "é", long.as_str()] {
        let mut study = v1();
        let wall = study.boundary_conditions.remove("walls").unwrap();
        study.boundary_conditions.insert(name.to_owned(), wall);
        assert_eq!(
            problems(&study),
            [(
                "CFD-BC-006".to_owned(),
                format!("/boundary_conditions/{}", pointer_token(name))
            )],
            "{name:?}"
        );
    }
    let max = "a".repeat(32);
    let mut study = v1();
    let wall = study.boundary_conditions.remove("walls").unwrap();
    study.boundary_conditions.insert(max, wall);
    resolve(&study).unwrap();

    let mut study = v1();
    study
        .boundary_conditions
        .get_mut("walls")
        .unwrap()
        .faces
        .clear();
    assert!(has(
        &problems(&study),
        "CFD-BC-003",
        "/boundary_conditions/walls/faces"
    ));

    let mut study = v1();
    study.boundary_conditions.get_mut("lid").unwrap().velocity = None;
    assert_eq!(
        problems(&study),
        [(
            "CFD-BC-004".to_owned(),
            "/boundary_conditions/lid/velocity".to_owned()
        )]
    );

    let mut study = v1();
    study.boundary_conditions.get_mut("lid").unwrap().kind = BoundaryKind::VelocityInlet;
    let lid_velocity = study.boundary_conditions["lid"].velocity;
    let walls = study.boundary_conditions.get_mut("walls").unwrap();
    walls.velocity = lid_velocity;
    assert!(has(
        &problems(&study),
        "CFD-BC-005",
        "/boundary_conditions/walls/velocity"
    ));
}

#[test]
fn unsupported_versions_and_a_supplied_outlet_pressure_are_rejected() {
    let input = V1.replace("\"schema_version\": 1", "\"schema_version\": 2");
    let error = Study::from_json(&input).unwrap_err();
    assert!(matches!(
        error,
        StudyError::UnsupportedSchemaVersion { found: 2 }
    ));
    let text = error.to_string();
    assert!(text.contains('2') && text.contains("0") && text.contains("1"));

    let mut value: serde_json::Value = serde_json::from_str(V1).unwrap();
    value["boundary_conditions"]["walls"]["pressure"] =
        serde_json::json!({"value": 5, "unit": "Pa"});
    let error = Study::from_json(&value.to_string())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("pressure") && error.contains("unknown field"),
        "{error}"
    );

    // A direct parse accepts schema 1 only.
    for old in [V0, input.as_str()] {
        let error = serde_json::from_str::<Study>(old).unwrap_err().to_string();
        assert!(error.contains("UnsupportedSchemaVersion"), "{error}");
    }
}

#[test]
fn a_study_with_a_missing_boundary_parses_but_does_not_resolve() {
    let mut study = v1();
    study.boundary_conditions.remove("walls");
    let found = problems(&study);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().all(|(code, _)| code == "CFD-BC-001"));
}

#[test]
fn velocity_vector_and_speed_from_vector_are_inverse() {
    let faces = [Face::XMin, Face::XMax, Face::YMin, Face::YMax];
    for kind in [BoundaryKind::VelocityInlet, BoundaryKind::MovingWall] {
        for face in faces {
            for speed in [0.1, -0.1, 3.5] {
                let vector = velocity_vector(kind, face, speed);
                assert_eq!(speed_from_vector(kind, face, vector), Some(speed));
                for axis in 0..3 {
                    if vector[axis] == 0.0 {
                        let mut off = vector;
                        off[axis] = 1.0;
                        assert_eq!(speed_from_vector(kind, face, off), None);
                    }
                }
            }
        }
    }
    // Kinds without a speed impose a zero vector and carry no speed back.
    for kind in [
        BoundaryKind::PressureOutlet,
        BoundaryKind::NoSlipWall,
        BoundaryKind::SlipWall,
        BoundaryKind::Symmetry,
    ] {
        for face in faces {
            assert_eq!(velocity_vector(kind, face, 0.7), [0.0; 3]);
            assert_eq!(speed_from_vector(kind, face, [0.0; 3]), None);
        }
    }
    assert_eq!(
        velocity_vector(BoundaryKind::MovingWall, Face::YMax, 0.1),
        [0.1, 0.0, 0.0]
    );
    assert_eq!(
        velocity_vector(BoundaryKind::VelocityInlet, Face::XMax, 2.0),
        [-2.0, 0.0, 0.0]
    );
}

#[test]
fn lid_velocity_needs_exactly_one_top_face_moving_wall() {
    let mut study = v1();
    study.boundary_conditions.get_mut("lid").unwrap().faces = vec![Face::YMin];
    study.boundary_conditions.get_mut("walls").unwrap().faces =
        vec![Face::XMin, Face::XMax, Face::YMax];
    assert_eq!(resolve(&study).unwrap().lid_velocity(), None);
    let mut study = v1();
    let lid: BoundaryCondition = study.boundary_conditions["lid"].clone();
    let mut second = lid.clone();
    second.faces = vec![Face::XMin];
    study
        .boundary_conditions
        .insert("second".to_owned(), second);
    study.boundary_conditions.get_mut("walls").unwrap().faces = vec![Face::XMax, Face::YMin];
    assert_eq!(resolve(&study).unwrap().lid_velocity(), None);
}

/// Cross-platform identity check: the macOS, Windows and Linux test jobs must all agree on
/// this literal.
#[test]
fn cavity_v1_hash_is_pinned() {
    assert_eq!(
        hash_of(&v1()),
        "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0"
    );
}
