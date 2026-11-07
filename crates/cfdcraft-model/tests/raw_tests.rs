use cfdcraft_model::{RawBackend, RawOverride, Study, canonical_hash, resolve};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");

fn relax_u(value: &str) -> Value {
    json!({"relax-u": {
        "backend": "esi",
        "file": "system/fvSolution",
        "entry": "relaxationFactors/equations/U",
        "value": value,
    }})
}

fn cavity_with(overrides: Option<Value>) -> Study {
    let mut value: Value = serde_json::from_str(CAVITY).unwrap();
    if let Some(overrides) = overrides {
        value["raw_overrides"] = overrides;
    }
    Study::from_json(&value.to_string()).unwrap()
}

fn hash(study: &Study) -> String {
    canonical_hash(&resolve(study).unwrap())
}

fn over(file: &str) -> RawOverride {
    RawOverride {
        backend: RawBackend::Esi,
        file: file.to_owned(),
        entry: "a".to_owned(),
        value: "1".to_owned(),
    }
}

#[test]
fn a_study_without_raw_overrides_serialises_as_before() {
    let study = cavity_with(None);
    assert!(
        !serde_json::to_string(&study)
            .unwrap()
            .contains("raw_overrides")
    );
    let resolved = resolve(&study).unwrap();
    assert!(
        !serde_jcs::to_string(&resolved)
            .unwrap()
            .contains("raw_overrides")
    );
    assert!(resolved.raw_overrides.is_empty());
}

#[test]
fn a_study_with_a_raw_override_round_trips() {
    let study = cavity_with(Some(relax_u("0.5")));
    assert_eq!(study.raw_overrides.len(), 1);
    let text = serde_json::to_string(&study).unwrap();
    assert!(text.contains("raw_overrides") && text.contains("relaxationFactors/equations/U"));
    assert_eq!(Study::from_json(&text).unwrap(), study);
    let pretty = serde_json::to_string_pretty(&study).unwrap();
    assert_eq!(Study::from_json(&pretty).unwrap(), study);
}

#[test]
fn a_raw_override_changes_the_study_hash() {
    let plain = hash(&cavity_with(None));
    let half = hash(&cavity_with(Some(relax_u("0.5"))));
    let again = hash(&cavity_with(Some(relax_u("0.5"))));
    let other = hash(&cavity_with(Some(relax_u("0.6"))));
    assert_ne!(plain, half);
    assert_eq!(half, again);
    assert_ne!(half, other);
}

#[test]
fn the_resolved_study_carries_the_raw_overrides() {
    let study = cavity_with(Some(relax_u("0.5")));
    let resolved = resolve(&study).unwrap();
    assert_eq!(resolved.raw_overrides, study.raw_overrides);
    assert!(
        serde_jcs::to_string(&resolved)
            .unwrap()
            .contains("raw_overrides")
    );
}

#[test]
fn which_files_affect_the_mesh() {
    for file in [
        "system/blockMeshDict",
        "system/snappyHexMeshDict",
        "system/meshQualityDict",
        "system/surfaceFeaturesDict",
        "constant/polyMesh/boundary",
        "constant/polyMesh/sets/x",
    ] {
        assert!(over(file).affects_mesh(), "{file}");
    }
    for file in [
        "system/fvSolution",
        "system/controlDict",
        "system/fvSchemes",
        "0/U",
        "constant/transportProperties",
    ] {
        assert!(!over(file).affects_mesh(), "{file}");
    }
}

#[test]
fn an_unknown_member_of_a_raw_override_is_refused() {
    let mut overrides = relax_u("0.5");
    overrides["relax-u"]["extra"] = json!(1);
    let mut value: Value = serde_json::from_str(CAVITY).unwrap();
    value["raw_overrides"] = overrides;
    let error = Study::from_json(&value.to_string()).unwrap_err();
    assert!(error.to_string().contains("unknown field"), "{error}");
}

#[test]
fn the_backend_tag_is_lower_case_text() {
    for (text, backend) in [
        ("esi", RawBackend::Esi),
        ("foundation", RawBackend::Foundation),
    ] {
        let read: RawBackend = serde_json::from_value(json!(text)).unwrap();
        assert_eq!(read, backend);
        assert_eq!(backend.as_str(), text);
        assert_eq!(backend.to_string(), text);
    }
    for bad in ["Esi", "openfoam"] {
        assert!(
            serde_json::from_value::<RawBackend>(json!(bad)).is_err(),
            "{bad}"
        );
    }
}
