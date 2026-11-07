use std::path::Path;

use toml::Value;

use crate::pins::{dependency_tables, inherited_spec, is_commit, member_manifests, parse_manifest};

/// The one git source the project may use: the storytold Craft libraries, by commit.
/// `deny.toml` `[sources] allow-git` carries the same URL; `check_deny` keeps them equal.
const CRAFT_LIBS: &str = "https://github.com/storytold/craft-libs";

/// A registry package with one of these names is a different publisher's code, not the
/// Craft crate (which comes from `CRAFT_LIBS` by git). cargo-deny cannot ban a name only
/// when it comes from a registry, so this check does it, by prefix.
fn is_craft_name(name: &str) -> bool {
    name == "cadcraft" || name.starts_with("craft-") || name.starts_with("cadcraft-")
}

/// Dependencies come from the registry, except the Craft libraries, which come from
/// `CRAFT_LIBS` by a full commit. Rejects any other git source (also a branch, a tag, no
/// `rev` or a short one) in `Cargo.lock` and in manifests, any registry package named like
/// a Craft crate, and any source replacement, `[patch]`, `[replace]` or `paths` override
/// in `.cargo/config(.toml)` or the workspace manifest, none of which show up in the
/// lockfile itself. Also checks `deny.toml` still carries the matching git rule.
pub fn check_sources(root: &Path) -> Result<(), String> {
    let mut violations = Vec::new();
    if let Err(e) = check_deny(root) {
        violations.push(e);
    }

    let lock_path = root.join("Cargo.lock");
    match std::fs::read_to_string(&lock_path) {
        Err(_) => violations.push("Cargo.lock is missing".to_string()),
        Ok(text) => match text.parse::<toml::Table>() {
            Err(e) => violations.push(format!("Cargo.lock is unreadable: {e}")),
            Ok(lock) => {
                let packages = lock.get("package").and_then(Value::as_array);
                for pkg in packages.into_iter().flatten() {
                    let source = pkg.get("source").and_then(Value::as_str).unwrap_or("");
                    let name = pkg.get("name").and_then(Value::as_str).unwrap_or("?");
                    if source.starts_with("git+") && !lock_git_ok(source) {
                        violations.push(format!(
                            "GIT-SOURCE: {name} comes from {source}; only {CRAFT_LIBS} by a full 40-hex rev is allowed"
                        ));
                    }
                    if source.starts_with("registry+") && is_craft_name(name) {
                        violations.push(format!("BANNED-CRATE: {name} comes from {source}"));
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

    match manifest_deps(root) {
        Ok(deps) => {
            for (manifest, dep, real, spec) in deps {
                if spec.get("git").is_some() {
                    if !manifest_git_ok(&spec) {
                        violations.push(format!(
                            "GIT-SOURCE: {manifest} depends on {real} (as {dep}) from git; only {CRAFT_LIBS} by a full 40-hex rev is allowed"
                        ));
                    }
                } else if spec.get("path").is_none() && is_craft_name(&real) {
                    violations.push(format!(
                        "BANNED-CRATE: {manifest} depends on {real} (as {dep}) from a registry"
                    ));
                }
            }
        }
        Err(e) => violations.push(e),
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("\n"))
    }
}

/// `git+<url>?rev=<commit>#<commit>` as written in `Cargo.lock`: the allowed URL and an
/// explicit full `rev` (a branch or tag shows up as `?branch=`/`?tag=`, none as no query).
fn lock_git_ok(source: &str) -> bool {
    let rest = source.strip_prefix("git+").unwrap_or(source);
    let rest = rest.split('#').next().unwrap_or(rest);
    let (url, query) = rest.split_once('?').unwrap_or((rest, ""));
    url == CRAFT_LIBS && query.strip_prefix("rev=").is_some_and(is_commit)
}

fn manifest_git_ok(spec: &Value) -> bool {
    spec.get("git").and_then(Value::as_str) == Some(CRAFT_LIBS)
        && spec
            .get("rev")
            .and_then(Value::as_str)
            .is_some_and(is_commit)
}

/// `deny.toml` must keep denying unknown git sources and allow exactly `CRAFT_LIBS`.
/// Fails closed: a missing file or a dropped rule is the regression this guards against.
fn check_deny(root: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(root.join("deny.toml"))
        .map_err(|e| format!("deny.toml is unreadable: {e}"))?;
    let doc: toml::Table = text
        .parse()
        .map_err(|e| format!("deny.toml is unreadable: {e}"))?;
    let sources = doc.get("sources");
    let denies_unknown = sources
        .and_then(|s| s.get("unknown-git"))
        .and_then(Value::as_str)
        == Some("deny");
    let allowed: Vec<&str> = sources
        .and_then(|s| s.get("allow-git"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if denies_unknown && allowed == [CRAFT_LIBS] {
        Ok(())
    } else {
        Err(format!(
            "deny.toml [sources] must set unknown-git = \"deny\" and allow-git = [\"{CRAFT_LIBS}\"]"
        ))
    }
}

/// Every dependency declared without `path` in the root and member manifests, as
/// (manifest, key, real crate name after `package = ...`, the spec).
fn manifest_deps(root: &Path) -> Result<Vec<(String, String, String, Value)>, String> {
    let mut manifests = member_manifests(root)?;
    manifests.push(root.join("Cargo.toml"));
    let root_doc = parse_manifest(&root.join("Cargo.toml"))?;
    let workspace_deps = root_doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table);
    let mut found = Vec::new();
    for manifest in manifests {
        let doc = parse_manifest(&manifest)?;
        for deps in dependency_tables(&doc) {
            for (dep, spec) in deps {
                let Ok(spec) = inherited_spec(dep, spec, workspace_deps) else {
                    continue; // pins reports a missing workspace entry
                };
                let real = spec.get("package").and_then(Value::as_str).unwrap_or(dep);
                found.push((
                    manifest.display().to_string(),
                    dep.clone(),
                    real.to_string(),
                    spec.clone(),
                ));
            }
        }
    }
    Ok(found)
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
        std::fs::write(root.join("deny.toml"), DENY).unwrap();
        if let Some(lock) = lock {
            std::fs::write(root.join("Cargo.lock"), lock).unwrap();
        }
        for (path, text) in files {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        }
        root
    }

    const DENY: &str = "[sources]\nunknown-git = \"deny\"\nallow-git = [\"https://github.com/storytold/craft-libs\"]\n";

    const COMMIT: &str = "c39e435cc34417843793df240a0b38bac970179b";
    const LIBS: &str = "https://github.com/storytold/craft-libs";

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

    fn lock_with(name: &str, source: Option<&str>) -> String {
        let source = source.map_or(String::new(), |s| format!("source = \"{s}\"\n"));
        format!("{REGISTRY}\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n{source}")
    }

    const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

    #[test]
    fn banned_crate_from_the_registry_fails_naming_it() {
        for name in ["craft-ui", "cadcraft-cli"] {
            let error =
                check_sources(&tree(Some(&lock_with(name, Some(CRATES_IO))), &[])).unwrap_err();
            assert!(
                error.contains("BANNED-CRATE") && error.contains(name),
                "{error}"
            );
        }
    }

    #[test]
    fn workspace_member_with_a_banned_name_is_not_a_registry_crate() {
        assert!(check_sources(&tree(Some(&lock_with("craft-ui", None)), &[])).is_ok());
    }

    #[test]
    fn banned_dependency_in_a_manifest_fails_unless_path_or_git() {
        let manifest = |dep: &str| {
            [
                ("Cargo.toml", "[workspace]\nmembers = [\"m\"]\n".to_string()),
                (
                    "m/Cargo.toml",
                    format!("[package]\nname = \"m\"\n[dependencies]\n{dep}\n"),
                ),
            ]
        };
        for dep in [
            "craft-ui = \"=1.0.0\"",
            "craft-ui = { version = \"=1.0.0\" }",
            "renamed = { package = \"craft-ui\", version = \"=1.0.0\" }",
        ] {
            let files = manifest(dep);
            let files: Vec<_> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
            let error = check_sources(&tree(Some(REGISTRY), &files)).unwrap_err();
            assert!(
                error.contains("BANNED-CRATE") && error.contains("craft-ui"),
                "{dep}: {error}"
            );
        }
        for dep in [
            "craft-ui = { path = \"../ui\" }",
            "craft-ui = { git = \"https://github.com/storytold/craft-libs\", rev = \"c39e435cc34417843793df240a0b38bac970179b\" }",
        ] {
            let files = manifest(dep);
            let files: Vec<_> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
            assert!(
                check_sources(&tree(Some(REGISTRY), &files)).is_ok(),
                "{dep}"
            );
        }
    }

    fn git_lock(url: &str, query: &str, fragment: &str) -> String {
        lock_with(
            "craft-atomic",
            Some(&format!("git+{url}{query}#{fragment}")),
        )
    }

    #[test]
    fn craft_libs_git_source_with_a_full_rev_passes() {
        let lock = git_lock(LIBS, &format!("?rev={COMMIT}"), COMMIT);
        assert!(check_sources(&tree(Some(&lock), &[])).is_ok());
    }

    #[test]
    fn git_source_that_is_not_craft_libs_by_full_rev_fails() {
        let rev = format!("?rev={COMMIT}");
        for (url, query) in [
            (LIBS, "?branch=main".to_string()),
            (LIBS, "?tag=v1".to_string()),
            (LIBS, String::new()),
            (LIBS, "?rev=c39e435".to_string()),
            ("https://example.invalid/storytold/craft-libs", rev.clone()),
            ("https://github.com/storytold/cadcraft", rev.clone()),
            ("https://github.com/someone-else/craft-libs", rev.clone()),
            ("https://github.com/storytold/craft-libs-fork", rev),
        ] {
            let error =
                check_sources(&tree(Some(&git_lock(url, &query, COMMIT)), &[])).unwrap_err();
            assert!(error.contains("GIT-SOURCE"), "{url}{query}: {error}");
        }
    }

    #[test]
    fn craft_named_registry_packages_fail_naming_them() {
        for name in [
            "craft-atomic",
            "cadcraft",
            "cadcraft-core",
            "craft-brand-new",
        ] {
            let error =
                check_sources(&tree(Some(&lock_with(name, Some(CRATES_IO))), &[])).unwrap_err();
            assert!(
                error.contains("BANNED-CRATE") && error.contains(name),
                "{error}"
            );
        }
    }

    #[test]
    fn registry_package_with_an_unrelated_name_passes() {
        for name in ["crafty", "cfdcraft-core", "scraft", "arcraft-x"] {
            assert!(
                check_sources(&tree(Some(&lock_with(name, Some(CRATES_IO))), &[])).is_ok(),
                "{name}"
            );
        }
    }

    #[test]
    fn manifest_git_dependency_must_be_craft_libs_by_full_rev() {
        let manifest = |dep: &str| {
            [
                ("Cargo.toml", "[workspace]\nmembers = [\"m\"]\n".to_string()),
                (
                    "m/Cargo.toml",
                    format!("[package]\nname = \"m\"\n[dependencies]\n{dep}\n"),
                ),
            ]
        };
        let run = |dep: &str| {
            let files = manifest(dep);
            let files: Vec<_> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
            check_sources(&tree(Some(REGISTRY), &files))
        };
        let good = format!("craft-atomic = {{ git = \"{LIBS}\", rev = \"{COMMIT}\" }}");
        assert!(run(&good).is_ok());
        for dep in [
            format!("craft-atomic = {{ git = \"{LIBS}\", branch = \"main\" }}"),
            format!("craft-atomic = {{ git = \"{LIBS}\", tag = \"v1\" }}"),
            format!("craft-atomic = {{ git = \"{LIBS}\" }}"),
            format!("craft-atomic = {{ git = \"{LIBS}\", rev = \"c39e435\" }}"),
            format!("x = {{ git = \"https://example.invalid/x\", rev = \"{COMMIT}\" }}"),
        ] {
            let error = run(&dep).unwrap_err();
            assert!(error.contains("GIT-SOURCE"), "{dep}: {error}");
        }
    }

    #[test]
    fn deny_toml_that_drops_the_git_rule_fails_closed() {
        for deny in [
            None,
            Some(""),
            Some(
                "[sources]\nunknown-git = \"allow\"\nallow-git = [\"https://github.com/storytold/craft-libs\"]\n",
            ),
            Some("[sources]\nunknown-git = \"deny\"\n"),
            Some(
                "[sources]\nunknown-git = \"deny\"\nallow-git = [\"https://example.invalid/x\"]\n",
            ),
            Some(
                "[sources]\nunknown-git = \"deny\"\nallow-git = [\"https://github.com/storytold/craft-libs\", \"https://example.invalid/x\"]\n",
            ),
        ] {
            let root = tree(Some(REGISTRY), &[]);
            match deny {
                None => std::fs::remove_file(root.join("deny.toml")).unwrap(),
                Some(text) => std::fs::write(root.join("deny.toml"), text).unwrap(),
            }
            assert!(
                check_sources(&root).unwrap_err().contains("deny.toml"),
                "{deny:?}"
            );
        }
    }
}
