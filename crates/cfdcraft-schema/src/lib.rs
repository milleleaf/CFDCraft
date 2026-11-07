//! The published JSON Schema files for the three JSON members of a study file.
//!
//! Each file is named after the number of the thing it describes: the manifest and the artifact
//! index carry the number of the file, the study carries its own.
//!
//! The schemas are generated from the Rust types the reader uses, so they cannot drift silently
//! from what the reader accepts. A test compares the checked-in files in the repository's `schema/`
//! folder with the generated text; regenerating them is an explicit opt-in.
//!
//! Validation compiles a schema from its value alone. The schemas hold no network references and
//! the validator is built without a resolver, so nothing here can reach the network or the disk.

use cfdcraft_doc::{ArtifactIndex, FORMAT_ID, Manifest};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::Study;
use schemars::{JsonSchema, SchemaGenerator};
use serde_json::Value;

fn generated<T: JsonSchema>() -> Value {
    SchemaGenerator::default()
        .into_root_schema_for::<T>()
        .to_value()
}

/// Give a generated schema its URN identity and title, and pin values the generator cannot see.
///
/// The reader checks the schema number and the format name with code of its own, so the generated
/// schema only says "integer" and "string" for them. A file with schema number 2 would pass the
/// generated schema and then be refused by the reader. Each pinned field gets a constant instead.
/// The identity is a URN so that the schema never claims to be hosted anywhere.
fn overlay(
    mut schema: Value,
    name: &str,
    member: &str,
    version: u32,
    pinned: &[(&str, Value)],
) -> Value {
    let root = schema
        .as_object_mut()
        .expect("the schema of a record is an object");
    root.insert(
        "$id".to_owned(),
        format!("urn:cfdcraft:schema:{name}:{version}").into(),
    );
    root.insert(
        "title".to_owned(),
        format!("CFDCraft {member}, schema {version}").into(),
    );
    let properties = root
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("the schema of a record lists its properties");
    for (field, constant) in pinned {
        properties
            .get_mut(*field)
            .and_then(Value::as_object_mut)
            .expect("a pinned field is a property of the record")
            .insert("const".to_owned(), constant.clone());
    }
    schema
}

/// The schema of `study.json`.
pub fn study_schema() -> Value {
    overlay(
        generated::<Study>(),
        "study",
        "study.json",
        Study::SCHEMA_VERSION,
        &[("schema_version", Study::SCHEMA_VERSION.into())],
    )
}

/// The schema of `manifest.json`.
pub fn manifest_schema() -> Value {
    overlay(
        generated::<Manifest>(),
        "manifest",
        "manifest.json",
        CURRENT_SCHEMA,
        &[
            ("schema_version", CURRENT_SCHEMA.into()),
            ("format", FORMAT_ID.into()),
        ],
    )
}

/// The schema of `artifacts.json`.
pub fn artifacts_schema() -> Value {
    overlay(
        generated::<ArtifactIndex>(),
        "artifacts",
        "artifacts.json",
        CURRENT_SCHEMA,
        &[],
    )
}

/// The files published in the `schema/` folder: each file name with the function that generates
/// its content. A name carries the number of what the file describes: the study's own number for
/// the study, the number of the whole file for the manifest and the artifact index.
pub fn published() -> Vec<(String, fn() -> Value)> {
    vec![
        (
            format!("cfdcraft-study-v{}.json", Study::SCHEMA_VERSION),
            study_schema as fn() -> Value,
        ),
        (
            format!("cfdcraft-manifest-v{CURRENT_SCHEMA}.json"),
            manifest_schema as fn() -> Value,
        ),
        (
            format!("cfdcraft-artifacts-v{CURRENT_SCHEMA}.json"),
            artifacts_schema as fn() -> Value,
        ),
    ]
}

/// The text of a schema file: pretty JSON and a final newline. Used for the files on disk and for
/// the comparison against them.
pub fn to_file_text(schema: &Value) -> String {
    let mut text = serde_json::to_string_pretty(schema).expect("a JSON value always serialises");
    text.push('\n');
    text
}

/// Check `instance` against `schema`. Every problem comes back as the path of the offending part of
/// the instance (for example `/mesh/cells_x`) and a message.
pub fn validate(schema: &Value, instance: &Value) -> Result<(), Vec<String>> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| vec![format!("the schema does not compile: {error}")])?;
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|error| {
            let path = error.instance_path().to_string();
            let path = if path.is_empty() {
                "(document)"
            } else {
                path.as_str()
            };
            format!("{path}: {error}")
        })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
