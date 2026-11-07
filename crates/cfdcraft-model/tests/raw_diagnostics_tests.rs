use cfdcraft_model::diagnostics::{ALL_KINDS, CODES, Diagnostic, is_known_code};
use cfdcraft_model::{Capabilities, RawBackend, ResolveError, Severity, Study, diagnose, resolve};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");

fn study_with(overrides: Value) -> Study {
    let mut value: Value = serde_json::from_str(CAVITY).unwrap();
    value["raw_overrides"] = overrides;
    Study::from_json(&value.to_string()).unwrap()
}

fn one(label: &str, file: &str, entry: &str, value: &str) -> Study {
    study_with(json!({label: {
        "backend": "esi", "file": file, "entry": entry, "value": value,
    }}))
}

fn good(file: &str, entry: &str, value: &str) -> Study {
    one("relax-u", file, entry, value)
}

fn esi() -> Capabilities {
    Capabilities {
        raw_backend: Some(RawBackend::Esi),
        ..Capabilities::unrestricted()
    }
}

fn writer_limits() -> Capabilities {
    Capabilities {
        boundary_kinds: ALL_KINDS.to_vec(),
        requires_single_cell_z: true,
        same_kind_boundaries_must_agree: true,
        raw_backend: Some(RawBackend::Esi),
    }
}

fn with_code(study: &Study, capabilities: &Capabilities, code: &str) -> Vec<Diagnostic> {
    diagnose(study, capabilities)
        .into_iter()
        .filter(|d| d.code == code)
        .collect()
}

/// Every text in `bad` gives exactly one finding of `code` at `pointer`; every text in `fine`
/// gives none. `make` builds the study from one text.
fn check(code: &str, pointer: &str, bad: &[&str], fine: &[&str], make: impl Fn(&str) -> Study) {
    for text in bad {
        let found = with_code(&make(text), &Capabilities::unrestricted(), code);
        assert_eq!(found.len(), 1, "{text:?}: {found:?}");
        assert_eq!(found[0].pointer, pointer, "{text:?}");
        assert_eq!(found[0].severity, Severity::Error);
        assert!(found[0].hint.is_some());
    }
    for text in fine {
        let found = with_code(&make(text), &Capabilities::unrestricted(), code);
        assert!(found.is_empty(), "{text:?}: {found:?}");
    }
}

#[test]
fn a_valid_raw_override_has_no_findings() {
    let study = good("system/fvSolution", "relaxationFactors/equations/U", "0.5");
    assert!(diagnose(&study, &Capabilities::unrestricted()).is_empty());
    assert!(diagnose(&study, &esi()).is_empty());
    assert!(diagnose(&study, &writer_limits()).is_empty());
}

#[test]
fn a_bad_label_is_cfd_raw_001() {
    let long = "a".repeat(33);
    for label in ["1x", "", long.as_str()] {
        let study = one(label, "system/fvSolution", "a", "1");
        let found = with_code(&study, &Capabilities::unrestricted(), "CFD-RAW-001");
        assert_eq!(found.len(), 1, "{label:?}: {found:?}");
        assert_eq!(found[0].pointer, format!("/raw_overrides/{label}"));
    }
    let thirty_two = "a".repeat(32);
    for label in ["relax-u", "a", thirty_two.as_str()] {
        let study = one(label, "system/fvSolution", "a", "1");
        assert!(with_code(&study, &Capabilities::unrestricted(), "CFD-RAW-001").is_empty());
    }
}

#[test]
fn a_file_outside_the_case_is_cfd_raw_002() {
    check(
        "CFD-RAW-002",
        "/raw_overrides/relax-u/file",
        &[
            "../x",
            "/etc/passwd",
            "system/../x",
            "other/fvSolution",
            "system",
            "system/a/b/c",
            "system/a b",
            "",
            "system\\fvSolution",
            "system/..",
            "system/.",
            "system//fvSolution",
        ],
        &["system/fvSolution", "constant/polyMesh/boundary", "0/U"],
        |file| good(file, "a", "1"),
    );
}

#[test]
fn a_bad_entry_is_cfd_raw_003() {
    check(
        "CFD-RAW-003",
        "/raw_overrides/relax-u/entry",
        &[
            "",
            "a//b",
            "/a",
            "a/",
            "a/b/c/d/e/f/g",
            "1a",
            "a/2b",
            "a.b",
            "a\"b",
        ],
        &["relaxationFactors/equations/U", "a", "a/b/c/d/e/f"],
        |entry| good("system/fvSolution", entry, "1"),
    );
}

#[test]
fn a_bad_value_is_cfd_raw_004() {
    let long = "1".repeat(201);
    let ok_long = "1".repeat(200);
    let bad = [
        "",
        long.as_str(),
        "1;",
        "{",
        "}",
        "#include x",
        "$x",
        "\"x\"",
        "a\\b",
        "1\n2",
        "1\t2",
        "é",
    ];
    check(
        "CFD-RAW-004",
        "/raw_overrides/relax-u/value",
        &bad,
        &[
            "0.5",
            "(1 0 0)",
            "uniform (0 0 0)",
            "Gauss linear",
            ok_long.as_str(),
        ],
        |value| good("system/fvSolution", "a", value),
    );
}

#[test]
fn a_raw_override_tagged_for_another_backend_is_cfd_raw_005() {
    let foundation = study_with(json!({"relax-u": {
        "backend": "foundation", "file": "system/fvSolution", "entry": "a", "value": "1",
    }}));
    let found = with_code(&foundation, &esi(), "CFD-RAW-005");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].pointer, "/raw_overrides/relax-u/backend");
    assert!(with_code(&foundation, &writer_limits(), "CFD-RAW-005").len() == 1);
    assert!(with_code(&foundation, &Capabilities::unrestricted(), "CFD-RAW-005").is_empty());
    let own = good("system/fvSolution", "a", "1");
    assert!(with_code(&own, &esi(), "CFD-RAW-005").is_empty());
    assert!(with_code(&own, &Capabilities::unrestricted(), "CFD-RAW-005").is_empty());
}

fn many(count: usize) -> Study {
    let mut map = serde_json::Map::new();
    for i in 0..count {
        map.insert(
            format!("o{i:02}"),
            json!({"backend": "esi", "file": "system/fvSolution", "entry": "a", "value": "1"}),
        );
    }
    study_with(Value::Object(map))
}

#[test]
fn more_than_fifty_raw_overrides_is_cfd_raw_006() {
    assert!(diagnose(&many(50), &Capabilities::unrestricted()).is_empty());
    let found = diagnose(&many(51), &Capabilities::unrestricted());
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, "CFD-RAW-006");
    assert_eq!(found[0].pointer, "/raw_overrides");
}

#[test]
fn resolve_refuses_an_invalid_raw_override() {
    let study = good("system/fvSolution", "a", "1; #include x");
    match resolve(&study) {
        Err(ResolveError::Diagnostics(found)) => {
            assert!(found.iter().any(|d| d.code == "CFD-RAW-004"), "{found:?}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_new_codes_are_listed() {
    for n in 1..=7 {
        assert!(is_known_code(&format!("CFD-RAW-00{n}")), "{n}");
    }
    assert!(!is_known_code("CFD-RAW-008"));
    assert_eq!(CODES.len(), 71);
}
