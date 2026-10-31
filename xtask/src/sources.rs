use std::path::Path;

use toml::Value;

/// Dependencies must come from the registry and the lockfile must match the
/// manifests. Rejects git sources in `Cargo.lock` and any source replacement,
/// `[patch]`, `[replace]` or `paths` override in `.cargo/config(.toml)` or the
/// workspace manifest, none of which show up in the lockfile itself.
pub fn check_sources(root: &Path) -> Result<(), String> {
    let mut violations = Vec::new();

    let lock_path = root.join("Cargo.lock");
    match std::fs::read_to_string(&lock_path) {
        Err(_) => violations.push("Cargo.lock is missing".to_string()),
        Ok(text) => match text.parse::<toml::Table>() {
            Err(e) => violations.push(format!("Cargo.lock is unreadable: {e}")),
            Ok(lock) => {
                let packages = lock.get("package").and_then(Value::as_array);
                for pkg in packages.into_iter().flatten() {
                    let source = pkg.get("source").and_then(Value::as_str).unwrap_or("");
                    if source.starts_with("git+") {
                        let name = pkg.get("name").and_then(Value::as_str).unwrap_or("?");
                        violations.push(format!("GIT-SOURCE: {name} comes from {source}"));
                    }
                }
            }
        },
    }

    for name in [".cargo/config", ".cargo/config.toml"] {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        match std::fs::read_to_string(&path).map(|t| t.parse::<toml::Table>()) {
            Ok(Ok(config)) => {
                let replaced =
                    config
                        .get("source")
                        .and_then(Value::as_table)
                        .is_some_and(|sources| {
                            sources
                                .values()
                                .filter_map(Value::as_table)
                                .any(|s| s.contains_key("replace-with"))
                        });
                if replaced || config.contains_key("patch") || config.contains_key("paths") {
                    violations.push(format!(
                        "SOURCE-OVERRIDE: {name} replaces, patches or overrides a source"
                    ));
                }
            }
            _ => violations.push(format!("UNREADABLE: {name}")),
        }
    }

    if let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml"))
        && let Ok(manifest) = text.parse::<toml::Table>()
        && (manifest.contains_key("patch") || manifest.contains_key("replace"))
    {
        violations.push("SOURCE-OVERRIDE: Cargo.toml has [patch] or [replace]".to_string());
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn tree(lock: Option<&str>, files: &[(&str, &str)]) -> std::path::PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/sources-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(root.join(".cargo")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        if let Some(lock) = lock {
            std::fs::write(root.join("Cargo.lock"), lock).unwrap();
        }
        for (path, text) in files {
            std::fs::write(root.join(path), text).unwrap();
        }
        root
    }

    const REGISTRY: &str = "version = 4\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";

    #[test]
    fn registry_only_lockfile_passes() {
        assert!(check_sources(&tree(Some(REGISTRY), &[])).is_ok());
    }

    #[test]
    fn missing_lockfile_fails() {
        assert!(
            check_sources(&tree(None, &[]))
                .unwrap_err()
                .contains("missing")
        );
    }

    #[test]
    fn git_source_fails() {
        let lock = format!(
            "{REGISTRY}\n[[package]]\nname = \"x\"\nversion = \"0.1.0\"\nsource = \"git+https://example.invalid/x?rev=abc#abc\"\n"
        );
        assert!(
            check_sources(&tree(Some(&lock), &[]))
                .unwrap_err()
                .contains("GIT-SOURCE")
        );
    }

    #[test]
    fn cargo_config_overrides_fail() {
        for config in [
            "[source.crates-io]\nreplace-with = \"mirror\"\n[source.mirror]\nregistry = \"https://example.invalid\"\n",
            "[patch.crates-io]\nserde = { path = \"../serde\" }\n",
            "paths = [\"../elsewhere\"]\n",
        ] {
            let root = tree(Some(REGISTRY), &[(".cargo/config.toml", config)]);
            assert!(
                check_sources(&root)
                    .unwrap_err()
                    .contains("SOURCE-OVERRIDE"),
                "{config}"
            );
        }
    }

    #[test]
    fn manifest_patch_fails() {
        let root = tree(
            Some(REGISTRY),
            &[(
                "Cargo.toml",
                "[workspace]\nmembers = []\n[patch.crates-io]\nserde = { path = \"x\" }\n",
            )],
        );
        assert!(
            check_sources(&root)
                .unwrap_err()
                .contains("SOURCE-OVERRIDE")
        );
    }
}
