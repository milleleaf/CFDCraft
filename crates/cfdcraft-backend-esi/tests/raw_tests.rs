//! Raw overrides are applied to the files the writer builds: an entry takes the new value or is
//! added, nothing else changes, and every way an override cannot be applied is an error.

use cfdcraft_backend_esi::{CaseTree, CompileError, Dialect, compile};
use cfdcraft_model::{RawBackend, RawOverride, ResolvedStudy, Study, resolve};
use std::path::Path;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn cavity() -> ResolvedStudy {
    resolve(&Study::from_json(CAVITY).unwrap()).unwrap()
}

fn raw(backend: RawBackend, file: &str, entry: &str, value: &str) -> RawOverride {
    RawOverride {
        backend,
        file: file.to_owned(),
        entry: entry.to_owned(),
        value: value.to_owned(),
    }
}

fn esi(file: &str, entry: &str, value: &str) -> RawOverride {
    raw(RawBackend::Esi, file, entry, value)
}

fn with(overrides: &[(&str, RawOverride)]) -> ResolvedStudy {
    let mut study = cavity();
    for (label, item) in overrides {
        study
            .raw_overrides
            .insert((*label).to_owned(), item.clone());
    }
    study
}

fn built(study: &ResolvedStudy) -> CaseTree {
    compile(study, Dialect::EsiV2512).unwrap()
}

fn refused(study: &ResolvedStudy) -> (String, String) {
    match compile(study, Dialect::EsiV2512) {
        Err(CompileError::Raw { label, reason }) => (label, reason),
        other => panic!("expected a refused override, found {other:?}"),
    }
}

fn text<'a>(tree: &'a CaseTree, name: &str) -> &'a str {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    std::str::from_utf8(&file.bytes).unwrap()
}

/// The names of the files whose bytes differ between two trees; the lists must be the same.
fn changed(a: &CaseTree, b: &CaseTree) -> Vec<String> {
    let names = |t: &CaseTree| {
        t.files
            .iter()
            .map(|f| f.rel_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(a), names(b), "the list of files changed");
    a.files
        .iter()
        .zip(&b.files)
        .filter(|(x, y)| x.bytes != y.bytes)
        .map(|(x, _)| x.rel_path.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn an_override_replaces_an_entry_in_the_written_file() {
    let plain = built(&cavity());
    let tree = built(&with(&[(
        "relax-u",
        esi("system/fvSolution", "relaxationFactors/equations/U", "0.5"),
    )]));
    assert!(
        text(&tree, "system/fvSolution").contains("equations { U 0.5; }"),
        "{}",
        text(&tree, "system/fvSolution")
    );
    assert!(!text(&tree, "system/fvSolution").contains("U 0.7"));
    assert_eq!(changed(&plain, &tree), vec!["system/fvSolution"]);
}

#[test]
fn an_override_adds_a_missing_entry() {
    let plain = built(&cavity());
    let tree = built(&with(&[(
        "predictor",
        esi("system/fvSolution", "SIMPLE/momentumPredictor", "no"),
    )]));
    let solution = text(&tree, "system/fvSolution");
    let simple = &solution[solution.find("SIMPLE").unwrap()..];
    let block = &simple[..simple.find('}').unwrap()];
    assert!(block.contains("momentumPredictor no;"), "{solution}");
    assert!(block.contains("consistent no;"), "the other entries stay");
    assert_eq!(changed(&plain, &tree), vec!["system/fvSolution"]);
}

#[test]
fn a_mesh_dictionary_override_edits_the_block_mesh_file() {
    let plain = built(&cavity());
    let tree = built(&with(&[(
        "merge",
        esi("system/blockMeshDict", "mergePatchPairs", "( )"),
    )]));
    // The writer already lists the entry, so its value is replaced.
    assert!(
        text(&tree, "system/blockMeshDict").ends_with("mergePatchPairs ( );\n"),
        "{}",
        text(&tree, "system/blockMeshDict")
    );
    assert_eq!(changed(&plain, &tree), vec!["system/blockMeshDict"]);
}

#[test]
fn a_file_the_case_does_not_have_is_refused() {
    let (label, reason) = refused(&with(&[("extra", esi("system/fvOptions", "a/b", "1"))]));
    assert_eq!(label, "extra");
    assert!(reason.contains("system/fvOptions"), "{reason}");
}

#[test]
fn a_block_the_file_does_not_have_is_refused() {
    let (label, reason) = refused(&with(&[(
        "nowhere",
        esi("system/fvSolution", "nope/x", "1"),
    )]));
    assert_eq!(label, "nowhere");
    assert!(reason.contains("nope/x"), "{reason}");
}

#[test]
fn an_override_tagged_for_another_backend_is_refused() {
    let (label, reason) = refused(&with(&[(
        "other",
        raw(
            RawBackend::Foundation,
            "system/fvSolution",
            "SIMPLE/consistent",
            "yes",
        ),
    )]));
    assert_eq!(label, "other");
    assert!(reason.contains("foundation"), "{reason}");
}

#[test]
fn two_overrides_of_one_entry_are_refused() {
    let (label, reason) = refused(&with(&[
        (
            "a-first",
            esi("system/fvSolution", "relaxationFactors/equations/U", "0.5"),
        ),
        (
            "b-second",
            esi("system/fvSolution", "relaxationFactors/equations/U", "0.6"),
        ),
    ]));
    assert_eq!(label, "b-second");
    assert!(reason.contains("already"), "{reason}");
}

#[test]
fn overrides_are_applied_in_label_order_and_compile_is_deterministic() {
    let u = esi("system/fvSolution", "relaxationFactors/equations/U", "0.5");
    let p = esi("system/fvSolution", "relaxationFactors/fields/p", "0.2");
    let one = with(&[("a", u.clone()), ("b", p.clone())]);
    assert_eq!(built(&one), built(&one));
    // The same two overrides under swapped labels give the same case.
    let swapped = with(&[("a", p), ("b", u)]);
    assert_eq!(built(&one), built(&swapped));
    let solution = text(&built(&one), "system/fvSolution").to_owned();
    assert!(solution.contains("fields { p 0.2; }"), "{solution}");
    assert!(solution.contains("equations { U 0.5; }"), "{solution}");
}

#[test]
fn a_study_without_overrides_compiles_as_before() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cavity-v2512");
    let tree = built(&cavity());
    assert_eq!(tree.files.len(), 8);
    for file in &tree.files {
        assert_eq!(
            file.bytes,
            std::fs::read(golden.join(&file.rel_path)).unwrap(),
            "{}",
            file.rel_path.display()
        );
    }
}
