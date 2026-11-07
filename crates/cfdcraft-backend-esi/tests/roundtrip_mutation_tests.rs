//! A fault in one value of the written bytes is reported. Every mutation is built from the spans
//! the reader itself recorded for the facts it recovered, and judged by the round-trip check with
//! the backend's list added, so no writer function decides whether a mutation was caught.

mod field_studies;
mod shape_studies;

use cfdcraft_backend_esi::{
    Dialect, compile,
    reader::{EsiReader, source_files},
    shaped_expected_facts,
};
use cfdcraft_model::{
    ResolvedStudy,
    diagnostics::Diagnostic,
    roundtrip::{CaseReader, Expected, Fact, SourceFile, Span, check_roundtrip_with},
};
use shape_studies::{cylinder_json, resolved, step_json};
use std::collections::BTreeMap;

const TRANSIENT: &str = r#"{"time":{"mode":"transient","delta_t":{"value":0.001,"unit":"s"}}}"#;
const PARABOLIC: &str = r#","profile":"parabolic""#;
const SST: &str =
    r#"{"turbulence":{"model":"k_omega_sst","intensity":0.05,"viscosity_ratio":10.0}}"#;
const FORCE: &str = r#","forces":{"drag":{"boundary":"cylinder","reference_velocity":{"value":1,"unit":"m/s"},"reference_length":{"value":0.1,"unit":"m"}}}"#;

/// The study, its written files, what the reader recovered and what the backend adds to the
/// model's facts. The unmutated case must pass.
struct Case {
    study: ResolvedStudy,
    files: Vec<SourceFile>,
    facts: BTreeMap<String, Fact>,
    extras: Vec<Expected>,
}

fn case(json: &str) -> Case {
    let study = resolved(json);
    let tree = compile(&study, Dialect::EsiV2512).unwrap();
    let files = source_files(&tree);
    let facts = EsiReader.read(&files).unwrap();
    let extras = shaped_expected_facts(&study);
    let c = Case {
        study,
        files,
        facts,
        extras,
    };
    assert_eq!(c.check(&c.files), vec![], "negative control");
    c
}

impl Case {
    fn check(&self, files: &[SourceFile]) -> Vec<Diagnostic> {
        check_roundtrip_with(&EsiReader, files, &self.study, &self.extras)
    }

    fn fact(&self, key: &str) -> &Fact {
        self.facts
            .get(key)
            .unwrap_or_else(|| panic!("the reader found no fact {key}"))
    }
}

/// A copy of `files` with the bytes of `span` replaced by `with`.
fn splice(files: &[SourceFile], span: &Span, with: &str) -> Vec<SourceFile> {
    files
        .iter()
        .map(|f| {
            let mut bytes = f.bytes.clone();
            if f.path == span.file {
                bytes.splice(span.start..span.end, with.bytes());
            }
            SourceFile {
                path: f.path.clone(),
                bytes,
            }
        })
        .collect()
}

fn token<'a>(files: &'a [SourceFile], span: &Span) -> &'a str {
    let file = files.iter().find(|f| f.path == span.file).unwrap();
    std::str::from_utf8(&file.bytes[span.start..span.end]).unwrap()
}

/// A token that reads differently from this one: a number one above it, or a word that names
/// nothing.
fn different(token: &str) -> String {
    token
        .parse::<f64>()
        .map_or_else(|_| "unrelated".to_owned(), |v| (v + 1.0).to_string())
}

/// The check failed with findings of the round trip only.
fn caught(found: &[Diagnostic]) -> bool {
    !found.is_empty()
        && found
            .iter()
            .all(|d| matches!(d.code.as_str(), "CFD-RT-001" | "CFD-RT-002"))
}

/// A finding names the fact's setting, or the file the mutation is in.
fn names(c: &Case, found: &[Diagnostic], key: &str, file: &str) -> bool {
    let setting = c
        .extras
        .iter()
        .find(|e| e.key == key)
        .map(|e| e.setting.as_str());
    found.iter().any(|d| {
        setting == Some(d.setting.as_str()) || d.location.as_ref().is_some_and(|l| l.file == file)
    })
}

/// Overwrite the first token of the fact `key` with a different one and expect a report.
fn overwrite(c: &Case, key: &str) {
    let span = c
        .fact(key)
        .value_spans
        .first()
        .unwrap_or_else(|| panic!("{key} has no token to overwrite"));
    let changed = splice(&c.files, span, &different(token(&c.files, span)));
    let found = c.check(&changed);
    assert!(
        caught(&found),
        "{key} at {}:{}: {found:?}",
        span.file,
        span.line
    );
    assert!(names(c, &found, key, &span.file), "{key}: {found:?}");
}

#[test]
fn overwriting_a_vertex_coordinate_is_caught() {
    let c = case(&cylinder_json(0, "cylinder"));
    let keys: Vec<&String> = c
        .facts
        .keys()
        .filter(|k| k.starts_with("/mesh/vertex/"))
        .step_by(7)
        .collect();
    assert!(keys.len() >= 8, "only {} vertex facts sampled", keys.len());
    for key in keys {
        // Every token that carries the coordinate, not only the first.
        for span in &c.fact(key).value_spans {
            let changed = splice(&c.files, span, &different(token(&c.files, span)));
            let found = c.check(&changed);
            assert!(
                caught(&found),
                "{key} at {}:{}: {found:?}",
                span.file,
                span.line
            );
            assert!(names(&c, &found, key, &span.file), "{key}: {found:?}");
        }
    }
    // A step: the same holds for a layout with a start-up section.
    let step = case(&step_json(true, 0, "floor_walls"));
    for key in ["/mesh/vertex/3/y", "/mesh/vertex/9/x", "/domain/length_x"] {
        overwrite(&step, key);
    }
}

#[test]
fn overwriting_a_block_or_a_patch_is_caught() {
    let c = case(&cylinder_json(0, "cylinder"));
    for key in [
        "/mesh/block/0/cells",
        "/mesh/block/5/vertices",
        "/mesh/block/2/grading",
        "/mesh/arc/1/through/x",
        "/mesh/arc/3/ends",
        "/mesh/patch/inlet/type",
        "/mesh/patch/obstacle/faces",
        "/mesh/cells_total",
        "/patch/obstacle/kind",
    ] {
        overwrite(&c, key);
    }
    // Every token of the faces of one patch, one at a time.
    for span in &c.fact("/mesh/patch/inlet/faces").value_spans {
        let found = c.check(&splice(&c.files, span, &different(token(&c.files, span))));
        assert!(caught(&found), "{found:?}");
    }
    // Removing the entry of a patch removes its facts, and the case no longer holds it.
    for key in ["/mesh/patch/outlet/type", "/patch/inlet/kind"] {
        let entry = c.fact(key).entry_span.as_ref().unwrap();
        let found = c.check(&splice(&c.files, entry, ""));
        assert!(caught(&found), "{key}: {found:?}");
    }
}

#[test]
fn overwriting_physics_values_is_caught() {
    // A transient cylinder with a parabolic inlet and a force coefficient on the cylinder.
    let json = field_studies::cylinder_json(TRANSIENT, "8", "0.5", PARABOLIC).replacen(
        r#""centreline_u_sample_count":64"#,
        &format!(r#""centreline_u_sample_count":64{FORCE}"#),
        1,
    );
    let cylinder = case(&json);
    for key in [
        "/solver/time/delta_t",
        "/solver/application",
        "/patch/inlet/velocity",
        "/monitors/force/1/reference_velocity",
        "/monitors/force/1/reference_length",
        "/monitors/force/1/patch",
    ] {
        overwrite(&cylinder, key);
    }
    // A closed cavity with buoyancy.
    let heated = case(&field_studies::heated_json("", "1000", "100"));
    for key in [
        "/physics/buoyancy/beta",
        "/physics/buoyancy/gravity",
        "/physics/buoyancy/prandtl",
        "/boundary_conditions/by_face/XMin/temperature",
        "/boundary_conditions/by_face/XMax/temperature",
    ] {
        overwrite(&heated, key);
    }
    // A step with a turbulence model.
    let step = case(&field_studies::step_json(SST, "100", "100", ""));
    for key in [
        "/physics/turbulence/k",
        "/physics/turbulence/omega",
        "/solver/turbulence",
    ] {
        overwrite(&step, key);
    }
}

#[test]
fn an_unaccounted_entry_is_caught() {
    let c = case(&step_json(true, 0, "floor_walls"));
    // A patch the layout does not have, written into the mesh and into both field files so that the
    // case still reads.
    let add = |files: &[SourceFile], path: &str, after: &str, entry: &str| {
        files
            .iter()
            .map(|f| {
                let mut file = f.clone();
                if f.path == path {
                    let text = std::str::from_utf8(&f.bytes).unwrap();
                    assert!(text.contains(after), "{path} has no {after:?}");
                    file.bytes = text
                        .replacen(after, &format!("{after}{entry}"), 1)
                        .into_bytes();
                }
                file
            })
            .collect::<Vec<_>>()
    };
    let files = add(
        &c.files,
        "system/blockMeshDict",
        "boundary\n(\n",
        "    extra\n    {\n        type wall;\n        faces ((0 1 2 3));\n    }\n",
    );
    let files = add(
        &files,
        "0/U",
        "boundaryField\n{\n",
        "    extra { type noSlip; }\n",
    );
    let files = add(
        &files,
        "0/p",
        "boundaryField\n{\n",
        "    extra { type zeroGradient; }\n",
    );
    let found = c.check(&files);
    assert!(!found.is_empty());
    assert!(
        found
            .iter()
            .all(|d| d.code == "CFD-RT-001" && d.message.contains("does not account for")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|d| d.setting.contains("extra")),
        "no finding names the patch: {found:?}"
    );
}
