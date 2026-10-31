use cfdcraft_backend_esi::{CaseFile, CaseTree, CompileError, Dialect, compile, write_to_disk};
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn resolved() -> ResolvedStudy {
    resolve(
        &Study::from_json(include_str!(
            "../../cfdcraft-model/tests/fixtures/cavity_v0.json"
        ))
        .unwrap(),
    )
    .unwrap()
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
fn golden_case_has_exact_files_and_bytes() {
    let tree = compile(&resolved(), Dialect::EsiV2512).unwrap();
    assert_eq!(tree.files.len(), 8);
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cavity-v2512");
    let mut actual: Vec<_> = tree.files.iter().map(|f| f.rel_path.clone()).collect();
    actual.sort();
    assert_eq!(actual, walk(&golden, &golden));
    for f in &tree.files {
        assert_eq!(
            &f.bytes,
            &std::fs::read(golden.join(&f.rel_path)).unwrap(),
            "{}",
            f.rel_path.display()
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

#[test]
fn deterministic_and_physically_sensitive() {
    let mut s = resolved();
    let a = compile(&s, Dialect::EsiV2512).unwrap();
    assert_eq!(a, compile(&s, Dialect::EsiV2512).unwrap());
    s.lid_velocity *= 2.0;
    let b = compile(&s, Dialect::EsiV2512).unwrap();
    assert_ne!(a, b);
    let u = String::from_utf8(
        b.files
            .iter()
            .find(|f| f.rel_path == Path::new("0/U"))
            .unwrap()
            .bytes
            .clone(),
    )
    .unwrap();
    assert!(u.contains("(0.2 0 0)"));
    s.mesh.cells_z = 8;
    assert!(matches!(
        compile(&s, Dialect::EsiV2512),
        Err(CompileError::NotTwoDimensional)
    ));
    s = resolved();
    s.nu = f64::NAN;
    assert!(compile(&s, Dialect::EsiV2512).is_err());
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
        write_to_disk(
            &compile(&resolved(), Dialect::EsiV2512).unwrap(),
            Path::new(&dest),
        )
        .unwrap();
    }
}

#[test]
fn timezone_locale_and_working_directory_do_not_change_bytes() {
    let expected = compile(&resolved(), Dialect::EsiV2512).unwrap();
    for (tz, lang) in [("UTC", "C"), ("Asia/Tokyo", "ja_JP.UTF-8")] {
        let dest = scratch("environment");
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "environment_child"])
            .current_dir(&dest)
            .env("TZ", tz)
            .env("LANG", lang)
            .env("CFDCRAFT_GOLDEN_CHILD_DIR", &dest)
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
