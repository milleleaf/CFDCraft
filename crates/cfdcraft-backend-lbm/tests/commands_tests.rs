//! The command stream of the lid-driven cavity: its shape, its numbers, its determinism, and the
//! studies it refuses instead of narrowing.

use cfdcraft_backend::{Backend, CompileFailure, CompileOptions};
use cfdcraft_backend_lbm::{backends, guard::MAX_LINE_BYTES, markers::SLAB_CELLS_Z};
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build};
use serde_json::{Value, json};

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn backend() -> &'static dyn Backend {
    backends()[0]
}

fn cavity(mesh: MeshPreset) -> ResolvedStudy {
    let options = TemplateOptions {
        mesh,
        ..TemplateOptions::default()
    };
    resolve(&build("lid-cavity", &options).unwrap()).unwrap()
}

/// The cavity fixture with `edit` applied to its JSON.
fn edited(edit: impl Fn(&mut Value)) -> ResolvedStudy {
    let mut value: Value = serde_json::from_str(CAVITY_V1).unwrap();
    edit(&mut value);
    resolve(&Study::from_json(&value.to_string()).unwrap()).unwrap()
}

fn stream(study: &ResolvedStudy) -> String {
    let tree = backend()
        .compile(study, &CompileOptions::default())
        .unwrap();
    assert_eq!(tree.files.len(), 1);
    assert_eq!(tree.files[0].rel_path.to_str(), Some("commands.jsonl"));
    String::from_utf8(tree.files[0].bytes.clone()).unwrap()
}

/// Every command of the stream in order, with its parameters.
fn commands_of(text: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let message: Value = serde_json::from_str(line).unwrap();
        let arguments = &message["params"]["arguments"];
        let entries = match message["params"]["name"].as_str().unwrap() {
            "batch" => arguments["commands"].as_array().unwrap().clone(),
            _ => vec![arguments.clone()],
        };
        for entry in entries {
            out.push((
                entry["command"].as_str().unwrap().to_owned(),
                entry["params"].clone(),
            ));
        }
    }
    out
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs())
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap()
}

#[test]
fn the_cavity_stream_has_an_initialize_line_then_the_commands() {
    let text = stream(&cavity(MeshPreset::Coarse));
    assert!(text.ends_with('\n'));
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    // The opening, the setup and the solve, then the probes: each of them its own call.
    assert!(lines.len() > 3, "{}", lines.len());
    for line in &lines {
        assert!(line.len() < MAX_LINE_BYTES);
        assert!(line.ends_with('\n'));
        let _: Value = serde_json::from_str(line).unwrap();
    }
    let first: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["method"], "initialize");
    for line in &lines[1..] {
        let message: Value = serde_json::from_str(line).unwrap();
        assert_eq!(message["method"], "tools/call");
    }
    let names: Vec<String> = commands_of(&text)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    // The setup commands up to the solve, and after the solve nothing but probes.
    assert_eq!(
        names[..5],
        [
            "model.units",
            "model.material.create",
            "model.fluid_domain.create",
            "model.step.create",
            "solve.fluid"
        ]
    );
    assert!(
        names[5..].iter().all(|name| name == "solve.probe"),
        "{names:?}"
    );
    // Five commands up to the solve in three lines, and one line for each probe.
    assert_eq!(names.len() - 5, lines.len() - 3);
}

#[test]
fn the_domain_follows_the_study() {
    for (preset, cells) in [(MeshPreset::Coarse, 32.0), (MeshPreset::Medium, 64.0)] {
        let commands = commands_of(&stream(&cavity(preset)));
        let domain = &commands[2].1;
        let spacing = number(&domain["spacing"]);
        assert!(close(spacing, 0.1 / cells), "{spacing}");
        assert!(
            domain["min"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| number(v) == 0.0)
        );
        let max: Vec<f64> = domain["max"]
            .as_array()
            .unwrap()
            .iter()
            .map(number)
            .collect();
        assert!(close(max[0], 0.1) && close(max[1], 0.1));
        assert!(close(max[2], f64::from(SLAB_CELLS_Z) * spacing));
        let faces = &domain["faces"];
        assert_eq!(faces["y_max"]["type"], "moving_wall");
        let lid: Vec<f64> = faces["y_max"]["velocity"]
            .as_array()
            .unwrap()
            .iter()
            .map(number)
            .collect();
        assert!(close(lid[0], 0.1) && lid[1] == 0.0 && lid[2] == 0.0);
        for side in ["x_min", "x_max", "y_min"] {
            assert_eq!(faces[side], json!({"type": "wall"}), "{side}");
        }
        for side in ["z_min", "z_max"] {
            assert_eq!(faces[side], json!({"type": "slip"}), "{side}");
        }
        let material = &commands[1].1;
        assert!(close(number(&material["density"]), 1.0));
        assert_eq!(material["fluid"]["type"], "newtonian");
        assert!(close(number(&material["fluid"]["viscosity"]), 1e-4));
        assert_eq!(commands[4].1["backend"], "cpu");
        let step = &commands[3].1["procedure"];
        assert_eq!(step["domain"], domain["name"]);
        assert!(number(&step["end_time"]) > 0.0);
    }
}

#[test]
fn the_stream_is_deterministic() {
    let study = cavity(MeshPreset::Fine);
    let first = stream(&study);
    assert_eq!(first, stream(&study));
    // Every number is the shortest text that reads back as the same number, and nothing else is
    // changed by reading a line and writing it again.
    for line in first.lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), line);
    }
}

#[test]
fn a_study_the_solver_cannot_represent_is_refused_and_not_narrowed() {
    fn refused(study: &ResolvedStudy) -> CompileFailure {
        backend()
            .compile(study, &CompileOptions::default())
            .unwrap_err()
    }
    fn setting(failure: CompileFailure) -> String {
        match failure {
            CompileFailure::NotSupported { setting, .. } => setting,
            other => panic!("{other:?}"),
        }
    }
    let graded = edited(|v| v["mesh"]["grading"] = json!({"y": 1.2}));
    assert_eq!(setting(refused(&graded)), "wall grading");
    let oblong = edited(|v| v["mesh"]["cells_y"] = json!(64));
    assert_eq!(setting(refused(&oblong)), "cell shape");
    let turbulent = edited(|v| {
        v["solver"]["solver"] = "FromPhysics".into();
        v["physics"] = json!({"turbulence": {
            "model": "k_omega_sst", "intensity": 0.05, "viscosity_ratio": 10.0
        }});
    });
    assert_eq!(setting(refused(&turbulent)), "turbulence model");
    let potential = edited(|v| {
        v["solver"]["solver"] = "FromPhysics".into();
        v["physics"] = json!({"init": "potential"});
    });
    assert_eq!(setting(refused(&potential)), "start of the flow");
    let raw = edited(|v| {
        v["raw_overrides"] = json!({"relax-u": {
            "backend": "esi", "file": "system/fvSolution",
            "entry": "relaxationFactors/equations/U", "value": "0.5",
        }});
    });
    assert!(matches!(
        refused(&raw),
        CompileFailure::Raw { label, .. } if label == "relax-u"
    ));
    let at_rest = edited(|v| {
        v["boundary_conditions"]["lid"] = json!({"faces": ["YMax"], "kind": "NoSlipWall"});
    });
    assert!(matches!(refused(&at_rest), CompileFailure::Invalid { .. }));
}
