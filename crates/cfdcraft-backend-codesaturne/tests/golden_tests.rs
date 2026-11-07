mod common;

use cfdcraft_backend::{Backend, CaseTree, CompileFailure, CompileOptions, Family};
use cfdcraft_backend_codesaturne::backends;
use cfdcraft_model::{
    InitMode, ResolvedShape, ResolvedStudy, Severity, Study, TurbulenceModel, diagnose, has_errors,
    resolve,
};
use cfdcraft_templates::{TemplateOptions, build};
use serde_json::json;
use std::path::{Path, PathBuf};

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn backend() -> &'static dyn Backend {
    backends()[0]
}

/// The lid cavity with `cells` cells along each side.
fn cavity(cells: u64) -> ResolvedStudy {
    let mut study = resolve(&Study::from_json(CAVITY_V1).unwrap()).unwrap();
    study.mesh.cells_x = cells;
    study.mesh.cells_y = cells;
    study
}

fn text_of<'a>(tree: &'a CaseTree, name: &str) -> &'a str {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    std::str::from_utf8(&file.bytes).unwrap()
}

/// The tree with `from` replaced by `to` in one file.
fn mutated(tree: &CaseTree, name: &str, from: &str, to: &str) -> CaseTree {
    let mut tree = tree.clone();
    let file = tree
        .files
        .iter_mut()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    let text = String::from_utf8(file.bytes.clone()).unwrap();
    assert!(text.contains(from), "{name} has no {from}");
    file.bytes = text.replace(from, to).into_bytes();
    tree
}

fn golden_root() -> PathBuf {
    golden_folder("cavity-codesaturne")
}

fn golden_folder(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// The `/`-separated names of every file below `dir`, sorted.
fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, base, out);
        } else {
            let rel = path.strip_prefix(base).unwrap();
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(parts.join("/"));
        }
    }
}

#[test]
fn the_cavity_tree_passes_the_gates_and_matches_its_golden() {
    let study = cavity(4);
    let options = CompileOptions::default();
    let tree = backend().compile(&study, &options).unwrap();
    let names: Vec<String> = tree
        .files
        .iter()
        .map(|f| f.rel_path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(names, ["DATA/setup.xml", "MESH/mesh.msh"]);
    assert!(backend().scan(&tree).is_empty());
    let found = backend().read_back(&study, &tree);
    assert!(found.is_empty(), "{found:?}");

    // Each value the check reads is a finding when the written case no longer holds it.
    let viscosity = mutated(&tree, "DATA/setup.xml", "0.0001", "0.0002");
    let iterations = mutated(&tree, "DATA/setup.xml", ">5000<", ">4999<");
    let label = mutated(&tree, "DATA/setup.xml", "label=\"y_max\"", "label=\"top\"");
    for (what, changed) in [
        ("viscosity", viscosity),
        ("iteration count", iterations),
        ("boundary label", label),
    ] {
        let found = backend().read_back(&study, &changed);
        assert!(
            found.iter().any(|d| d.severity == Severity::Error),
            "{what} was changed and nothing was found"
        );
        assert!(
            found.iter().all(|d| d.code.starts_with("CFD-RT-")),
            "{found:?}"
        );
    }
    let short = mutated(&tree, "MESH/mesh.msh", "2.2 0 8", "4.1 0 8");
    assert!(!backend().read_back(&study, &short).is_empty());

    // The golden files hold the text the first CI run prints; until then they hold a placeholder.
    let root = golden_root();
    let mut golden_names = Vec::new();
    walk(&root, &root, &mut golden_names);
    golden_names.sort();
    assert_eq!(golden_names, names);
    let mut misses = Vec::new();
    for name in &names {
        let golden = std::fs::read_to_string(root.join(name)).unwrap();
        let actual = text_of(&tree, name);
        if golden != actual {
            misses.push(format!(
                "{name} differs from its golden file; the actual text is:\n{actual}"
            ));
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n=====\n"));
}

#[test]
fn the_row_states_its_identity_and_limits() {
    let b = backend();
    assert_eq!(b.id(), "codesaturne-9.2.0");
    assert_eq!(b.family(), Family::CodeSaturne);
    assert_eq!(b.image(), None);
    assert!(!b.preview());
    let caps = b.capabilities();
    assert_eq!(
        caps.turbulence_models,
        [TurbulenceModel::Laminar, TurbulenceModel::KOmegaSst]
    );
    assert!(caps.transient);
    assert!(!caps.buoyancy);
    assert!(!caps.potential_init);
    assert!(!caps.parabolic_inlet);
    assert!(!caps.accepts_raw);
    assert!(caps.requires_single_cell_z);

    let options = TemplateOptions::default();
    let code_of = |study: &Study, code: &str| diagnose(study, &caps).iter().any(|d| d.code == code);
    let realizable = build(
        "lid-cavity",
        &TemplateOptions {
            turbulence: Some(TurbulenceModel::RealizableKe),
            ..options
        },
    )
    .unwrap();
    assert!(code_of(&realizable, "CFD-BACKEND-002"));
    let heated = build("heated-cavity", &options).unwrap();
    assert!(code_of(&heated, "CFD-BACKEND-004"));
    let potential = build(
        "lid-cavity",
        &TemplateOptions {
            init: Some(InitMode::Potential),
            ..options
        },
    )
    .unwrap();
    assert!(code_of(&potential, "CFD-BACKEND-005"));
    let mut raw: serde_json::Value = serde_json::from_str(CAVITY_V1).unwrap();
    raw["raw_overrides"] = json!({"relax-u": {
        "backend": "esi",
        "file": "system/fvSolution",
        "entry": "relaxationFactors/equations/U",
        "value": "0.5"
    }});
    assert!(code_of(
        &Study::from_json(&raw.to_string()).unwrap(),
        "CFD-BACKEND-006"
    ));

    // The plain cavity is inside every limit.
    let plain = Study::from_json(CAVITY_V1).unwrap();
    let found = diagnose(&plain, &caps);
    assert!(!has_errors(&found), "{found:?}");
}

#[test]
fn the_backend_refuses_what_it_cannot_write() {
    let options = CompileOptions::default();
    // A cylinder with a parabolic inlet is refused by name, not written without its profile.
    let cylinder = common::coarse("cylinder");
    assert!(matches!(
        backend().compile(&cylinder, &options),
        Err(CompileFailure::NotSupported { .. })
    ));
    // A shape the study gives boundaries for only partly is invalid, not silently completed.
    let mut shaped = cavity(2);
    shaped.domain.shape = Some(ResolvedShape::CylinderChannel {
        diameter: 0.01,
        centre_x: 0.05,
        centre_y: 0.05,
    });
    assert!(matches!(
        backend().compile(&shaped, &options),
        Err(CompileFailure::Invalid { .. })
    ));
    let mut thick = cavity(2);
    thick.mesh.cells_z = 2;
    assert!(matches!(
        backend().compile(&thick, &options),
        Err(CompileFailure::Invalid { .. })
    ));
    assert!(matches!(
        backend().compile(&cavity(2), &CompileOptions { np: 0 }),
        Err(CompileFailure::Invalid { .. })
    ));
    let mut raw: serde_json::Value = serde_json::from_str(CAVITY_V1).unwrap();
    raw["raw_overrides"] = json!({"relax-u": {
        "backend": "esi",
        "file": "system/fvSolution",
        "entry": "relaxationFactors/equations/U",
        "value": "0.5"
    }});
    // The study resolves under the permissive limits; this backend still refuses to write it.
    let with_raw = resolve(&Study::from_json(&raw.to_string()).unwrap()).unwrap();
    assert!(matches!(
        backend().compile(&with_raw, &options),
        Err(CompileFailure::Raw { .. })
    ));
}

#[test]
fn the_run_plan_runs_the_solver_from_the_case_folder() {
    let study = cavity(2);
    let plan = backend().run_plan(&study, 1);
    assert_eq!(plan.steps.len(), 1);
    let step = &plan.steps[0];
    assert!(
        step.command.starts_with("code_saturne run "),
        "{}",
        step.command
    );
    assert!(step.command.contains("--case ."), "{}", step.command);
    assert!(
        step.command.contains("--param DATA/setup.xml"),
        "{}",
        step.command
    );
    assert!(!step.command.contains("-n "), "{}", step.command);
    assert_eq!(step.log, "log.code_saturne");
    assert_eq!(plan.solver_log, "log.code_saturne");
    for word in ["gui", "..", "/home", "~"] {
        assert!(
            !step.command.contains(word),
            "{} holds {word}",
            step.command
        );
    }
    assert!(!step.command.split_whitespace().any(|w| w.starts_with('/')));
    assert!(!plan.transient && !plan.buoyant && !plan.potential);

    let parallel = backend().run_plan(&study, 4);
    assert!(
        parallel.steps[0].command.ends_with(" -n 4"),
        "{}",
        parallel.steps[0].command
    );

    let options = CompileOptions { np: 4 };
    let tree = backend().compile(&study, &options).unwrap();
    let exported = backend().export(&study, &tree, "abc123", &options);
    let script = text_of(&exported, "Allrun");
    assert!(script.starts_with("#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n"));
    assert!(script.contains(&format!(
        "{} > log.code_saturne 2>&1 || exit 1\n",
        parallel.steps[0].command
    )));
    let readme = text_of(&exported, "README.md");
    assert!(readme.contains("abc123") && readme.contains("CFDCraft"));
    assert!(!readme.contains("cfdcraft"));
    assert!(readme.lines().all(|l| !l.ends_with(' ')));
}

/// The summary of a mesh text a reviewer can check against the shape: counts, extent and the
/// number of faces of every group. The text of a real benchmark mesh is far too long to review.
fn mesh_summary(text: &str) -> String {
    let msh = common::read(text);
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for node in &msh.nodes {
        lo = [lo[0].min(node[0]), lo[1].min(node[1]), lo[2].min(node[2])];
        hi = [hi[0].max(node[0]), hi[1].max(node[1]), hi[2].max(node[2])];
    }
    let hexahedra = msh
        .elements
        .iter()
        .filter(|(kind, _, _)| *kind == 5)
        .count();
    let mut out = format!(
        "nodes {}\nhexahedra {hexahedra}\nx {:.6} {:.6}\ny {:.6} {:.6}\nz {:.6} {:.6}\n",
        msh.nodes.len(),
        lo[0],
        hi[0],
        lo[1],
        hi[1],
        lo[2],
        hi[2]
    );
    for (id, (_, name)) in &msh.names {
        let faces = msh
            .elements
            .iter()
            .filter(|(kind, group, _)| *kind == 3 && group == id)
            .count();
        if faces > 0 {
            out.push_str(&format!("group {name} {faces}\n"));
        }
    }
    out
}

/// What a study that asks for more than the backend offers is told, one finding a line.
fn refusal_text(id: &str) -> String {
    let study = build(id, &common::options(cfdcraft_templates::MeshPreset::Coarse)).unwrap();
    let mut out = String::new();
    for found in diagnose(&study, &backend().capabilities()) {
        if found.severity == Severity::Error {
            out.push_str(&format!(
                "{} {} {}\n",
                found.code, found.setting, found.pointer
            ));
        }
    }
    out
}

#[test]
fn the_sst_step_matches_its_golden_and_the_refused_studies_state_why() {
    // The monitoring lines hold three points each here so the setup stays short enough to review;
    // the full counts are checked on the study as the template gives it.
    let mut study = common::coarse("backward-step");
    for line in &mut study.monitors.lines {
        line.samples = 3;
    }
    let tree = backend()
        .compile(&study, &CompileOptions::default())
        .unwrap();
    let names: Vec<String> = tree
        .files
        .iter()
        .map(|f| f.rel_path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(names, ["DATA/setup.xml", "MESH/mesh.msh"]);
    assert!(backend().scan(&tree).is_empty());
    let found = backend().read_back(&study, &tree);
    assert!(found.is_empty(), "{found:?}");

    // One difference per file is collected, so one run prints every file that needs a paste.
    let mut misses = Vec::new();
    let mut check = |folder: &str, name: &str, actual: &str| {
        let path = golden_folder(folder).join(name);
        let golden =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if golden != actual {
            misses.push(format!(
                "{folder}/{name} differs from its golden file; the actual text is:\n{actual}"
            ));
        }
    };
    check(
        "step-sst-codesaturne",
        "DATA/setup.xml",
        text_of(&tree, "DATA/setup.xml"),
    );
    check(
        "step-sst-codesaturne",
        "mesh.summary",
        &mesh_summary(text_of(&tree, "MESH/mesh.msh")),
    );
    for (folder, id) in [
        ("step-codesaturne", "backward-step-laminar"),
        ("cylinder-codesaturne", "cylinder"),
        ("heated-codesaturne", "heated-cavity"),
    ] {
        let actual = refusal_text(id);
        assert!(!actual.is_empty(), "{id} is inside every limit");
        check(folder, "REFUSED.txt", &actual);
    }
    assert!(misses.is_empty(), "{}", misses.join("\n=====\n"));
}
