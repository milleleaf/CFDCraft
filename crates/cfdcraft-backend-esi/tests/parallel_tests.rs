use cfdcraft_backend_esi::{
    CaseTree, CompileError, Dialect,
    case::{CaseOptions, compile_with, np_for},
    compile,
    export::{allrun_script, allrun_script_for, export_tree, export_tree_for, readme_text},
    guard::scan_tree,
};
use cfdcraft_model::{ResolvedStudy, Study, canonical_hash, resolve};
use std::path::Path;

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");

fn study() -> ResolvedStudy {
    resolve(&Study::from_json(CAVITY_V0).unwrap()).unwrap()
}
fn tree(np: u32) -> CaseTree {
    compile_with(&study(), Dialect::EsiV2512, &CaseOptions { np }).unwrap()
}
fn golden(dir: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(dir)
}
fn text(tree: &CaseTree, name: &str) -> String {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    String::from_utf8(file.bytes.clone()).unwrap()
}

#[test]
fn one_process_is_byte_identical_to_today() {
    let study = study();
    let plain = compile(&study, Dialect::EsiV2512).unwrap();
    let one = tree(1);
    assert_eq!(one, plain);
    assert_eq!(CaseOptions::default(), CaseOptions { np: 1 });
    let case_golden = golden("cavity-v2512");
    for file in &one.files {
        assert_eq!(
            file.bytes,
            std::fs::read(case_golden.join(&file.rel_path)).unwrap()
        );
    }
    let hash = canonical_hash(&study);
    let exported = export_tree_for(&one, &hash, 1);
    assert_eq!(exported, export_tree(&plain, &hash));
    assert_eq!(allrun_script_for(1), allrun_script());
    let export_golden = golden("cavity-v2512-export");
    for file in &exported.files[8..] {
        assert_eq!(
            file.bytes,
            std::fs::read(export_golden.join(&file.rel_path)).unwrap()
        );
    }
}

#[test]
fn two_processes_add_a_scotch_decomposition_dictionary() {
    let (one, two) = (tree(1), tree(2));
    assert_eq!(two.files.len(), one.files.len() + 1);
    let last = two.files.last().unwrap();
    assert_eq!(last.rel_path, Path::new("system/decomposeParDict"));
    assert_eq!(&two.files[..one.files.len()], &one.files[..]);
    let dict = text(&two, "system/decomposeParDict");
    assert!(dict.starts_with("FoamFile\n{"));
    assert!(dict.contains("numberOfSubdomains 2;"));
    assert!(dict.contains("method scotch;"));
    assert!(scan_tree(&two).is_empty());
}

#[test]
fn the_parallel_allrun_decomposes_runs_under_mpirun_and_reconstructs() {
    let serial = allrun_script_for(1);
    let parallel = allrun_script_for(2);
    let lines: Vec<&str> = parallel.lines().collect();
    assert_eq!(
        lines,
        [
            "#!/bin/sh",
            "cd \"$(dirname \"$0\")\" || exit 1",
            "blockMesh > log.blockMesh 2>&1 || exit 1",
            "checkMesh > log.checkMesh 2>&1 || exit 1",
            "decomposePar -force > log.decomposePar 2>&1 || exit 1",
            "mpirun -np 2 simpleFoam -parallel > log.simpleFoam 2>&1 || exit 1",
            "reconstructPar -latestTime > log.reconstructPar 2>&1 || exit 1",
        ]
    );
    let head: Vec<&str> = serial.lines().take(3).collect();
    assert_eq!(&lines[..3], &head[..]);
    assert!(!serial.contains("mpirun"));
}

#[test]
fn np_follows_the_cap_and_the_cell_count() {
    assert_eq!(np_for(16384, 2), 1);
    assert_eq!(np_for(40000, 2), 2);
    assert_eq!(np_for(100000, 4), 4);
    assert_eq!(np_for(100000, 1), 1);
    assert_eq!(np_for(0, 4), 1);
    assert_eq!(np_for(u64::MAX, 2), 2);
    assert_eq!(np_for(39999, 8), 1);
    assert_eq!(np_for(1_000_000, 0), 1);
}

#[test]
fn np_zero_is_refused_by_the_options() {
    let r = compile_with(&study(), Dialect::EsiV2512, &CaseOptions { np: 0 });
    assert!(matches!(r, Err(CompileError::Invalid(_))));
}

#[test]
fn the_readme_for_a_parallel_case_names_the_cpu_cap() {
    let study = study();
    let hash = canonical_hash(&study);
    let two = text(&export_tree_for(&tree(2), &hash, 2), "README.md");
    assert!(two.contains("the container needs `--cpus 2`"));
    assert!(two.contains(&hash));
    let one = text(&export_tree_for(&tree(1), &hash, 1), "README.md");
    assert_eq!(one, readme_text(&hash));
    assert!(!one.contains("parallel run"));
}
