use std::fs;
use std::path::PathBuf;

use cfdcraft_doc::FORMAT_ID;
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::Study;
use cfdcraft_schema::{
    artifacts_schema, manifest_schema, published, study_schema, to_file_text, validate,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const META_SCHEMA: &str = "https://json-schema.org/draft/2020-12/schema";

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema")
}

fn cavity() -> Value {
    serde_json::from_str(CAVITY).unwrap()
}

#[test]
fn the_cavity_study_validates_and_opens() {
    assert_eq!(validate(&study_schema(), &cavity()), Ok(()));
    Study::from_json(CAVITY).unwrap();
}

#[test]
fn schema_version_is_pinned_to_the_current_number() {
    let mut other = cavity();
    other["schema_version"] = json!(CURRENT_SCHEMA + 1);
    assert!(validate(&study_schema(), &other).is_err());
    assert!(Study::from_json(&other.to_string()).is_err());

    let mut current = cavity();
    current["schema_version"] = json!(CURRENT_SCHEMA);
    assert_eq!(validate(&study_schema(), &current), Ok(()));
    assert!(Study::from_json(&current.to_string()).is_ok());
}

/// Collect every string that holds a web address, except the root `$schema` value.
fn addresses(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::String(text) if text.contains("http://") || text.contains("https://") => {
            found.push(format!("{path}: {text}"));
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                addresses(item, &format!("{path}/{index}"), found);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                if path.is_empty() && key == "$schema" {
                    continue;
                }
                addresses(item, &format!("{path}/{key}"), found);
            }
        }
        _ => {}
    }
}

#[test]
fn schemas_have_an_identity_a_closed_shape_and_no_network_references() {
    for (name, schema) in [
        ("study", study_schema()),
        ("manifest", manifest_schema()),
        ("artifacts", artifacts_schema()),
    ] {
        assert_eq!(schema["$schema"], META_SCHEMA, "{name}");
        assert_eq!(
            schema["$id"],
            format!("urn:cfdcraft:schema:{name}:{CURRENT_SCHEMA}"),
            "{name}"
        );
        assert_eq!(schema["additionalProperties"], json!(false), "{name}");
        let mut found = Vec::new();
        addresses(&schema, "", &mut found);
        assert!(found.is_empty(), "{name} holds web addresses: {found:?}");
    }
    let manifest = manifest_schema();
    assert_eq!(manifest["properties"]["format"]["const"], FORMAT_ID);
    assert_eq!(
        manifest["properties"]["schema_version"]["const"],
        json!(CURRENT_SCHEMA)
    );
    assert_eq!(
        study_schema()["properties"]["schema_version"]["const"],
        json!(CURRENT_SCHEMA)
    );
}

/// Regenerating is an explicit opt-in: the files change only when this variable is `1`.
#[test]
fn checked_in_schema_files_equal_what_the_types_generate() {
    let update = std::env::var("CFDCRAFT_UPDATE_SCHEMA").is_ok_and(|value| value == "1");
    let mut stale = Vec::new();
    for (file, generate) in published() {
        let wanted = to_file_text(&generate());
        let path = schema_dir().join(&file);
        if update {
            fs::create_dir_all(schema_dir()).unwrap();
            fs::write(path.as_path(), wanted.as_bytes()).unwrap();
        } else if fs::read_to_string(path.as_path()).ok().as_deref() != Some(wanted.as_str()) {
            stale.push(format!("---- schema/{file} should read:\n{wanted}"));
        }
    }
    assert!(
        stale.is_empty(),
        "checked-in schema files differ from what the types generate. Regenerate them with \
         `CFDCRAFT_UPDATE_SCHEMA=1 cargo test -p cfdcraft-schema --test schema_tests`, then review \
         the change. The text that would be written follows.\n{}",
        stale.join("\n")
    );
}

#[test]
fn published_names_carry_the_current_version_and_exist() {
    let suffix = format!("-v{CURRENT_SCHEMA}.json");
    let files = published();
    assert_eq!(files.len(), 3);
    for (file, _) in files {
        assert!(
            file.starts_with("cfdcraft-") && file.ends_with(&suffix),
            "{file}"
        );
        assert!(
            schema_dir().join(&file).is_file(),
            "schema/{file} is missing"
        );
    }
}

#[test]
fn every_version_has_its_files_and_older_ones_are_frozen() {
    let frozen_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frozen");
    for version in 1..=CURRENT_SCHEMA {
        for kind in ["study", "manifest", "artifacts"] {
            let file = format!("cfdcraft-{kind}-v{version}.json");
            let on_disk = schema_dir().join(&file);
            assert!(on_disk.is_file(), "schema/{file} is missing");
            if version < CURRENT_SCHEMA {
                // A published version is never edited: its text is held in a frozen copy, and a
                // version bump cannot pass without adding the copies.
                let frozen = fs::read_to_string(frozen_dir.join(&file)).unwrap_or_else(|_| {
                    panic!("tests/fixtures/frozen/{file} is missing: freeze the published text")
                });
                assert_eq!(
                    fs::read_to_string(on_disk).unwrap(),
                    frozen,
                    "schema/{file} was edited after it was published"
                );
            }
        }
    }
}

#[test]
fn the_readme_names_every_published_file() {
    let readme = fs::read_to_string(schema_dir().join("README.md")).unwrap();
    for (file, _) in published() {
        assert!(
            readme.contains(&file),
            "schema/README.md does not mention {file}"
        );
    }
}
