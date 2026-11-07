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

/// Each kind of member with the number its schema file carries: the study its own number, the
/// manifest and the artifact index the number of the whole file.
const KINDS: [(&str, u32); 3] = [
    ("study", Study::SCHEMA_VERSION),
    ("manifest", CURRENT_SCHEMA),
    ("artifacts", CURRENT_SCHEMA),
];

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
fn the_study_schema_is_still_the_published_v1_file() {
    let published = fs::read_to_string(schema_dir().join("cfdcraft-study-v1.json")).unwrap();
    assert_eq!(
        to_file_text(&study_schema()),
        published,
        "the study schema changed, but the study keeps its own number"
    );
    let schema = study_schema();
    assert_eq!(schema["$id"], "urn:cfdcraft:schema:study:1");
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        json!(Study::SCHEMA_VERSION)
    );

    let mut other = cavity();
    other["schema_version"] = json!(Study::SCHEMA_VERSION + 1);
    assert!(validate(&schema, &other).is_err());
    assert!(Study::from_json(&other.to_string()).is_err());

    let mut current = cavity();
    current["schema_version"] = json!(Study::SCHEMA_VERSION);
    assert_eq!(validate(&schema, &current), Ok(()));
    assert!(Study::from_json(&current.to_string()).is_ok());
}

#[test]
fn the_manifest_pins_the_file_number() {
    let schema = manifest_schema();
    assert_eq!(
        schema["$id"],
        format!("urn:cfdcraft:schema:manifest:{CURRENT_SCHEMA}")
    );
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        json!(CURRENT_SCHEMA)
    );
    let manifest = |number: u32| {
        json!({
            "format": FORMAT_ID,
            "schema_version": number,
            "writer": { "name": "example-authoring-tool", "version": "0.0.1" },
        })
    };
    assert_eq!(validate(&schema, &manifest(CURRENT_SCHEMA)), Ok(()));
    assert!(validate(&schema, &manifest(CURRENT_SCHEMA - 1)).is_err());
    assert!(validate(&schema, &manifest(CURRENT_SCHEMA + 1)).is_err());
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
    for (name, version, schema) in [
        ("study", Study::SCHEMA_VERSION, study_schema()),
        ("manifest", CURRENT_SCHEMA, manifest_schema()),
        ("artifacts", CURRENT_SCHEMA, artifacts_schema()),
    ] {
        assert_eq!(schema["$schema"], META_SCHEMA, "{name}");
        assert_eq!(
            schema["$id"],
            format!("urn:cfdcraft:schema:{name}:{version}"),
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
        json!(Study::SCHEMA_VERSION)
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
fn published_names_carry_each_kinds_own_version_and_exist() {
    let wanted: Vec<String> = KINDS
        .iter()
        .map(|(kind, version)| format!("cfdcraft-{kind}-v{version}.json"))
        .collect();
    let files: Vec<String> = published().into_iter().map(|(file, _)| file).collect();
    assert_eq!(files, wanted);
    for file in files {
        assert!(
            schema_dir().join(&file).is_file(),
            "schema/{file} is missing"
        );
    }
}

#[test]
fn every_version_has_its_files_and_older_ones_are_frozen() {
    let frozen_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frozen");
    for (kind, current) in KINDS {
        for version in 1..=current {
            let file = format!("cfdcraft-{kind}-v{version}.json");
            let on_disk = schema_dir().join(&file);
            assert!(on_disk.is_file(), "schema/{file} is missing");
            if version < current {
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

/// The names of the properties of a definition or of the root, sorted.
fn property_names(schema: &Value) -> Vec<&str> {
    schema["properties"]
        .as_object()
        .unwrap_or_else(|| panic!("no properties in {schema}"))
        .keys()
        .map(String::as_str)
        .collect()
}

#[test]
fn the_artifact_index_schema_describes_a_run_record_completely() {
    let schema = artifacts_schema();
    assert_eq!(schema["properties"]["runs"]["type"], "array");
    assert_eq!(
        schema["properties"]["runs"]["items"]["$ref"],
        "#/$defs/RunRecord"
    );
    assert!(
        schema.get("required").is_none_or(|required| {
            !required
                .as_array()
                .is_some_and(|names| names.contains(&json!("runs")))
        }),
        "a file without runs must stay valid"
    );

    let defs = &schema["$defs"];
    assert_eq!(
        property_names(&defs["RunRecord"]),
        [
            "checks",
            "comparison",
            "id",
            "key_numbers",
            "mesh",
            "provenance",
            "rerun_of",
            "result"
        ]
    );
    assert_eq!(
        property_names(&defs["Provenance"]),
        [
            "geometry_hash",
            "host",
            "mesh_hash",
            "mesh_key",
            "result_key",
            "settings",
            "tool"
        ]
    );
    assert_eq!(
        property_names(&defs["ToolIdentity"]),
        ["dialect", "image_digest", "sidecar_lock", "writer_version"]
    );
}

#[test]
fn a_schema_2_study_file_without_runs_validates() {
    let schema = artifacts_schema();
    for index in [
        json!({}),
        json!({ "blobs": [], "links": [] }),
        json!({ "blobs": [], "links": [], "runs": [] }),
    ] {
        assert_eq!(validate(&schema, &index), Ok(()), "{index}");
    }
    assert!(validate(&schema, &json!({ "runs": {} })).is_err());
}
