use cfdcraft_backend_esi::{CaseFile, CaseTree, CompileError, Dialect, compile, write_to_disk};
use cfdcraft_model::{Face, ResolvedStudy, Study, resolve};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");
/// (golden directory, study json)
const CASES: [(&str, &str); 2] = [("cavity-v2512", CAVITY_V0), ("channel-v2512", CHANNEL_V1)];

fn resolve_json(json: &str) -> ResolvedStudy {
    resolve(&Study::from_json(json).unwrap()).unwrap()
}
fn resolved() -> ResolvedStudy {
    resolve_json(CAVITY_V0)
}
fn file<'a>(tree: &'a CaseTree, name: &str) -> &'a [u8] {
    &tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap()
        .bytes
}
/// Compares written bytes with a golden file; a difference prints the whole actual text so it can
/// be pasted into the golden file as it is.
fn assert_same(name: &str, actual: &[u8], golden: &[u8]) {
    assert!(
        actual == golden,
        "{name} differs from its golden file; the actual text is:\n{}",
        String::from_utf8_lossy(actual)
    );
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}
fn scratch(label: &str) -> PathBuf {
    let p = root().join("target").join(format!(
        "backend-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn walk(p: &Path, base: &Path) -> Vec<PathBuf> {
    let mut all = vec![];
    for entry in std::fs::read_dir(p).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            all.extend(walk(&p, base));
        } else {
            all.push(p.strip_prefix(base).unwrap().to_owned());
        }
    }
    all.sort();
    all
}

#[test]
fn schema_0_and_schema_1_cavity_compile_to_the_same_golden() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cavity-v2512");
    for json in [CAVITY_V0, CAVITY_V1] {
        let tree = compile(&resolve_json(json), Dialect::EsiV2512).unwrap();
        assert_eq!(tree.files.len(), 8);
        for f in &tree.files {
            assert_same(
                &f.rel_path.display().to_string(),
                &f.bytes,
                &std::fs::read(golden.join(&f.rel_path)).unwrap(),
            );
        }
    }
}

#[test]
fn renaming_boundaries_changes_no_byte() {
    let base = compile(&resolved(), Dialect::EsiV2512).unwrap();
    let mut s = resolved();
    let lid = s.boundary_conditions.remove("lid").unwrap();
    let walls = s.boundary_conditions.remove("walls").unwrap();
    s.boundary_conditions.insert("top".into(), lid);
    s.boundary_conditions.insert("rest".into(), walls);
    assert_eq!(base, compile(&s, Dialect::EsiV2512).unwrap());
}

#[test]
fn malformed_boundary_sets_are_refused() {
    let invalid =
        |s: &ResolvedStudy| matches!(compile(s, Dialect::EsiV2512), Err(CompileError::Invalid(_)));
    let mut unassigned = resolved();
    unassigned
        .boundary_conditions
        .get_mut("walls")
        .unwrap()
        .faces = vec![Face::XMin, Face::XMax];
    assert!(invalid(&unassigned));
    let mut twice = resolved();
    twice
        .boundary_conditions
        .get_mut("walls")
        .unwrap()
        .faces
        .push(Face::YMax);
    assert!(invalid(&twice));
    // Two moving walls whose faces call for different vectors cannot share one patch.
    let mut split = resolved();
    let mut second = split.boundary_conditions["lid"].clone();
    second.faces = vec![Face::XMin];
    split.boundary_conditions.get_mut("walls").unwrap().faces = vec![Face::XMax, Face::YMin];
    split.boundary_conditions.insert("side".into(), second);
    assert!(invalid(&split));
}

#[test]
fn golden_cases_have_exact_files_and_bytes() {
    for (dir, json) in CASES {
        let tree = compile(&resolve_json(json), Dialect::EsiV2512).unwrap();
        assert_eq!(tree.files.len(), 8);
        let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(dir);
        let mut actual: Vec<_> = tree.files.iter().map(|f| f.rel_path.clone()).collect();
        actual.sort();
        assert_eq!(actual, walk(&golden, &golden), "{dir}");
        for f in &tree.files {
            assert_same(
                &format!("{dir}/{}", f.rel_path.display()),
                &f.bytes,
                &std::fs::read(golden.join(&f.rel_path)).unwrap(),
            );
            assert!(!String::from_utf8_lossy(&f.bytes).contains("2026-"));
        }
        let dest = scratch("disk");
        write_to_disk(&tree, &dest).unwrap();
        assert_eq!(actual, walk(&dest, &dest));
        for f in &tree.files {
            assert_eq!(f.bytes, std::fs::read(dest.join(&f.rel_path)).unwrap());
        }
        std::fs::remove_dir_all(dest).unwrap();
    }
}

#[test]
fn deterministic_and_physically_sensitive() {
    for (dir, json) in CASES {
        let mut s = resolve_json(json);
        let a = compile(&s, Dialect::EsiV2512).unwrap();
        assert_eq!(a, compile(&s, Dialect::EsiV2512).unwrap(), "{dir}");
        // Doubling the one driven boundary's speed (0.1 to 0.2) changes 0/U and nothing else.
        let driven = s
            .boundary_conditions
            .values_mut()
            .find(|b| b.velocity.is_some())
            .unwrap();
        driven.velocity = Some(0.2);
        let b = compile(&s, Dialect::EsiV2512).unwrap();
        assert!(String::from_utf8_lossy(file(&b, "0/U")).contains("(0.2 0 0)"));
        for (x, y) in a.files.iter().zip(&b.files) {
            assert_eq!(x.bytes == y.bytes, x.rel_path != Path::new("0/U"), "{dir}");
        }
        s.mesh.cells_z = 8;
        assert!(matches!(
            compile(&s, Dialect::EsiV2512),
            Err(CompileError::NotTwoDimensional)
        ));
        s = resolve_json(json);
        s.fluid.kinematic_viscosity = f64::NAN;
        assert!(compile(&s, Dialect::EsiV2512).is_err());
    }
}

#[test]
fn the_channel_has_its_five_patches_in_the_fixed_order() {
    let tree = compile(&resolve_json(CHANNEL_V1), Dialect::EsiV2512).unwrap();
    let mesh = String::from_utf8(file(&tree, "system/blockMeshDict").to_vec()).unwrap();
    let boundary = mesh.split("boundary\n(").nth(1).unwrap();
    let names: Vec<_> = boundary
        .lines()
        .filter(|l| l.starts_with("    ") && !l.starts_with("     "))
        .map(str::trim)
        .filter(|l| !l.starts_with('{') && !l.starts_with('}'))
        .collect();
    assert_eq!(
        names,
        ["inlet", "outlet", "fixedWalls", "symmetry", "frontAndBack"]
    );
}

#[test]
fn open_and_closed_differ_only_in_the_pressure_reference() {
    let open = resolve_json(CHANNEL_V1);
    let mut closed = open.clone();
    closed.domain.open = false;
    let (a, b) = (
        compile(&open, Dialect::EsiV2512).unwrap(),
        compile(&closed, Dialect::EsiV2512).unwrap(),
    );
    for (x, y) in a.files.iter().zip(&b.files) {
        assert_eq!(
            x.bytes == y.bytes,
            x.rel_path != Path::new("system/fvSolution")
        );
    }
    assert!(!String::from_utf8_lossy(file(&a, "system/fvSolution")).contains("pRefCell"));
    assert!(String::from_utf8_lossy(file(&b, "system/fvSolution")).contains("pRefCell 0;"));
}

#[test]
fn rejects_traversal_when_writing_a_case() {
    let tree = CaseTree {
        files: vec![CaseFile {
            rel_path: "../escape".into(),
            bytes: vec![],
        }],
    };
    assert!(write_to_disk(&tree, &scratch("traversal")).is_err());
}

#[test]
fn environment_child() {
    if let Some(dest) = std::env::var_os("CFDCRAFT_GOLDEN_CHILD_DIR") {
        let (_, json) = CASES[std::env::var("CFDCRAFT_GOLDEN_CHILD_CASE")
            .unwrap()
            .parse::<usize>()
            .unwrap()];
        write_to_disk(
            &compile(&resolve_json(json), Dialect::EsiV2512).unwrap(),
            Path::new(&dest),
        )
        .unwrap();
    }
}

#[test]
fn timezone_locale_and_working_directory_do_not_change_bytes() {
    for (index, (_, json)) in CASES.iter().enumerate() {
        let expected = compile(&resolve_json(json), Dialect::EsiV2512).unwrap();
        for (tz, lang) in [("UTC", "C"), ("Asia/Tokyo", "ja_JP.UTF-8")] {
            let dest = scratch("environment");
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "environment_child"])
                .current_dir(&dest)
                .env("TZ", tz)
                .env("LANG", lang)
                .env("CFDCRAFT_GOLDEN_CHILD_DIR", &dest)
                .env("CFDCRAFT_GOLDEN_CHILD_CASE", index.to_string())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            for f in &expected.files {
                assert_eq!(f.bytes, std::fs::read(dest.join(&f.rel_path)).unwrap());
            }
            std::fs::remove_dir_all(dest).unwrap();
        }
    }
}
