use std::path::Path;

use toml::Value;

/// Allowlist for `crates/cfdcraft-model` (L0). The model must never gain a
/// compute, UI or transport dependency (no `uom` for now; the wasm-clean rule
/// bans tokio/ssh/egui in the core).
const MODEL_ALLOWLIST: &[&str] = &[
    "serde",
    "serde_json",
    "thiserror",
    "schemars",
    "serde_jcs",
    "blake3",
];

/// Assert the dependency graph respects the layering: L0 `cfdcraft-model`
/// depends only on the allowlist; no crate may gain a layer-violating
/// dependency (no third-party layering checker fits, so this small check
/// reads the workspace manifests directly).
pub fn check_layers(root: &Path) -> Result<(), String> {
    let members = super::pins::member_manifests(root)?;
    let root_doc = super::pins::parse_manifest(&root.join("Cargo.toml"))?;
    let workspace = root_doc
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table);
    let mut violations = Vec::new();
    for manifest in members {
        let doc = super::pins::parse_manifest(&manifest)?;
        let name = doc
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("?");
        for deps in super::pins::dependency_tables(&doc) {
            for (alias, spec) in deps {
                let spec = super::pins::inherited_spec(alias, spec, workspace)?;
                let dep = spec.get("package").and_then(Value::as_str).unwrap_or(alias);
                if name == "cfdcraft-model" && !MODEL_ALLOWLIST.contains(&dep) {
                    violations.push(format!("LAYER-VIOLATION: {name} depends on {dep}"));
                }
            }
        }
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
    use crate::pins::tests::workspace;

    #[test]
    fn model_compute_dependency_fails_even_under_alias_or_target() {
        for deps in [
            "[dependencies]\nasync_runtime = { package = \"tokio\", version = \"=1.0.0\" }",
            "[target.'cfg(unix)'.dependencies]\ntokio = \"=1.0.0\"",
            "[dev-dependencies]\negui = \"=0.37.0\"",
        ] {
            let root = workspace("", deps);
            assert!(check_layers(&root).unwrap_err().contains("LAYER-VIOLATION"));
        }
    }

    #[test]
    fn model_hashing_dependencies_are_allowed() {
        let root = workspace(
            "",
            "[dependencies]\nserde_jcs = \"=0.2.0\"\nblake3 = \"=1.8.7\"",
        );
        assert!(check_layers(&root).is_ok());
    }
}
