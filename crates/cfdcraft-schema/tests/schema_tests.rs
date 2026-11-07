use std::fs;
use std::path::PathBuf;

use cfdcraft_doc::{ArtifactIndex, FORMAT_ID};
use cfdcraft_migrate::CURRENT_SCHEMA;
use cfdcraft_model::Study;
use cfdcraft_queue::{QUEUE_SCHEMA, Queue};
use cfdcraft_schema::{
    artifacts_schema, manifest_schema, published, queue_schema, study_schema, to_file_text,
    validate,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const META_SCHEMA: &str = "https://json-schema.org/draft/2020-12/schema";

/// Each kind of member with the number its schema file carries: the study its own number, the
/// manifest and the artifact index the number of the whole file, the queue its own number.
const KINDS: [(&str, u32); 4] = [
    ("study", Study::SCHEMA_VERSION),
    ("manifest", CURRENT_SCHEMA),
    ("artifacts", CURRENT_SCHEMA),
    ("queue", QUEUE_SCHEMA),
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
        ("queue", QUEUE_SCHEMA, queue_schema()),
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

/// The schema 1 and schema 2 files as they stood when the next schema was introduced: the length
/// in bytes and an FNV-1a 64-bit checksum of each. The artifact-index file for schema 1 carries
/// the link id rule that commit 844169c added earlier the same day, before any release; the
/// schema/README.md says so. The checksum is there to notice an edit, and it is not a security
/// hash.
const FROZEN_SUMS: [(&str, usize, u64); 4] = [
    ("cfdcraft-artifacts-v1.json", 2978, 0x8ed5_3786_352f_22a3),
    ("cfdcraft-artifacts-v2.json", 24176, 0x2b47_f412_48b7_3eec),
    ("cfdcraft-manifest-v1.json", 1289, 0x7643_2505_6cea_c61e),
    ("cfdcraft-manifest-v2.json", 1289, 0x22f5_c08b_b492_dd7b),
];

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |sum, &byte| {
        (sum ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn the_schema_1_and_2_files_are_the_text_recorded_when_they_were_frozen() {
    // Comparing the file with its frozen copy only shows that the two agree: a change to both at
    // once passes that. The recorded checksum is what such a change has to get past.
    let frozen_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frozen");
    for (file, length, sum) in FROZEN_SUMS {
        for (place, dir) in [
            ("schema", schema_dir()),
            ("frozen copy", frozen_dir.clone()),
        ] {
            let bytes = fs::read(dir.join(file)).unwrap();
            assert_eq!(
                (bytes.len(), checksum(&bytes)),
                (length, sum),
                "the {place} of {file} is not the text recorded when it was frozen; a \
                 published file is never edited, so raise the schema number instead"
            );
        }
    }
}

#[test]
fn the_readme_names_every_published_file() {
    let readme = fs::read_to_string(schema_dir().join("README.md")).unwrap();
    let frozen = [
        "cfdcraft-manifest-v1.json",
        "cfdcraft-manifest-v2.json",
        "cfdcraft-manifest-v3.json",
        "cfdcraft-manifest-v4.json",
        "cfdcraft-artifacts-v1.json",
        "cfdcraft-artifacts-v2.json",
        "cfdcraft-artifacts-v3.json",
        "cfdcraft-artifacts-v4.json",
    ]
    .map(str::to_owned);
    let published_files = published().into_iter().map(|(file, _)| file);
    for file in published_files.chain(frozen) {
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
            "checkpoint",
            "checks",
            "comparison",
            "continued_from",
            "id",
            "key_numbers",
            "mesh",
            "np",
            "outcome",
            "provenance",
            "rerun_of",
            "result",
            "scenario",
            "verdict"
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

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn the_artifact_index_schema_describes_variants_sweeps_and_the_pruned_list() {
    let schema = artifacts_schema();
    for (name, definition) in [
        ("scenarios", "Scenario"),
        ("sweeps", "Sweep"),
        ("pruned", "ContentHash"),
    ] {
        assert_eq!(schema["properties"][name]["type"], "array", "{name}");
        assert_eq!(
            schema["properties"][name]["items"]["$ref"],
            format!("#/$defs/{definition}"),
            "{name}"
        );
    }
    assert!(
        schema.get("required").is_none(),
        "a file without variants, sweeps or pruned results must stay valid"
    );

    let defs = &schema["$defs"];
    assert_eq!(
        property_names(&defs["Scenario"]),
        [
            "id",
            "name",
            "notes",
            "origin",
            "overrides",
            "parent",
            "pinned"
        ]
    );
    assert_eq!(defs["Scenario"]["required"], json!(["id"]));
    assert_eq!(property_names(&defs["Sweep"]), ["base", "id", "parameters"]);
    assert_eq!(
        property_names(&defs["MeshRecipe"]),
        [
            "cells", "geometry", "grading", "level", "patches", "raw", "shape", "tool"
        ]
    );
    assert_eq!(
        property_names(&defs["ResultRecipe"]),
        [
            "boundaries",
            "fluid",
            "mesh_key",
            "monitors",
            "physics",
            "raw",
            "solver",
            "tool"
        ]
    );
    assert!(defs["Study"]["properties"].get("raw_overrides").is_some());
    for name in [
        "Override",
        "Origin",
        "Parameter",
        "Target",
        "Values",
        "RawOverride",
        "RawBackend",
    ] {
        assert!(defs.get(name).is_some(), "{name} is not described");
    }
}

#[test]
fn the_study_schema_is_extended_not_changed() {
    let schema = study_schema();
    let mut with = cavity();
    with["raw_overrides"] = json!({
        "relax": {
            "backend": "esi",
            "file": "system/fvSolution",
            "entry": "relaxationFactors/equations/U",
            "value": "0.7"
        }
    });
    assert_eq!(validate(&schema, &cavity()), Ok(()));
    assert_eq!(validate(&schema, &with), Ok(()));
    Study::from_json(&with.to_string()).unwrap();
    // The monitors added later are optional members of the same study file.
    assert_eq!(validate(&schema, &probe_study()), Ok(()));

    let mut extra = with.clone();
    extra["raw_overrides"]["relax"]["comment"] = json!("x");
    assert!(validate(&schema, &extra).is_err());

    let mut other = with;
    other["raw_overrides"]["relax"]["backend"] = json!("openfoam");
    assert!(validate(&schema, &other).is_err());
}

#[test]
fn an_index_with_variants_sweeps_and_pruned_hashes_validates() {
    let schema = artifacts_schema();
    let lid = "/boundary_conditions/lid/velocity/value";
    let index = json!({
        "blobs": [{ "hash": HASH, "size": 1, "kind": "result" }],
        "scenarios": [{
            "id": "fast-lid",
            "parent": "base",
            "name": "Faster lid",
            "pinned": true,
            "notes": "Checked against the slower lid.",
            "overrides": [
                { "op": "set", "setting": lid, "value": 2.0 },
                { "op": "remove", "setting": "/raw_overrides/relax" }
            ],
            "origin": { "sweep": "lid-sweep", "index": 0 }
        }],
        "sweeps": [{
            "id": "lid-sweep",
            "base": "base",
            "parameters": [
                {
                    "name": "lid",
                    "target": { "kind": "quantity", "setting": lid, "unit": "m/s" },
                    "values": { "mode": "list", "items": [1.0, 2.0] }
                },
                {
                    "name": "cells",
                    "target": { "kind": "count", "setting": "/mesh/cells_x" },
                    "values": { "mode": "range", "start": 16.0, "stop": 64.0, "step": 16.0 }
                },
                {
                    "name": "limit",
                    "target": { "kind": "number", "setting": "/solver/convergence/p" },
                    "values": { "mode": "steps", "start": 0.001, "stop": 0.01, "count": 4 }
                },
                {
                    "name": "angle",
                    "target": {
                        "kind": "direction",
                        "setting_x": "/a/x",
                        "setting_y": "/a/y",
                        "unit": "m/s",
                        "base_x": 1.0,
                        "base_y": 0.0
                    },
                    "values": { "mode": "list", "items": [0.0, 90.0] }
                }
            ]
        }],
        "pruned": [HASH]
    });
    assert_eq!(validate(&schema, &index), Ok(()));
    serde_json::from_value::<ArtifactIndex>(index.clone()).unwrap();

    let mut other_op = index.clone();
    other_op["scenarios"][0]["overrides"][0]["op"] = json!("replace");
    assert!(validate(&schema, &other_op).is_err());

    let mut no_value = index.clone();
    no_value["scenarios"][0]["overrides"][0]
        .as_object_mut()
        .unwrap()
        .remove("value");
    assert!(validate(&schema, &no_value).is_err());

    let mut other_kind = index.clone();
    other_kind["sweeps"][0]["parameters"][0]["target"]["kind"] = json!("angle");
    assert!(validate(&schema, &other_kind).is_err());

    let mut other_mode = index.clone();
    other_mode["sweeps"][0]["parameters"][0]["values"]["mode"] = json!("random");
    assert!(validate(&schema, &other_mode).is_err());

    let mut extra = index;
    extra["scenarios"][0]["origin"]["comment"] = json!("x");
    assert!(validate(&schema, &extra).is_err());
}

/// The schema 3 files as they stood when schema 4 was introduced: the length in bytes and an
/// FNV-1a 64-bit checksum of each.
const FROZEN_SUMS_V3: [(&str, usize, u64); 2] = [
    ("cfdcraft-artifacts-v3.json", 36131, 0x3d39_6105_a876_562c),
    ("cfdcraft-manifest-v3.json", 1289, 0x5d96_90e5_9265_8798),
];

#[test]
fn the_frozen_v3_files_are_byte_identical_to_the_published_v3_files_and_pinned() {
    let frozen_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frozen");
    for (file, length, sum) in FROZEN_SUMS_V3 {
        let published = fs::read(schema_dir().join(file)).unwrap();
        let frozen = fs::read(frozen_dir.join(file)).unwrap();
        assert_eq!(published, frozen, "{file}: the frozen copy differs");
        assert_eq!(
            (frozen.len(), checksum(&frozen)),
            (length, sum),
            "{file} is not the text recorded when it was frozen; a published file is never \
             edited, so raise the schema number instead"
        );
    }
}

#[test]
fn the_artifact_index_schema_describes_the_new_run_members() {
    let schema = artifacts_schema();
    let defs = &schema["$defs"];
    let run = &defs["RunRecord"];
    for name in ["continued_from", "outcome", "checkpoint", "verdict", "np"] {
        assert!(run["properties"].get(name).is_some(), "{name}");
        let required = run["required"].as_array().unwrap();
        assert!(!required.contains(&json!(name)), "{name} must be optional");
    }
    assert_eq!(
        property_names(&defs["MonitorsRecipe"]),
        [
            "centreline_u_sample_count",
            "forces",
            "lines",
            "probes",
            "surfaces"
        ]
    );
    assert_eq!(
        property_names(&defs["ProbeRecipe"]),
        ["field", "on", "x", "y"]
    );
    assert_eq!(
        property_names(&defs["LineRecipe"]),
        ["field", "samples", "x0", "x1", "y0", "y1"]
    );
    assert_eq!(
        property_names(&defs["SurfaceRecipe"]),
        ["boundary", "kind", "quantity"]
    );
    assert_eq!(
        property_names(&defs["Checkpoint"]),
        ["blob", "size", "time"]
    );
    assert_eq!(property_names(&defs["ContinuedFrom"]), ["run", "time"]);
    // The verdict kept on a run is the settle verdict, which the compare verdict of a rerun
    // shares a name with.
    assert_eq!(
        property_names(&defs["Verdict2"]),
        [
            "energy",
            "mass",
            "quantities",
            "reasons",
            "residuals_ok",
            "state"
        ]
    );
    assert_eq!(
        run["properties"]["verdict"]["anyOf"][0]["$ref"],
        "#/$defs/Verdict2"
    );
    assert_eq!(
        property_names(&defs["QuantityState"]),
        [
            "drift", "last", "mean", "name", "samples", "spread", "status"
        ]
    );
    assert_eq!(
        property_names(&defs["Balance"]),
        ["imbalance", "reference", "status"]
    );
}

fn probe_study() -> Value {
    let mut study = cavity();
    study["monitors"]["probes"] = json!({
        "centre": {
            "x": { "value": 0.05, "unit": "m" },
            "y": { "value": 0.05, "unit": "m" },
            "field": "velocity"
        }
    });
    study["monitors"]["lines"] = json!({
        "mid": {
            "x0": { "value": 0.0, "unit": "m" },
            "y0": { "value": 0.05, "unit": "m" },
            "x1": { "value": 0.1, "unit": "m" },
            "y1": { "value": 0.05, "unit": "m" },
            "samples": 64,
            "field": "pressure"
        }
    });
    study["monitors"]["surfaces"] = json!({
        "out": { "boundary": "lid", "quantity": "flux" }
    });
    study["monitors"]["settle"] = json!({ "tail_fraction": 0.2, "tail_floor": 50 });
    study
}

#[test]
fn the_study_schema_takes_the_optional_monitors_and_refuses_unknown_members() {
    let schema = study_schema();
    assert_eq!(validate(&schema, &cavity()), Ok(()));
    let with = probe_study();
    assert_eq!(validate(&schema, &with), Ok(()));
    Study::from_json(&with.to_string()).unwrap();

    let mut extra = with.clone();
    extra["monitors"]["probes"]["centre"]["comment"] = json!("x");
    assert!(validate(&schema, &extra).is_err());

    let mut other_field = with.clone();
    other_field["monitors"]["probes"]["centre"]["field"] = json!("temperature");
    assert!(validate(&schema, &other_field).is_err());

    let mut other_quantity = with.clone();
    other_quantity["monitors"]["surfaces"]["out"]["quantity"] = json!("heat");
    assert!(validate(&schema, &other_quantity).is_err());

    let mut unknown_setting = with;
    unknown_setting["monitors"]["settle"]["tolerance"] = json!(0.1);
    assert!(validate(&schema, &unknown_setting).is_err());
}

#[test]
fn the_queue_schema_validates_a_good_queue_and_refuses_an_unknown_state() {
    let schema = queue_schema();
    assert_eq!(schema["$id"], "urn:cfdcraft:schema:queue:1");
    let good = json!({
        "schema_version": QUEUE_SCHEMA,
        "next_number": 3,
        "jobs": [
            {
                "id": "q-001",
                "kind": "solve",
                "state": "running",
                "run_id": "cavity-4242-1",
                "progress": { "time": 120.5, "iterations": 240 }
            },
            { "id": "q-002", "kind": { "continue": { "run": "run-1" } }, "state": "queued" }
        ]
    });
    assert_eq!(validate(&schema, &good), Ok(()));
    Queue::from_json(&good.to_string()).unwrap();
    assert_eq!(validate(&schema, &json!({ "schema_version": 1 })), Ok(()));

    let mut state = good.clone();
    state["jobs"][0]["state"] = json!("paused");
    assert!(validate(&schema, &state).is_err());

    let mut number = good.clone();
    number["schema_version"] = json!(2);
    assert!(validate(&schema, &number).is_err());

    let mut extra = good;
    extra["jobs"][1]["comment"] = json!("x");
    assert!(validate(&schema, &extra).is_err());
}

#[test]
fn published_includes_the_queue_file() {
    let files: Vec<String> = published().into_iter().map(|(file, _)| file).collect();
    assert_eq!(
        files
            .iter()
            .filter(|file| file.as_str() == "cfdcraft-queue-v1.json")
            .count(),
        1
    );
}

/// The schema 4 files as they stood when schema 5 was introduced: the length in bytes and an
/// FNV-1a 64-bit checksum of each.
const FROZEN_SUMS_V4: [(&str, usize, u64); 2] = [
    ("cfdcraft-artifacts-v4.json", 51391, 0x29dc_0307_a5d2_8f4f),
    ("cfdcraft-manifest-v4.json", 1289, 0x6d4e_e149_1469_6555),
];

#[test]
fn the_frozen_v4_files_are_byte_identical_to_the_published_v4_files_and_pinned() {
    let frozen_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frozen");
    for (file, length, sum) in FROZEN_SUMS_V4 {
        let published = fs::read(schema_dir().join(file)).unwrap();
        let frozen = fs::read(frozen_dir.join(file)).unwrap();
        assert_eq!(published, frozen, "{file}: the frozen copy differs");
        assert_eq!(
            (frozen.len(), checksum(&frozen)),
            (length, sum),
            "{file} is not the text recorded when it was frozen; a published file is never \
             edited, so raise the schema number instead"
        );
    }
}

/// Whether an enum definition written as `oneOf` or as `enum` holds the word.
fn has_word(definition: &Value, word: &str) -> bool {
    let listed = definition["oneOf"].as_array().is_some_and(|variants| {
        variants.iter().any(|variant| {
            variant["const"] == word
                || variant["enum"]
                    .as_array()
                    .is_some_and(|words| words.contains(&json!(word)))
        })
    });
    listed
        || definition["enum"]
            .as_array()
            .is_some_and(|words| words.contains(&json!(word)))
}

#[test]
fn the_artifact_index_schema_describes_the_new_recipe_members() {
    let schema = artifacts_schema();
    let defs = &schema["$defs"];
    for (definition, member) in [
        ("MeshRecipe", "level"),
        ("MeshRecipe", "grading"),
        ("MeshRecipe", "shape"),
        ("PatchRecipe", "thermal"),
        ("ResultRecipe", "physics"),
        ("BoundaryRecipe", "profile"),
        ("BoundaryRecipe", "thermal"),
        ("MonitorsRecipe", "forces"),
        ("ProbeRecipe", "on"),
        ("SurfaceRecipe", "boundary"),
    ] {
        let found = &defs[definition];
        assert!(
            found["properties"].get(member).is_some(),
            "{definition}.{member}"
        );
        let required = found["required"].as_array().cloned().unwrap_or_default();
        assert!(
            !required.contains(&json!(member)),
            "{definition}.{member} must be optional"
        );
    }
    assert_eq!(
        property_names(&defs["ForceRecipe"]),
        ["boundary", "reference_length", "reference_velocity"]
    );
    assert_eq!(property_names(&defs["GradingRecipe"]), ["x", "y"]);
    assert_eq!(
        property_names(&defs["PhysicsRecipe"]),
        ["buoyancy", "init", "time", "turbulence"]
    );
    assert_eq!(
        property_names(&defs["TurbulenceRecipe"]),
        [
            "intensity",
            "model",
            "reference_velocity",
            "viscosity_ratio"
        ]
    );
    assert_eq!(
        property_names(&defs["BuoyancyRecipe"]),
        [
            "beta",
            "gravity",
            "prandtl",
            "reference_temperature",
            "turbulent_prandtl"
        ]
    );
    assert!(has_word(&defs["SurfaceQuantity"], "heat_flux"));
    for word in ["laminar", "k_omega_sst", "realizable_ke", "rng_k_epsilon"] {
        assert!(has_word(&defs["TurbulenceModel"], word), "{word}");
    }
    // The study carried inside the index is the same study definition the study schema holds.
    let study = study_schema();
    for name in [
        "Domain",
        "MeshSpec",
        "BoundaryCondition",
        "Monitors",
        "Probe",
        "Face",
    ] {
        assert_eq!(defs[name], study["$defs"][name], "{name}");
    }
}

fn study_member(extra: Value) -> Value {
    let mut study = cavity();
    for (name, value) in extra.as_object().unwrap() {
        let mut at = &mut study;
        let mut path = name.split('/').peekable();
        while let Some(step) = path.next() {
            if path.peek().is_none() {
                at[step] = value.clone();
            } else {
                at = &mut at[step];
            }
        }
    }
    study
}

#[test]
fn the_study_schema_is_extended_not_changed_again() {
    let schema = study_schema();
    assert_eq!(schema["$id"], "urn:cfdcraft:schema:study:1");
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        json!(Study::SCHEMA_VERSION)
    );
    let defs = &schema["$defs"];
    for (parent, members) in [
        (
            "Domain",
            vec!["length_x", "length_y", "length_z", "open", "shape"],
        ),
        (
            "MeshSpec",
            vec!["cells_x", "cells_y", "cells_z", "grading", "level"],
        ),
        (
            "BoundaryCondition",
            vec!["faces", "kind", "profile", "thermal", "velocity"],
        ),
        (
            "Monitors",
            vec![
                "centreline_u_sample_count",
                "forces",
                "lines",
                "probes",
                "settle",
                "surfaces",
            ],
        ),
        ("Probe", vec!["boundary", "field", "x", "y"]),
    ] {
        assert_eq!(property_names(&defs[parent]), members, "{parent}");
    }
    assert!(schema["properties"].get("physics").is_some());
    assert!(has_word(&defs["Face"], "Obstacle"));
    assert!(has_word(&defs["SolverKind"], "FromPhysics"));
    assert!(has_word(&defs["SolverKind"], "SimpleLaminar"));
    assert!(has_word(&defs["SurfaceQuantity"], "heat_flux"));
    assert!(has_word(&defs["SurfaceQuantity"], "flux"));

    let length = |value: f64| json!({ "value": value, "unit": "m" });
    let shaped = study_member(json!({
        "domain/shape": {
            "kind": "backward_step",
            "step_height": length(0.01),
            "upstream_length": length(0.05),
            "startup_length": length(0.0)
        },
        "mesh/level": 2,
        "mesh/grading": { "x": 1.2 },
        "boundary_conditions/walls/profile": "parabolic",
        "boundary_conditions/walls/thermal": { "kind": "adiabatic" },
        "monitors/forces": {
            "drag": {
                "boundary": "walls",
                "reference_velocity": { "value": 1.0, "unit": "m/s" },
                "reference_length": length(0.1)
            }
        },
        "physics": {
            "time": { "mode": "transient", "delta_t": { "value": 0.001, "unit": "s" } },
            "turbulence": { "model": "k_omega_sst", "intensity": 0.05 },
            "init": "potential"
        }
    }));
    assert_eq!(validate(&schema, &shaped), Ok(()));
    Study::from_json(&shaped.to_string()).unwrap();

    let mut sphere = shaped.clone();
    sphere["domain"]["shape"]["kind"] = json!("sphere");
    assert!(validate(&schema, &sphere).is_err());

    let mut extra = shaped.clone();
    extra["domain"]["shape"]["comment"] = json!("x");
    assert!(validate(&schema, &extra).is_err());

    let mut word = shaped.clone();
    word["physics"]["turbulence"]["model"] = json!("k_epsilon");
    assert!(validate(&schema, &word).is_err());

    let mut unknown = shaped;
    unknown["physics"]["turbulence"]["wall_law"] = json!("standard");
    assert!(validate(&schema, &unknown).is_err());
}

#[test]
fn the_readme_says_what_each_number_is() {
    let readme = fs::read_to_string(schema_dir().join("README.md")).unwrap();
    for text in [
        "cfdcraft-manifest-v5.json",
        "cfdcraft-artifacts-v5.json",
        "cfdcraft-study-v1.json",
        "cfdcraft-queue-v1.json",
        "cfdcraft-manifest-v4.json",
        "cfdcraft-artifacts-v4.json",
    ] {
        assert!(readme.contains(text), "{text}");
    }
    assert!(readme.contains("the file number is 5"), "the file number");
}
