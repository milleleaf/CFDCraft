use cfdcraft_backend_esi::{
    CaseFile, CaseTree, Dialect,
    case::{ContinueError, continue_case},
    compile,
};
use cfdcraft_model::{Study, resolve};
use std::path::Path;

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");

fn tree() -> CaseTree {
    compile(
        &resolve(&Study::from_json(CAVITY_V0).unwrap()).unwrap(),
        Dialect::EsiV2512,
    )
    .unwrap()
}
fn field(path: &str, body: &str) -> CaseFile {
    CaseFile {
        rel_path: path.into(),
        bytes: format!(
            "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class volVectorField;\n    object field;\n}}\n\ninternalField uniform {body};\n"
        )
        .into_bytes(),
    }
}
fn pair() -> Vec<CaseFile> {
    vec![field("1200/U", "(0 0 0)"), field("1200/p", "0")]
}
fn find<'a>(tree: &'a CaseTree, name: &str) -> Option<&'a CaseFile> {
    tree.files.iter().find(|f| f.rel_path == Path::new(name))
}
fn bad_seed(seed: Vec<CaseFile>) -> String {
    match continue_case(&tree(), &seed) {
        Err(ContinueError::BadSeed { path, .. }) => path,
        other => panic!("expected BadSeed, got {other:?}"),
    }
}

#[test]
fn continuing_changes_exactly_three_controldict_entries() {
    let original = tree();
    let continued = continue_case(&original, &pair()).unwrap();
    let before =
        String::from_utf8(find(&original, "system/controlDict").unwrap().bytes.clone()).unwrap();
    let after = String::from_utf8(
        find(&continued, "system/controlDict")
            .unwrap()
            .bytes
            .clone(),
    )
    .unwrap();
    assert!(before.contains("startFrom startTime;"));
    // A write interval is counted from the time a run starts at, so the first write must fall on
    // the end time (a real continued run once finished without its final time folder).
    assert!(before.contains("writeControl timeStep;"));
    assert!(before.contains("endTime 5000.0;"));
    assert_eq!(
        after,
        before
            .replace("startFrom startTime;", "startFrom latestTime;")
            .replacen("writeControl timeStep;", "writeControl runTime;", 1)
            .replacen("writeInterval 500.0;", "writeInterval 3800.0;", 1)
    );
    let (b, a): (Vec<_>, Vec<_>) = (before.lines().collect(), after.lines().collect());
    assert_eq!(b.len(), a.len());
    assert_eq!(b.iter().zip(&a).filter(|(x, y)| x != y).count(), 3);
}

#[test]
fn the_parents_fields_are_added_under_their_time_folder() {
    let original = tree();
    let seed = pair();
    let continued = continue_case(&original, &seed).unwrap();
    assert_eq!(continued.files.len(), original.files.len() + 2);
    for file in &seed {
        assert_eq!(
            find(&continued, file.rel_path.to_str().unwrap())
                .unwrap()
                .bytes,
            file.bytes
        );
    }
    for file in original
        .files
        .iter()
        .filter(|f| f.rel_path != Path::new("system/controlDict"))
    {
        assert_eq!(
            find(&continued, file.rel_path.to_str().unwrap()),
            Some(file)
        );
    }
}

#[test]
fn an_optional_flux_field_is_accepted() {
    let mut seed = pair();
    seed.push(field("1200/phi", "0"));
    let continued = continue_case(&tree(), &seed).unwrap();
    assert!(find(&continued, "1200/phi").is_some());
    let names: Vec<_> = continued.files.iter().rev().take(3).collect();
    assert_eq!(names[0].rel_path, Path::new("1200/phi"));
}

#[test]
fn a_seed_name_outside_the_pattern_is_refused() {
    for path in [
        "../1200/U",
        "1200/../U",
        "abc/U",
        "1200/other",
        "1200/U/extra",
        "/1200/U",
        "0/U",
        "-5/U",
        "1.2.3/U",
        "U",
    ] {
        assert_eq!(bad_seed(vec![field(path, "(0 0 0)")]), path);
    }
}

#[test]
fn two_seed_time_folders_are_refused() {
    let mut seed = pair();
    seed.push(field("1300/U", "(0 0 0)"));
    bad_seed(seed);
    bad_seed(vec![field("1200/U", "(0 0 0)"), field("1300/p", "0")]);
}

#[test]
fn a_seed_that_the_safety_scan_refuses_is_refused() {
    let original = tree();
    let before = original.clone();
    let mut seed = pair();
    seed[0].bytes = b"FoamFile\n{\n}\n\n#include \"/etc/passwd\"\n".to_vec();
    match continue_case(&original, &seed) {
        Err(ContinueError::Guard { rule, file, line }) => {
            assert!(rule.starts_with("CFD-GUARD-"));
            assert_eq!(file, "1200/U");
            assert_eq!(line, 5);
        }
        other => panic!("expected Guard, got {other:?}"),
    }
    assert_eq!(original, before);
}

#[test]
fn a_tree_without_a_controldict_is_refused() {
    let mut original = tree();
    original
        .files
        .retain(|f| f.rel_path != Path::new("system/controlDict"));
    assert_eq!(
        continue_case(&original, &pair()),
        Err(ContinueError::NoControlDict)
    );
}

#[test]
fn a_seed_over_the_size_limit_is_refused() {
    let mut seed = pair();
    seed[0].bytes = vec![b' '; 17 * 1024 * 1024];
    assert_eq!(bad_seed(seed), "1200/U");
    let mut at_limit = pair();
    at_limit[1].bytes = vec![b' '; 16 * 1024 * 1024];
    assert!(continue_case(&tree(), &at_limit).is_ok());
}

#[test]
fn a_seed_missing_a_required_field_or_repeating_one_is_refused() {
    bad_seed(Vec::new());
    bad_seed(vec![field("1200/U", "(0 0 0)")]);
    bad_seed(vec![field("1200/p", "0"), field("1200/phi", "0")]);
    let mut twice = pair();
    twice.push(field("1200/U", "(1 0 0)"));
    bad_seed(twice);
}

#[test]
fn the_input_tree_is_never_changed() {
    let original = tree();
    let before = original.clone();
    let _ = continue_case(&original, &pair()).unwrap();
    assert_eq!(original, before);
    let _ = continue_case(&original, &[field("0/U", "(0 0 0)")]);
    assert_eq!(original, before);
}
