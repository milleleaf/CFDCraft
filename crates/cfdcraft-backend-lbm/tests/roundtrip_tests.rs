//! The reader reads every fact of the stream back, and a value that is lost or changed is named.

use cfdcraft_backend::{Backend, CaseFile, CaseTree, CompileOptions};
use cfdcraft_backend_lbm::backends;
use cfdcraft_model::{Diagnostic, ResolvedStudy, Severity, Study, resolve};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build};
use serde_json::{Value, json};

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

fn compiled(study: &ResolvedStudy) -> CaseTree {
    backend()
        .compile(study, &CompileOptions::default())
        .unwrap()
}

/// The stream with `edit` applied to its second line, the one that holds the setup commands.
fn mutated(tree: &CaseTree, edit: impl Fn(&mut Value)) -> CaseTree {
    let text = String::from_utf8(tree.files[0].bytes.clone()).unwrap();
    let mut lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    edit(&mut lines[1]);
    let mut out = String::new();
    for line in &lines {
        out.push_str(&serde_json::to_string(line).unwrap());
        out.push('\n');
    }
    CaseTree {
        files: vec![CaseFile {
            rel_path: "commands.jsonl".into(),
            bytes: out.into_bytes(),
        }],
    }
}

/// The setup command with this index: units, material, domain, step.
fn setup(line: &mut Value, index: usize) -> &mut Value {
    &mut line["params"]["arguments"]["commands"][index]["params"]
}

fn assert_names(found: &[Diagnostic], word: &str) {
    assert!(!found.is_empty(), "nothing was reported for {word}");
    for d in found {
        assert_eq!(d.code, "CFD-RT-001", "{d:?}");
        assert_eq!(d.severity, Severity::Error);
    }
    assert!(
        found.iter().any(|d| d.setting.contains(word)),
        "no finding names {word}: {found:?}"
    );
}

#[test]
fn the_reader_reads_every_fact_back() {
    for preset in [MeshPreset::Coarse, MeshPreset::Medium, MeshPreset::Fine] {
        let study = cavity(preset);
        let found = backend().read_back(&study, &compiled(&study));
        assert!(found.is_empty(), "{found:?}");
    }
    let study = cavity(MeshPreset::Coarse);
    let tree = compiled(&study);
    let viscosity = mutated(&tree, |line| {
        setup(line, 1)["fluid"]["viscosity"] = json!(2e-4);
    });
    assert_names(
        &backend().read_back(&study, &viscosity),
        "kinematic viscosity",
    );
    let speed = mutated(&tree, |line| {
        setup(line, 2)["faces"]["y_max"]["velocity"] = json!([0.2, 0.0, 0.0]);
    });
    assert_names(&backend().read_back(&study, &speed), "lid velocity");
    let spacing = mutated(&tree, |line| {
        setup(line, 2)["spacing"] = json!(0.00625);
    });
    assert_names(&backend().read_back(&study, &spacing), "cell size");
    let lid_face = mutated(&tree, |line| {
        setup(line, 2)["faces"]["y_max"] = json!({"type": "wall"});
    });
    assert_names(&backend().read_back(&study, &lid_face), "lid boundary kind");
}

#[test]
fn a_changed_slab_and_a_changed_processor_are_named() {
    let study = cavity(MeshPreset::Coarse);
    let tree = compiled(&study);
    let thin = mutated(&tree, |line| {
        setup(line, 2)["max"][2] = json!(0.003125);
    });
    assert_names(&backend().read_back(&study, &thin), "slab");
    let text = String::from_utf8(tree.files[0].bytes.clone()).unwrap();
    let gpu = CaseTree {
        files: vec![CaseFile {
            rel_path: "commands.jsonl".into(),
            bytes: text.replace("\"cpu\"", "\"gpu\"").into_bytes(),
        }],
    };
    assert_names(&backend().read_back(&study, &gpu), "processor");
}

#[test]
fn a_stream_that_cannot_be_read_is_reported_and_never_passed() {
    let study = cavity(MeshPreset::Coarse);
    let garbage = CaseTree {
        files: vec![CaseFile {
            rel_path: "commands.jsonl".into(),
            bytes: b"not json\n".to_vec(),
        }],
    };
    let found = backend().read_back(&study, &garbage);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, "CFD-RT-002");
    let none = CaseTree { files: Vec::new() };
    assert_eq!(backend().read_back(&study, &none)[0].code, "CFD-RT-002");
    // A command that the study does not account for.
    let extra = mutated(&compiled(&study), |line| {
        line["params"]["arguments"]["commands"]
            .as_array_mut()
            .unwrap()
            .push(json!({"command": "file.save", "params": {}}));
    });
    assert_names(&backend().read_back(&study, &extra), "command stream");
}

/// The tree with `edit` applied to the lines of its stream.
fn with_lines(tree: &CaseTree, edit: impl Fn(&mut Vec<Value>)) -> CaseTree {
    let text = String::from_utf8(tree.files[0].bytes.clone()).unwrap();
    let mut lines: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    edit(&mut lines);
    let mut out = String::new();
    for line in &lines {
        out.push_str(&serde_json::to_string(line).unwrap());
        out.push('\n');
    }
    let mut files = tree.files.clone();
    files[0].bytes = out.into_bytes();
    CaseTree { files }
}

/// The parameters of the probe call on this line of the stream.
fn probe_params(lines: &mut [Value], index: usize) -> &mut Value {
    &mut lines[index]["params"]["arguments"]["params"]
}

#[test]
fn a_dropped_or_changed_line_probe_is_a_finding() {
    let study = cavity(MeshPreset::Coarse);
    let tree = compiled(&study);
    // The first probe line is the first component of the vertical centreline.
    let first = 3;
    let dropped = with_lines(&tree, |lines| {
        lines.remove(first);
    });
    assert_names(&backend().read_back(&study, &dropped), "centreline");
    let count = with_lines(&tree, |lines| {
        probe_params(lines, first)["line"]["count"] = json!(33);
    });
    assert_names(&backend().read_back(&study, &count), "sample count");
    let end = with_lines(&tree, |lines| {
        probe_params(lines, first)["line"]["to"][1] = json!(0.05);
    });
    assert_names(&backend().read_back(&study, &end), "end of the line");
    let start = with_lines(&tree, |lines| {
        probe_params(lines, first)["line"]["from"][1] = json!(0.01);
    });
    assert_names(&backend().read_back(&study, &start), "start of the line");
    let field = with_lines(&tree, |lines| {
        probe_params(lines, first)["line"]["field"] = json!("velocity_y");
    });
    assert_names(&backend().read_back(&study, &field), "velocity component");
    let step = with_lines(&tree, |lines| {
        probe_params(lines, first)["step"] = json!("Other");
    });
    assert_names(&backend().read_back(&study, &step), "step");

    // A call the study does not account for, and a command after a probe.
    let extra = with_lines(&tree, |lines| {
        let mut again = lines.last().unwrap().clone();
        let id = again["id"].as_u64().unwrap() + 1;
        again["id"] = json!(id);
        lines.push(again);
    });
    assert_names(&backend().read_back(&study, &extra), "probe call");
    let after = with_lines(&tree, |lines| {
        let mut solve = lines[2].clone();
        solve["id"] = json!(9999);
        lines.push(solve);
    });
    assert_names(&backend().read_back(&study, &after), "command stream");
}

/// A template study at the coarsest mesh with `edit` applied to its JSON.
fn template(id: &str, edit: impl Fn(&mut Value)) -> ResolvedStudy {
    let options = TemplateOptions {
        mesh: MeshPreset::Coarse,
        ..TemplateOptions::default()
    };
    let mut value = serde_json::to_value(build(id, &options).unwrap()).unwrap();
    edit(&mut value);
    resolve(&Study::from_json(&value.to_string()).unwrap()).unwrap()
}

/// The lines of the stream of a tree, parsed.
fn lines_of(tree: &CaseTree) -> Vec<Value> {
    String::from_utf8(tree.files[0].bytes.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The index of the first line whose probe call holds this member.
fn first_with(lines: &[Value], member: &str) -> usize {
    lines
        .iter()
        .position(|line| line["params"]["arguments"]["params"].get(member).is_some())
        .unwrap_or_else(|| panic!("no probe call holds {member}"))
}

#[test]
fn a_dropped_or_changed_probe_is_a_finding() {
    // The cavity: its point probes, one call for each frame.
    let study = cavity(MeshPreset::Coarse);
    let tree = compiled(&study);
    let first = first_with(&lines_of(&tree), "points");
    let frame = with_lines(&tree, |lines| {
        probe_params(lines, first)["frame"] = json!(7);
    });
    assert_names(&backend().read_back(&study, &frame), "frame number");
    let moved = with_lines(&tree, |lines| {
        probe_params(lines, first)["points"][0][0] = json!(0.0123);
    });
    assert_names(&backend().read_back(&study, &moved), "position of probe 1");
    let dropped = with_lines(&tree, |lines| {
        let points = probe_params(lines, first + 1)["points"]
            .as_array_mut()
            .unwrap();
        points.remove(1);
    });
    assert_names(&backend().read_back(&study, &dropped), "number of points");
    let height = with_lines(&tree, |lines| {
        probe_params(lines, first)["points"][2][2] = json!(0.5);
    });
    assert_names(&backend().read_back(&study, &height), "position of probe 3");
    let missing_frame = with_lines(&tree, |lines| {
        lines.remove(first);
    });
    assert_names(&backend().read_back(&study, &missing_frame), "frame number");
    let frames = mutated(&tree, |line| {
        setup(line, 3)["procedure"]["frames"] = json!(20);
    });
    assert_names(&backend().read_back(&study, &frames), "frames of the step");
    let no_frames = mutated(&tree, |line| {
        setup(line, 3)["procedure"]
            .as_object_mut()
            .unwrap()
            .remove("frames");
    });
    assert_names(
        &backend().read_back(&study, &no_frames),
        "frames of the step",
    );

    // The cylinder: the wall its force probe names.
    let cylinder = template("cylinder", |_| {});
    let tree = compiled(&cylinder);
    assert!(backend().read_back(&cylinder, &tree).is_empty());
    let wall = with_lines(&tree, |lines| {
        let at = first_with(lines, "forces");
        probe_params(lines, at)["forces"][0] = json!("Cylinder/face1");
    });
    assert_names(&backend().read_back(&cylinder, &wall), "wall");
    let no_force = with_lines(&tree, |lines| {
        let at = first_with(lines, "forces");
        lines.remove(at);
    });
    assert_names(&backend().read_back(&cylinder, &no_force), "force probe");

    // The heated cavity: the sides its heat probe names.
    let heated = template("heated-cavity", |v| {
        v["mesh"].as_object_mut().unwrap().remove("grading");
    });
    let tree = compiled(&heated);
    assert!(backend().read_back(&heated, &tree).is_empty());
    let sides = with_lines(&tree, |lines| {
        let at = first_with(lines, "heat");
        probe_params(lines, at)["heat"] = json!(["x_min", "y_max"]);
    });
    assert_names(&backend().read_back(&heated, &sides), "sides");
}
