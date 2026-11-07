//! Studies and helpers the tests share.
#![allow(dead_code)]

use cfdcraft_backend::{Backend, CaseFile, CaseTree, CompileOptions};
use cfdcraft_backend_of14::backends;
use cfdcraft_model::{
    Grading, InitMode, LineMonitor, MonitorField, Quantity, ResolvedStudy, Study, SurfaceMonitor,
    SurfaceQuantity, TurbulenceModel, Unit, diagnostics::Severity, resolve,
};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build};
use std::path::{Path, PathBuf};

pub fn backend() -> &'static dyn Backend {
    backends()[0]
}

pub fn options(mesh: MeshPreset) -> TemplateOptions {
    TemplateOptions {
        mesh,
        ..TemplateOptions::default()
    }
}

/// The authored study of a template.
pub fn authored(id: &str, options: &TemplateOptions) -> Study {
    build(id, options).unwrap()
}

pub fn study(id: &str, options: &TemplateOptions) -> ResolvedStudy {
    resolve(&authored(id, options)).unwrap()
}

/// A template at its coarsest mesh preset.
pub fn coarse(id: &str) -> ResolvedStudy {
    study(id, &options(MeshPreset::Coarse))
}

pub fn cavity() -> ResolvedStudy {
    coarse("lid-cavity")
}

pub fn step() -> ResolvedStudy {
    coarse("backward-step-laminar")
}

pub fn heated() -> ResolvedStudy {
    coarse("heated-cavity")
}

/// The transient laminar cylinder in a channel, at its coarsest mesh preset.
pub fn cylinder() -> ResolvedStudy {
    coarse("cylinder")
}

/// The turbulent step with an explicit model and start.
pub fn turbulent_step(model: TurbulenceModel, init: Option<InitMode>) -> ResolvedStudy {
    study(
        "backward-step",
        &TemplateOptions {
            turbulence: Some(model),
            init,
            ..options(MeshPreset::Coarse)
        },
    )
}

/// The turbulent step as the template has it: the SST model, a uniform start.
pub fn sst_step() -> ResolvedStudy {
    coarse("backward-step")
}

/// The cavity with wall grading on both axes.
pub fn graded_box() -> ResolvedStudy {
    let mut study = authored("lid-cavity", &options(MeshPreset::Coarse));
    study.mesh.grading = Some(Grading {
        x: Some(1.088),
        y: Some(1.088),
    });
    resolve(&study).unwrap()
}

fn metres(value: f64) -> Quantity {
    Quantity {
        value,
        unit: Unit::Metre,
    }
}

/// The cylinder with a sampled line in its wake and two integrals over its outlet, so every kind
/// of monitor is written.
pub fn cylinder_with_every_monitor() -> ResolvedStudy {
    let mut study = authored("cylinder", &options(MeshPreset::Coarse));
    study.monitors.lines.insert(
        "wake".into(),
        LineMonitor {
            x0: metres(0.3),
            y0: metres(0.2),
            x1: metres(1.0),
            y1: metres(0.2),
            samples: 21,
            field: MonitorField::Velocity,
        },
    );
    for (label, quantity) in [
        ("outlet_pressure", SurfaceQuantity::PressureAverage),
        ("outlet_flux", SurfaceQuantity::Flux),
    ] {
        study.monitors.surfaces.insert(
            label.into(),
            SurfaceMonitor {
                boundary: "outlet".into(),
                quantity,
            },
        );
    }
    resolve(&study).unwrap()
}

pub fn compiled(study: &ResolvedStudy) -> CaseTree {
    backend()
        .compile(study, &CompileOptions::default())
        .unwrap()
}

pub fn text(tree: &CaseTree, name: &str) -> String {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path.to_str() == Some(name))
        .unwrap_or_else(|| panic!("the case has no {name}"));
    String::from_utf8(file.bytes.clone()).unwrap()
}

/// The sorted names of the files of a tree.
pub fn names(tree: &CaseTree) -> Vec<String> {
    let mut names: Vec<String> = tree
        .files
        .iter()
        .map(|f| f.rel_path.to_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

/// The tree with the text of one file replaced by `to_text(text)`.
pub fn rewritten(tree: &CaseTree, name: &str, to_text: impl Fn(&str) -> String) -> CaseTree {
    let original = text(tree, name);
    let files = tree
        .files
        .iter()
        .map(|f| {
            if f.rel_path.to_str() == Some(name) {
                CaseFile {
                    rel_path: f.rel_path.clone(),
                    bytes: to_text(&original).into_bytes(),
                }
            } else {
                f.clone()
            }
        })
        .collect();
    CaseTree { files }
}

/// The tree with `from` replaced by `to` in one file; the file must hold `from`.
pub fn edited(tree: &CaseTree, name: &str, from: &str, to: &str) -> CaseTree {
    assert!(text(tree, name).contains(from), "{name} has no {from}");
    rewritten(tree, name, |t| t.replacen(from, to, 1))
}

/// Whether reading the tree back finds the fact with this pointer wrong or lost, and nothing else
/// is reported as anything but an error.
pub fn lost(tree: &CaseTree, study: &ResolvedStudy, pointer_part: &str) -> bool {
    let found = backend().read_back(study, tree);
    !found.is_empty()
        && found
            .iter()
            .all(|d| d.severity == cfdcraft_model::diagnostics::Severity::Error)
        && found.iter().any(|d| d.pointer.contains(pointer_part))
}

/// Whether reading the tree back finds something wrong and every finding is an error.
pub fn broken(tree: &CaseTree, study: &ResolvedStudy) -> bool {
    let found = backend().read_back(study, tree);
    !found.is_empty() && found.iter().all(|d| d.severity == Severity::Error)
}

/// The first line of a file whose text starts with `key` after its indentation.
pub fn line_of(tree: &CaseTree, name: &str, key: &str) -> String {
    text(tree, name)
        .lines()
        .find(|l| l.trim_start().starts_with(key))
        .unwrap_or_else(|| panic!("{name} has no line starting with {key}"))
        .to_owned()
}

/// The number after `key` on its line of a file, as `key value;` is written.
pub fn number_after(tree: &CaseTree, name: &str, key: &str) -> f64 {
    let line = line_of(tree, name, &format!("{key} "));
    line.trim()
        .strip_prefix(&format!("{key} "))
        .and_then(|rest| rest.strip_suffix(';'))
        .and_then(|rest| rest.trim().parse().ok())
        .unwrap_or_else(|| panic!("{name}: {line} is not `{key} <number>;`"))
}

/// The block of a file that starts with the object name `object`: from its line to its closing
/// brace at the same indentation.
pub fn block_of(tree: &CaseTree, name: &str, object: &str) -> String {
    let all = text(tree, name);
    let start = all
        .find(&format!("    {object}\n    {{\n"))
        .unwrap_or_else(|| panic!("{name} has no object {object}"));
    let rest = &all[start..];
    let end = rest
        .find("\n    }\n")
        .unwrap_or_else(|| panic!("{name}: {object} is never closed"));
    rest[..end + "\n    }\n".len()].to_owned()
}

fn golden_root(folder: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(folder)
}

/// The files below a folder as `/`-joined names, without following links.
fn files_below(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for path in paths {
        let kind = std::fs::symlink_metadata(&path).unwrap().file_type();
        if kind.is_dir() {
            files_below(&path, base, out);
        } else if kind.is_file() {
            let rel = path.strip_prefix(base).unwrap();
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(parts.join("/"));
        }
    }
}

/// Every difference between the tree and the golden folder, as text that can be pasted in. Empty
/// means every file equals the golden file of its name and the folder holds nothing else.
/// Differences are collected, not raised, so one run prints every differing file. A golden file
/// that still holds the placeholder text is a difference.
pub fn golden_misses(folder: &str, tree: &CaseTree) -> Vec<String> {
    let dir = golden_root(folder);
    let mut misses = Vec::new();
    let mut golden_names = Vec::new();
    files_below(&dir, &dir, &mut golden_names);
    golden_names.sort();
    if golden_names != names(tree) {
        misses.push(format!(
            "{folder} holds {golden_names:?} but the case has {:?}",
            names(tree)
        ));
    }
    for file in &tree.files {
        let name = file.rel_path.to_str().unwrap();
        let actual = String::from_utf8_lossy(&file.bytes);
        let golden = std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        if actual != golden {
            misses.push(format!(
                "{folder}/{name} differs from its golden file; the actual text is:\n{actual}"
            ));
        }
    }
    misses
}

/// The study compiles to the same tree twice and to the files of its golden folder.
pub fn check_golden(folder: &str, study: &ResolvedStudy) {
    let first = compiled(study);
    assert_eq!(first, compiled(study), "{folder} is not deterministic");
    let misses = golden_misses(folder, &first);
    assert!(misses.is_empty(), "{}", misses.join("\n=====\n"));
}
