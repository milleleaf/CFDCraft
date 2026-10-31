use std::path::{Path, PathBuf};

use toml::Value;

/// Parse every member `Cargo.toml` under `root` and require an exact `=x.y.z`
/// pin on every `[dependencies]` entry (git deps require a pinned `rev`;
/// `workspace = true` entries are resolved by the root manifest's own entry,
/// which is checked as part of the root scan when the workspace manifest is
/// itself a member-scanned file — here we scan the root manifest explicitly).
pub fn check_pins(root: &Path) -> Result<(), String> {
    let mut manifests = member_manifests(root)?;
    manifests.push(root.join("Cargo.toml"));
    let root_doc = parse_manifest(&root.join("Cargo.toml"))?;
    let workspace_deps = root_doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table);
    let mut violations = Vec::new();
    for manifest in manifests {
        check_manifest(&manifest, workspace_deps, &mut violations);
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations.join("\n"))
    }
}

pub(crate) fn member_manifests(root: &Path) -> Result<Vec<PathBuf>, String> {
    let workspace = parse_manifest(&root.join("Cargo.toml"))?;
    let mut manifests = Vec::new();
    let Some(Value::Array(members)) = workspace.get("workspace").and_then(|w| w.get("members"))
    else {
        return Err("workspace manifest has no `members`".into());
    };
    for member in members {
        let Some(name) = member.as_str() else {
            continue;
        };
        if name.ends_with("/*") {
            let base = root.join(name.trim_end_matches("/*"));
            let entries = std::fs::read_dir(&base)
                .map_err(|e| format!("cannot list {}: {e}", base.display()))?;
            for entry in entries {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.path().join("Cargo.toml").is_file() {
                    manifests.push(entry.path().join("Cargo.toml"));
                }
            }
        } else {
            manifests.push(root.join(name).join("Cargo.toml"));
        }
    }
    Ok(manifests)
}

pub(crate) fn dependency_tables(doc: &toml::Table) -> Vec<&toml::Table> {
    let mut tables = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = doc.get(section).and_then(Value::as_table) {
            tables.push(table);
        }
    }
    if let Some(workspace) = doc.get("workspace").and_then(Value::as_table) {
        tables.extend(dependency_tables(workspace));
    }
    if let Some(targets) = doc.get("target").and_then(Value::as_table) {
        for target in targets.values().filter_map(Value::as_table) {
            tables.extend(dependency_tables(target));
        }
    }
    tables
}

pub(crate) fn inherited_spec<'a>(
    dep: &str,
    spec: &'a Value,
    workspace: Option<&'a toml::Table>,
) -> Result<&'a Value, String> {
    if spec.get("workspace").and_then(Value::as_bool) == Some(true) {
        workspace
            .and_then(|w| w.get(dep))
            .ok_or_else(|| format!("UNPINNED: {dep} has no workspace dependency"))
    } else {
        Ok(spec)
    }
}

fn check_manifest(path: &Path, workspace: Option<&toml::Table>, violations: &mut Vec<String>) {
    let doc = match parse_manifest(path) {
        Ok(d) => d,
        Err(e) => {
            violations.push(format!("UNREADABLE: {}: {e}", path.display()));
            return;
        }
    };
    for deps in dependency_tables(&doc) {
        for (dep, spec) in deps {
            let where_at = format!("{} {dep}", path.display());
            let spec = match inherited_spec(dep, spec, workspace) {
                Ok(spec) => spec,
                Err(error) => {
                    violations.push(error);
                    continue;
                }
            };
            match spec {
                Value::String(version) => {
                    if !is_exact(version) {
                        violations.push(format!("UNPINNED: {where_at} = \"{version}\""));
                    }
                }
                Value::Table(t) => {
                    if let Some(Value::String(version)) = t.get("version")
                        && !is_exact(version)
                    {
                        violations.push(format!("UNPINNED: {where_at} = \"{version}\""));
                    }
                    if t.contains_key("git")
                        && !t.get("rev").and_then(Value::as_str).is_some_and(|rev| {
                            rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit())
                        })
                    {
                        violations.push(format!(
                            "UNPINNED: {where_at} git dep requires a full commit `rev`"
                        ));
                    }
                    if !t.contains_key("version")
                        && !t.contains_key("git")
                        && !t.contains_key("path")
                    {
                        violations.push(format!("UNPINNED: {where_at} has no version or source"));
                    }
                }
                other => violations.push(format!("UNREADABLE: {where_at}: {other:?}")),
            }
        }
    }
}

fn is_exact(version: &str) -> bool {
    let Some(version) = version.strip_prefix('=') else {
        return false;
    };
    let version = version.trim();
    if version.is_empty()
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
    {
        return false;
    }
    let base = version.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<_> = base.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

pub(crate) fn parse_manifest(path: &Path) -> Result<toml::Table, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.parse()
        .map_err(|e: toml::de::Error| format!("{}: {e}", path.display()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) fn workspace(deps: &str, member: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/governance-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(root.join("crates/cfdcraft-model")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            format!("[workspace]\nmembers = [\"crates/cfdcraft-model\"]\n{deps}\n"),
        )
        .unwrap();
        std::fs::write(
            root.join("crates/cfdcraft-model/Cargo.toml"),
            format!("[package]\nname = \"cfdcraft-model\"\n{member}\n"),
        )
        .unwrap();
        root
    }

    #[test]
    fn workspace_versions_are_checked() {
        let root = workspace(
            "[workspace.dependencies]\nserde = \"^1.0.0\"",
            "[dependencies]\nserde.workspace = true",
        );
        assert!(check_pins(&root).unwrap_err().contains("UNPINNED"));
    }

    #[test]
    fn target_dependencies_are_checked() {
        let root = workspace("", "[target.'cfg(unix)'.dependencies]\nserde = \"~1.0.0\"");
        assert!(check_pins(&root).unwrap_err().contains("UNPINNED"));
    }

    #[test]
    fn inherited_dependency_must_exist() {
        let root = workspace("", "[dependencies]\nserde.workspace = true");
        assert!(check_pins(&root).is_err());
    }

    #[test]
    fn exact_pin_is_a_complete_version() {
        for invalid in [
            "1.2.3",
            "^1.2.3",
            "~1.2.3",
            "=1",
            "=1.2.*",
            "=1.2.3 || =2.0.0",
            "=",
        ] {
            assert!(!is_exact(invalid), "accepted {invalid}");
        }
        assert!(is_exact("=1.2.3"));
        assert!(is_exact("=0.7.0-rc2"));
    }

    #[test]
    fn pinned_inherited_and_path_dependencies_pass() {
        let root = workspace(
            "[workspace.dependencies]\nserde = \"=1.0.229\"",
            "[dependencies]\nserde.workspace = true\nmodel = { path = \"../model\" }",
        );
        assert!(check_pins(&root).is_ok());
    }
}
