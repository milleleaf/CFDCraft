use cfdcraft_migrate::{
    CURRENT_SCHEMA, MigrateError, Parts, STEPS, Step, check_readable, migrate, migrate_with,
};
use cfdcraft_model::{MigrationNote, Study};
use serde_json::{Value, json};

const V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn cavity_v0_migrates_to_the_current_schema_with_two_notes() {
    let parts = Parts {
        study: json_of(V0),
        artifacts: None,
    };
    let (out, notes) = migrate(parts, 0).unwrap();
    // Compare as typed studies: the number 5000.0 and 5000 are different JSON values but the
    // same quantity.
    let migrated: Study = serde_json::from_value(out.study).unwrap();
    let expected: Study = serde_json::from_str(V1).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(out.artifacts, None);
    assert_eq!(notes.len(), 2);
    assert_eq!((notes[0].from, notes[0].to), (0, 1));
    assert!(notes[0].summary.contains("lid") && notes[0].summary.contains("walls"));
    assert_eq!((notes[1].from, notes[1].to), (1, 2));
}

#[test]
fn a_schema_1_file_gets_an_empty_runs_list_and_one_note() {
    let parts = Parts {
        study: json_of(V1),
        artifacts: Some(json!({"blobs": [], "links": []})),
    };
    let (out, notes) = migrate(parts.clone(), 1).unwrap();
    assert_eq!(out.study, parts.study);
    assert_eq!(
        out.artifacts,
        Some(json!({"blobs": [], "links": [], "runs": []}))
    );
    assert_eq!(notes.len(), 1);
    assert_eq!((notes[0].from, notes[0].to), (1, 2));
    assert!(notes[0].summary.contains("runs"));
}

#[test]
fn a_schema_1_file_without_an_artifact_index_stays_without_one() {
    let parts = Parts {
        study: json_of(V1),
        artifacts: None,
    };
    let (out, notes) = migrate(parts, 1).unwrap();
    assert_eq!(out.artifacts, None);
    assert_eq!(notes.len(), 1);
    assert_eq!((notes[0].from, notes[0].to), (1, 2));
}

#[test]
fn an_index_that_already_has_runs_is_left_alone() {
    let artifacts = json!({"blobs": [], "links": [], "runs": [{"id": "run-1"}]});
    let parts = Parts {
        study: json_of(V1),
        artifacts: Some(artifacts.clone()),
    };
    let (out, _) = migrate(parts, 1).unwrap();
    assert_eq!(out.artifacts, Some(artifacts));
}

#[test]
fn an_index_that_is_not_an_object_is_left_for_the_typed_read_to_refuse() {
    let parts = Parts {
        study: json_of(V1),
        artifacts: Some(json!(7)),
    };
    let (out, notes) = migrate(parts, 1).unwrap();
    assert_eq!(out.artifacts, Some(json!(7)));
    assert_eq!(notes.len(), 1);
}

#[test]
fn current_schema_is_left_alone() {
    let parts = Parts {
        study: json_of(V1),
        artifacts: Some(json!({"items": []})),
    };
    let (out, notes) = migrate(parts.clone(), CURRENT_SCHEMA).unwrap();
    assert_eq!(out, parts);
    assert!(notes.is_empty());
}

#[test]
fn a_current_study_keeps_its_own_number() {
    let study = Study::from_json_with_note(V1).unwrap().0;
    assert_eq!(study.schema_version, Study::SCHEMA_VERSION);
    assert_eq!(Study::SCHEMA_VERSION, 1);
    assert_eq!(CURRENT_SCHEMA, 2);
}

#[test]
fn malformed_old_study_is_a_typed_step_error() {
    let mut extra = json_of(V0);
    extra["surprise"] = json!(1);
    let mut wrong_type = json_of(V0);
    wrong_type["fluid"] = json!("water");
    for study in [extra, wrong_type] {
        let err = migrate(
            Parts {
                study,
                artifacts: None,
            },
            0,
        )
        .unwrap_err();
        match err {
            MigrateError::Step { from, to, reason } => {
                assert_eq!((from, to), (0, 1));
                assert!(!reason.is_empty());
            }
            other => panic!("expected a step error, got {other:?}"),
        }
    }
}

#[test]
fn newer_schema_is_refused_by_number_alone() {
    assert_eq!(
        check_readable(CURRENT_SCHEMA + 1),
        Err(MigrateError::NewerSchema {
            found: CURRENT_SCHEMA + 1,
            supported: CURRENT_SCHEMA
        })
    );
    assert!(check_readable(0).is_ok());
    assert!(check_readable(CURRENT_SCHEMA).is_ok());
    let garbage = Parts {
        study: json!("garbage"),
        artifacts: Some(json!(7)),
    };
    assert!(matches!(
        migrate(garbage, CURRENT_SCHEMA + 1),
        Err(MigrateError::NewerSchema { .. })
    ));
}

fn push(mut parts: Parts, n: u32) -> Result<(Parts, MigrationNote), MigrateError> {
    parts.study.as_array_mut().unwrap().push(json!(n));
    Ok((
        parts,
        MigrationNote {
            from: n,
            to: n + 1,
            summary: format!("step {n}"),
        },
    ))
}
fn s1(p: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    push(p, 1)
}
fn s2(p: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    push(p, 2)
}
fn s3(p: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    push(p, 3)
}
fn failing(_: Parts) -> Result<(Parts, MigrationNote), MigrateError> {
    Err(MigrateError::Step {
        from: 2,
        to: 3,
        reason: "boom".to_owned(),
    })
}

fn table() -> [Step; 3] {
    [
        Step { from: 1, apply: s1 },
        Step { from: 2, apply: s2 },
        Step { from: 3, apply: s3 },
    ]
}

fn empty() -> Parts {
    Parts {
        study: json!([]),
        artifacts: None,
    }
}

#[test]
fn synthetic_chain_runs_in_order() {
    let t = table();
    let (out, notes) = migrate_with(&t, 4, empty(), 1).unwrap();
    assert_eq!(out.study, json!([1, 2, 3]));
    let summaries: Vec<_> = notes.iter().map(|n| n.summary.as_str()).collect();
    assert_eq!(summaries, ["step 1", "step 2", "step 3"]);

    let (out, notes) = migrate_with(&t, 4, empty(), 3).unwrap();
    assert_eq!(out.study, json!([3]));
    assert_eq!(notes.len(), 1);

    let (out, notes) = migrate_with(&t, 4, empty(), 4).unwrap();
    assert_eq!(out, empty());
    assert!(notes.is_empty());

    assert!(matches!(
        migrate_with(&t, 4, empty(), 5),
        Err(MigrateError::NewerSchema {
            found: 5,
            supported: 4
        })
    ));
}

#[test]
fn a_gap_is_reported_before_any_step_runs() {
    let t = [Step { from: 1, apply: s1 }, Step { from: 3, apply: s3 }];
    // Had step 1 run, it would have changed the study; the error carries no parts either way,
    // so check the reason names the missing version.
    match migrate_with(&t, 4, empty(), 1).unwrap_err() {
        MigrateError::Step { from, reason, .. } => {
            assert_eq!(from, 2);
            assert!(reason.contains('2'));
        }
        other => panic!("expected a step error, got {other:?}"),
    }
}

#[test]
fn a_failing_step_stops_the_chain() {
    let t = [
        Step { from: 1, apply: s1 },
        Step {
            from: 2,
            apply: failing,
        },
        Step { from: 3, apply: s3 },
    ];
    match migrate_with(&t, 4, empty(), 1).unwrap_err() {
        MigrateError::Step { from, to, reason } => {
            assert_eq!((from, to), (2, 3));
            assert_eq!(reason, "boom");
        }
        other => panic!("expected a step error, got {other:?}"),
    }
}

#[test]
fn real_table_is_contiguous_and_reaches_current() {
    assert!(!STEPS.is_empty());
    for (i, step) in STEPS.iter().enumerate() {
        assert_eq!(step.from as usize, i);
    }
    assert_eq!(STEPS.last().unwrap().from + 1, CURRENT_SCHEMA);
}
