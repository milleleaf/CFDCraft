use std::path::{Path, PathBuf};

/// Low-level calls that belong to the shared crash-safe save, and archive extraction, which
/// trusts the entry names inside an archive. Production sources must not call them directly.
const FORBIDDEN: &[&str] = &[
    "hard_link(",
    "sync_all(",
    "sync_data(",
    "craft_tmp",
    "rename(",
    ".extract(",
];

/// What to do instead of a forbidden token.
fn advice(token: &str) -> &'static str {
    if token == ".extract(" {
        "read archive entries by name through a size-capped reader and never use an entry name as a path"
    } else {
        "take crash-safe saves from the shared Craft atomic-save crate"
    }
}

/// (path, token, reason): the one file allowed to use that token. An entry that no longer
/// matches code fails the gate, so this list can only shrink.
const ALLOWED: &[(&str, &str, &str)] = &[(
    "crates/cfdcraft-ops/src/workspace.rs",
    "rename(",
    "swaps a whole output directory; the shared crate replaces single files only, so there is no equivalent to use",
)];

/// Refuse in-house copies of the shared crash-safe save.
pub fn check_equivalents(root: &Path) -> Result<(), String> {
    check_with(root, ALLOWED)
}

fn check_with(root: &Path, allowed: &[(&str, &str, &str)]) -> Result<(), String> {
    let mut files = Vec::new();
    let crates = root.join("crates");
    let entries =
        std::fs::read_dir(&crates).map_err(|e| format!("cannot list {}: {e}", crates.display()))?;
    for entry in entries {
        let src = entry.map_err(|e| e.to_string())?.path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files)?;
        }
    }
    files.sort();
    let mut seen = Vec::new();
    let mut problems = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for token in FORBIDDEN.iter().filter(|t| line.contains(**t)) {
                if allowed.iter().any(|(p, t, _)| *p == rel && t == token) {
                    seen.push((rel.clone(), *token));
                } else {
                    problems.push(format!(
                        "EQUIVALENT: {rel}:{} uses {token}; {}",
                        n + 1,
                        advice(token)
                    ));
                }
            }
        }
    }
    for (path, token, _) in allowed {
        if !seen.iter().any(|(p, t)| p == path && t == token) {
            problems.push(format!("STALE-ALLOW: {path} {token}"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        std::fs::read_dir(dir).map_err(|e| format!("cannot list {}: {e}", dir.display()))?
    {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tree(files: &[(&str, &str)]) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/governance-tests")
            .join(format!(
                "equivalents-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        for (path, body) in files {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, body).unwrap();
        }
        root
    }

    #[test]
    fn primitives_in_production_sources_fail_with_file_line_and_token() {
        for token in ["hard_link(", "sync_all(", "sync_data(", "rename("] {
            let root = tree(&[(
                "crates/x/src/lib.rs",
                &format!("fn a() {{}}\nfn b() {{ f.{token}); }}\n"),
            )]);
            let error = check_with(&root, &[]).unwrap_err();
            assert!(
                error.contains(&format!("EQUIVALENT: crates/x/src/lib.rs:2 uses {token}")),
                "{error}"
            );
        }
        let root = tree(&[("crates/x/src/lib.rs", "let craft_tmp = 1;\n")]);
        assert!(
            check_with(&root, &[])
                .unwrap_err()
                .contains("uses craft_tmp")
        );
    }

    #[test]
    fn archive_extraction_in_production_sources_fails_with_its_own_advice() {
        let root = tree(&[(
            "crates/x/src/lib.rs",
            "fn a() {}\nfn b() { archive.extract(dir); }\n",
        )]);
        let error = check_with(&root, &[]).unwrap_err();
        assert!(
            error.contains("EQUIVALENT: crates/x/src/lib.rs:2 uses .extract("),
            "{error}"
        );
        assert!(error.contains("size-capped reader"), "{error}");
        assert!(!error.contains("atomic-save"), "{error}");

        let root = tree(&[
            ("crates/x/src/lib.rs", "// archive.extract(dir) is banned\n"),
            ("crates/x/tests/t.rs", "fn t() { archive.extract(dir); }\n"),
        ]);
        assert!(check_with(&root, &[]).is_ok());
    }

    #[test]
    fn comments_tests_and_examples_are_not_scanned() {
        let root = tree(&[
            (
                "crates/x/src/lib.rs",
                "// calls rename( here\n    /// and sync_all( there\n",
            ),
            ("crates/x/tests/t.rs", "fn t() { std::fs::rename(a, b); }\n"),
            ("crates/x/examples/e.rs", "fn e() { f.sync_all(); }\n"),
        ]);
        assert!(check_with(&root, &[]).is_ok());
    }

    #[test]
    fn an_allowlist_entry_covers_that_file_and_token_only() {
        let allow = [("crates/x/src/lib.rs", "rename(", "reason")];
        let root = tree(&[("crates/x/src/lib.rs", "fn a() { fs::rename(a, b); }\n")]);
        assert!(check_with(&root, &allow).is_ok());
        let root = tree(&[(
            "crates/x/src/lib.rs",
            "fn a() { fs::rename(a, b); f.sync_all(); }\n",
        )]);
        assert!(
            check_with(&root, &allow)
                .unwrap_err()
                .contains("uses sync_all(")
        );
        let root = tree(&[
            ("crates/x/src/lib.rs", "fn a() { fs::rename(a, b); }\n"),
            ("crates/x/src/other.rs", "fn a() { fs::rename(a, b); }\n"),
        ]);
        assert!(
            check_with(&root, &allow)
                .unwrap_err()
                .contains("crates/x/src/other.rs:1")
        );
    }

    #[test]
    fn an_entry_that_no_longer_matches_code_is_stale() {
        let allow = [("crates/x/src/lib.rs", "rename(", "reason")];
        let root = tree(&[("crates/x/src/lib.rs", "fn a() {}\n")]);
        assert!(
            check_with(&root, &allow)
                .unwrap_err()
                .contains("STALE-ALLOW: crates/x/src/lib.rs rename(")
        );
        let root = tree(&[("crates/x/src/other.rs", "fn a() {}\n")]);
        assert!(
            check_with(&root, &allow)
                .unwrap_err()
                .contains("STALE-ALLOW")
        );
    }

    #[test]
    fn the_real_tree_passes_with_the_shipped_allowlist() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        assert_eq!(check_equivalents(&root), Ok(()));
    }
}
