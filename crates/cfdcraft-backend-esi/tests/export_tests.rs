use cfdcraft_backend_esi::{CaseTree, Dialect, compile, dialect::IMAGE, export::export_tree};
use cfdcraft_model::{Study, canonical_hash, resolve};
use std::path::Path;

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");

fn golden(dir: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(dir)
}
fn cavity() -> (CaseTree, String) {
    let study = resolve(&Study::from_json(CAVITY_V0).unwrap()).unwrap();
    (
        compile(&study, Dialect::EsiV2512).unwrap(),
        canonical_hash(&study),
    )
}

#[test]
fn the_cavity_export_is_the_case_golden_plus_a_golden_allrun_and_readme() {
    let (tree, hash) = cavity();
    let exported = export_tree(&tree, &hash);
    assert_eq!(exported.files.len(), 10);
    let case_golden = golden("cavity-v2512");
    for (file, original) in exported.files.iter().zip(&tree.files) {
        assert_eq!(file.rel_path, original.rel_path);
        assert_eq!(
            file.bytes,
            std::fs::read(case_golden.join(&file.rel_path)).unwrap()
        );
    }
    let export_golden = golden("cavity-v2512-export");
    assert_eq!(exported.files[8].rel_path, Path::new("Allrun"));
    assert_eq!(exported.files[9].rel_path, Path::new("README.md"));
    for file in &exported.files[8..] {
        assert_eq!(
            file.bytes,
            std::fs::read(export_golden.join(&file.rel_path)).unwrap(),
            "{}",
            file.rel_path.display()
        );
    }
}

#[test]
fn the_readme_names_the_study_hash_and_the_image() {
    let (tree, hash) = cavity();
    let exported = export_tree(&tree, &hash);
    let readme = String::from_utf8(exported.files[9].bytes.clone()).unwrap();
    assert!(readme.contains(&hash));
    assert!(readme.contains(IMAGE));
}

#[test]
fn exporting_does_not_change_the_dictionary_files() {
    let (tree, hash) = cavity();
    let before = tree.clone();
    let _ = export_tree(&tree, &hash);
    assert_eq!(tree, before);
}

const CHANNEL_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn text(tree: &CaseTree, name: &str) -> String {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    String::from_utf8(file.bytes.clone()).unwrap()
}

#[test]
fn allrun_holds_only_the_four_allowed_lines() {
    let (tree, hash) = cavity();
    let allrun = text(&export_tree(&tree, &hash), "Allrun");
    let allowed = [
        "cd \"$(dirname \"$0\")\" || exit 1",
        "blockMesh > log.blockMesh 2>&1 || exit 1",
        "checkMesh > log.checkMesh 2>&1 || exit 1",
        "simpleFoam > log.simpleFoam 2>&1 || exit 1",
    ];
    let lines: Vec<_> = allrun
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    assert_eq!(lines, allowed);
    assert!(allrun.starts_with("#!/bin/sh\n"));
}

#[test]
fn the_readme_says_what_a_reader_needs_to_run_and_judge_the_case() {
    let (tree, hash) = cavity();
    let readme = text(&export_tree(&tree, &hash), "README.md");
    for needle in [
        "CFDCraft is not approved or endorsed by OpenCFD Limited. OPENFOAM® and OpenCFD® are registered trade marks of OpenCFD Limited.",
        "Judge the result by the logs, not the exit status",
        "--network none",
        "cd /w && sh ./Allrun",
    ] {
        assert!(readme.contains(needle), "{needle}");
    }
    let digest = IMAGE.split('@').nth(1).unwrap();
    let pull = readme
        .lines()
        .find(|l| l.starts_with("docker pull "))
        .unwrap();
    let run = readme
        .lines()
        .find(|l| l.starts_with("docker run "))
        .unwrap();
    assert!(pull.contains(digest) && run.contains(digest));
}

#[test]
fn the_readme_is_one_line_per_paragraph_and_has_no_dates_or_versions() {
    let (tree, hash) = cavity();
    let readme = text(&export_tree(&tree, &hash), "README.md");
    let mut in_code = false;
    let mut previous_was_text = false;
    for line in readme.lines() {
        if line.starts_with("```") {
            in_code = !in_code;
            previous_was_text = false;
            continue;
        }
        if in_code {
            continue;
        }
        assert!(
            !(previous_was_text && !line.is_empty()),
            "hard-wrapped paragraph at: {line}"
        );
        previous_was_text = !line.is_empty();
    }
    assert!(!readme.contains("2026") && !readme.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn the_channel_export_differs_from_the_cavity_only_in_case_files_and_study_hash() {
    let (cavity_tree, cavity_hash) = cavity();
    let study = resolve(&Study::from_json(CHANNEL_V1).unwrap()).unwrap();
    let channel_hash = canonical_hash(&study);
    let tree = compile(&study, Dialect::EsiV2512).unwrap();
    let exported = export_tree(&tree, &channel_hash);
    let golden = golden("channel-v2512");
    assert_eq!(exported.files.len(), 10);
    for file in &exported.files[..8] {
        assert_eq!(
            file.bytes,
            std::fs::read(golden.join(&file.rel_path)).unwrap()
        );
    }
    let cavity_export = export_tree(&cavity_tree, &cavity_hash);
    assert_eq!(text(&exported, "Allrun"), text(&cavity_export, "Allrun"));
    assert_eq!(
        text(&exported, "README.md"),
        text(&cavity_export, "README.md").replace(&cavity_hash, &channel_hash)
    );
}

#[test]
fn export_child() {
    if let Some(dest) = std::env::var_os("CFDCRAFT_EXPORT_CHILD_DIR") {
        let (tree, hash) = cavity();
        for file in &export_tree(&tree, &hash).files[8..] {
            std::fs::write(Path::new(&dest).join(&file.rel_path), &file.bytes).unwrap();
        }
    }
}

#[test]
fn timezone_and_locale_do_not_change_the_exported_bytes() {
    let (tree, hash) = cavity();
    let expected = export_tree(&tree, &hash);
    for (tz, lang) in [("UTC", "C"), ("Asia/Tokyo", "ja_JP.UTF-8")] {
        let dest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("export-env-{}-{tz}", std::process::id()).replace('/', "-"));
        std::fs::create_dir_all(&dest).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "export_child"])
            .current_dir(&dest)
            .env("TZ", tz)
            .env("LANG", lang)
            .env("CFDCRAFT_EXPORT_CHILD_DIR", &dest)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for file in &expected.files[8..] {
            assert_eq!(
                file.bytes,
                std::fs::read(dest.join(&file.rel_path)).unwrap()
            );
        }
        std::fs::remove_dir_all(dest).unwrap();
    }
}
