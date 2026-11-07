//! The run plan, the exported files and the compiled cases against their golden text.

mod support;

use cfdcraft_backend::{Backend, CompileOptions};
use cfdcraft_backend_lbm::backends;
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build};
use serde_json::Value;
use support::model_text;

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const GOLDEN: &str = include_str!("golden/cavity-lbm/commands.jsonl");

fn backend() -> &'static dyn Backend {
    backends()[0]
}

fn cavity() -> ResolvedStudy {
    resolve(&Study::from_json(CAVITY_V1).unwrap()).unwrap()
}

#[test]
fn the_run_plan_pipes_the_stream_through_the_headless_server() {
    let plan = backend().run_plan(&cavity(), 1);
    assert_eq!(plan.steps.len(), 1);
    let step = &plan.steps[0];
    assert_eq!(step.log, "log.fieldcraft");
    let command = &step.command;
    assert!(
        command.contains("fieldcraft-cli mcp --headless"),
        "{command}"
    );
    assert!(command.contains("--automation-read-root /w "), "{command}");
    assert!(
        command.contains("--automation-write-root /w/out "),
        "{command}"
    );
    assert!(command.contains("< commands.jsonl"), "{command}");
    assert!(command.contains("> replies.jsonl"), "{command}");
    assert!(command.contains("2> log.fieldcraft"), "{command}");
    assert!(!command.contains("gpu"), "{command}");
    assert_eq!(plan.final_fields, ["replies.jsonl", "commands.jsonl"]);
    assert_eq!(plan.solver_log, "log.fieldcraft");
    assert!(!plan.transient && !plan.buoyant && !plan.potential);
    assert!(plan.pressure_field.is_empty() && plan.residual_checks.is_empty());
    // The processes asked for do not change what runs.
    assert_eq!(backend().run_plan(&cavity(), 4), plan);
}

#[test]
fn the_export_adds_the_run_script_and_a_preview_readme() {
    let study = cavity();
    let options = CompileOptions::default();
    let tree = backend().compile(&study, &options).unwrap();
    let exported = backend().export(&study, &tree, "abc123", &options);
    let names: Vec<&str> = exported
        .files
        .iter()
        .map(|f| f.rel_path.to_str().unwrap())
        .collect();
    assert_eq!(names, ["commands.jsonl", "Allrun", "README.md"]);
    let script = String::from_utf8(exported.files[1].bytes.clone()).unwrap();
    assert!(script.starts_with("#!/bin/sh\n"));
    assert!(script.contains(&backend().run_plan(&study, 1).steps[0].command));
    let readme = String::from_utf8(exported.files[2].bytes.clone()).unwrap();
    assert!(readme.contains("abc123"));
    assert!(readme.contains("preview, not a gated result"));
    assert!(!readme.contains("cfdcraft"), "write CFDCraft in prose");
    for paragraph in readme.trim_end().split("\n\n") {
        assert!(
            paragraph.starts_with("```") || !paragraph.contains('\n'),
            "{paragraph}"
        );
    }
}

#[test]
fn the_cavity_tree_matches_its_golden() {
    let study = cavity();
    let tree = backend()
        .compile(&study, &CompileOptions::default())
        .unwrap();
    assert_eq!(tree.files.len(), 1);
    assert!(backend().scan(&tree).is_empty());
    assert!(backend().read_back(&study, &tree).is_empty());
    let actual = String::from_utf8(tree.files[0].bytes.clone()).unwrap();
    assert!(
        actual == GOLDEN,
        "commands.jsonl differs from its golden file; the actual text is:\n{actual}"
    );
}

/// A template study at the coarsest mesh with `edit` applied to its JSON.
fn template(id: &str, edit: impl Fn(&mut Value)) -> ResolvedStudy {
    let options = TemplateOptions {
        mesh: MeshPreset::Coarse,
        ..TemplateOptions::default()
    };
    let study = build(id, &options).unwrap();
    let mut value = serde_json::to_value(&study).unwrap();
    edit(&mut value);
    resolve(&Study::from_json(&value.to_string()).unwrap()).unwrap()
}

/// One compiled case, the golden text of its stream and the golden text of its surface, if any.
struct Golden {
    name: &'static str,
    study: ResolvedStudy,
    stream: &'static str,
    surface: Option<&'static str>,
}

#[test]
fn every_compiling_study_round_trips_and_matches_its_golden() {
    let cases = [
        Golden {
            name: "step-lbm",
            // The template's parabolic inlet cannot be spread over the channel above the step.
            study: template("backward-step-laminar", |v| {
                v["boundary_conditions"]["inlet"]
                    .as_object_mut()
                    .unwrap()
                    .remove("profile");
            }),
            stream: include_str!("golden/step-lbm/commands.jsonl"),
            surface: Some(include_str!("golden/step-lbm/obstacles/step.model.xml")),
        },
        Golden {
            name: "cylinder-lbm",
            study: template("cylinder", |_| {}),
            stream: include_str!("golden/cylinder-lbm/commands.jsonl"),
            surface: Some(include_str!(
                "golden/cylinder-lbm/obstacles/cylinder.model.xml"
            )),
        },
        Golden {
            name: "heated-lbm",
            // The solver needs equal square cells, so the template's wall grading is removed.
            study: template("heated-cavity", |v| {
                v["mesh"].as_object_mut().unwrap().remove("grading");
            }),
            stream: include_str!("golden/heated-lbm/commands.jsonl"),
            surface: None,
        },
    ];
    // Every difference of every case is collected before anything is asserted, so one run shows
    // all of them.
    let mut problems = Vec::new();
    for case in &cases {
        let tree = backend()
            .compile(&case.study, &CompileOptions::default())
            .unwrap();
        let refusals = backend().scan(&tree);
        if !refusals.is_empty() {
            problems.push(format!("{}: the guard refuses {refusals:?}", case.name));
        }
        let findings = backend().read_back(&case.study, &tree);
        if !findings.is_empty() {
            problems.push(format!("{}: read back differs: {findings:?}", case.name));
        }
        let stream = String::from_utf8(tree.files[0].bytes.clone()).unwrap();
        if stream != case.stream {
            problems.push(format!(
                "{}/commands.jsonl differs from its golden file; the actual text is:\n{stream}",
                case.name
            ));
        }
        match (case.surface, tree.files.get(1)) {
            (Some(golden), Some(file)) => {
                let model = model_text(&file.bytes);
                if model != golden {
                    problems.push(format!(
                        "{}/{} differs from its golden file; the actual model text is:\n{model}",
                        case.name,
                        file.rel_path.display()
                    ));
                }
            }
            (None, None) => {}
            (golden, file) => problems.push(format!(
                "{}: expected a surface file: {}, found one: {}",
                case.name,
                golden.is_some(),
                file.is_some()
            )),
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n\n"));
}

#[test]
fn the_obstacle_file_stands_for_the_mesh() {
    // The cavity has no body, so no file stands for its mesh.
    assert!(backend().mesh_files(&cavity()).is_empty());

    // The cylinder's body is a file in the case, named the same way.
    let study = template("cylinder", |_| {});
    let named = backend().mesh_files(&study);
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(
        named[0].starts_with("obstacles/") && named[0].ends_with(".3mf"),
        "{named:?}"
    );
    let tree = backend()
        .compile(&study, &CompileOptions::default())
        .unwrap();
    assert!(
        tree.files
            .iter()
            .any(|file| file.rel_path.to_str() == Some(named[0].as_str()))
    );
}
