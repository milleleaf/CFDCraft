use std::path::Path;

use toml::Value;

/// Dependency allowlists per crate, matched against each dependency's real package name.
/// The model crate must never gain a compute, UI or transport dependency (no `uom` for now;
/// the wasm-clean rule bans tokio/ssh/egui in the core). The document crates are held to the
/// same discipline: they stay off compute, UI and transport code so the per-crate CI gate
/// stays fast and a browser viewer can reuse them. The keys, compare, runs and capture crates
/// stay pure or off compute for the same reason, and so do the scenario, sweep, disk-use,
/// dictionary-editing, queue, log-scanning, monitor-reading, settling and assessing crates. A
/// crate that is not listed is unconstrained.
const RULES: &[(&str, &[&str])] = &[
    (
        "cfdcraft-model",
        &[
            "serde",
            "serde_json",
            "thiserror",
            "schemars",
            "serde_jcs",
            "blake3",
        ],
    ),
    (
        "cfdcraft-store",
        &[
            "blake3",
            "craft-atomic",
            "serde",
            "serde_json",
            "schemars",
            "thiserror",
        ],
    ),
    // Pure on purpose: a browser viewer can read old studies later.
    (
        "cfdcraft-migrate",
        &["cfdcraft-model", "serde_json", "thiserror"],
    ),
    ("cfdcraft-text", &["craft-i18n"]),
    (
        "cfdcraft-doc",
        &[
            "cfdcraft-model",
            "cfdcraft-migrate",
            "cfdcraft-store",
            "cfdcraft-runs",
            "cfdcraft-scenarios",
            "cfdcraft-sweep",
            "craft-atomic",
            "craft-docpath",
            "zip",
            "blake3",
            "serde",
            "serde_json",
            "schemars",
            "thiserror",
        ],
    ),
    (
        "cfdcraft-pack",
        &[
            "cfdcraft-doc",
            "cfdcraft-store",
            "cfdcraft-model",
            "craft-atomic",
            "zip",
            "blake3",
            "serde_json",
            "thiserror",
        ],
    ),
    (
        "cfdcraft-schema",
        &[
            "cfdcraft-model",
            "cfdcraft-migrate",
            "cfdcraft-doc",
            "cfdcraft-queue",
            "jsonschema",
            "schemars",
            "serde_json",
            "zip",
            "thiserror",
        ],
    ),
    (
        "cfdcraft-keys",
        &[
            "cfdcraft-model",
            "cfdcraft-store",
            "blake3",
            "serde",
            "serde_json",
            "serde_jcs",
            "schemars",
        ],
    ),
    (
        "cfdcraft-compare",
        &["serde", "schemars", "thiserror", "serde_json"],
    ),
    (
        "cfdcraft-runs",
        &[
            "cfdcraft-keys",
            "cfdcraft-compare",
            "cfdcraft-model",
            "cfdcraft-settle",
            "cfdcraft-store",
            "serde",
            "serde_json",
            "schemars",
            "thiserror",
        ],
    ),
    (
        "cfdcraft-capture",
        &["cfdcraft-compare", "cfdcraft-store", "zip", "thiserror"],
    ),
    (
        "cfdcraft-scenarios",
        &[
            "cfdcraft-model",
            "cfdcraft-runs",
            "cfdcraft-keys",
            "serde",
            "serde_json",
            "schemars",
            "thiserror",
        ],
    ),
    (
        "cfdcraft-sweep",
        &["serde", "serde_json", "schemars", "thiserror"],
    ),
    (
        "cfdcraft-diskuse",
        &[
            "cfdcraft-keys",
            "cfdcraft-store",
            "serde",
            "serde_json",
            "schemars",
            "thiserror",
        ],
    ),
    ("cfdcraft-dictedit", &["thiserror"]),
    (
        "cfdcraft-queue",
        &["serde", "serde_json", "schemars", "thiserror"],
    ),
    ("cfdcraft-logscan", &["serde", "schemars", "thiserror"]),
    ("cfdcraft-monitors", &["serde", "schemars", "thiserror"]),
    (
        "cfdcraft-settle",
        &["serde", "schemars", "thiserror", "serde_json"],
    ),
    (
        "cfdcraft-assess",
        &[
            "cfdcraft-logscan",
            "cfdcraft-monitors",
            "cfdcraft-settle",
            "serde",
            "schemars",
            "thiserror",
        ],
    ),
];

/// Assert the dependency graph respects the layering: every crate listed in `RULES`
/// depends only on its allowlist (no third-party layering checker fits, so this small
/// check reads the workspace manifests directly).
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
        let Some((_, allowed)) = RULES.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        for deps in super::pins::dependency_tables(&doc) {
            for (alias, spec) in deps {
                let spec = super::pins::inherited_spec(alias, spec, workspace)?;
                let dep = spec.get("package").and_then(Value::as_str).unwrap_or(alias);
                if !allowed.contains(&dep) {
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

    /// A fixture workspace with the `crates/*` glob and one crate called `name`.
    fn named_workspace(name: &str, member: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/governance-tests")
            .join(format!(
                "layers-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(root.join("crates").join(name)).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates").join(name).join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\n{member}\n"),
        )
        .unwrap();
        root
    }

    fn deps_of(names: &[&str]) -> String {
        let lines: Vec<String> = names.iter().map(|n| format!("{n} = \"=1.0.0\"")).collect();
        format!("[dependencies]\n{}", lines.join("\n"))
    }

    #[test]
    fn document_crate_compute_dependency_fails_under_alias_target_or_dev() {
        for deps in [
            "[dependencies]\ntokio = \"=1.0.0\"",
            "[dependencies]\nclap = \"=1.0.0\"",
            "[dependencies]\negui = \"=1.0.0\"",
            "[dependencies]\nrmcp = \"=1.0.0\"",
            "[dependencies]\ncfdcraft-ops = { path = \"../cfdcraft-ops\" }",
            "[dependencies]\ncfdcraft-executor = { path = \"../cfdcraft-executor\" }",
            "[dependencies]\nasync_runtime = { package = \"tokio\", version = \"=1.0.0\" }",
            "[target.'cfg(unix)'.dependencies]\ntokio = \"=1.0.0\"",
            "[dev-dependencies]\negui = \"=0.37.0\"",
        ] {
            let root = named_workspace("cfdcraft-doc", deps);
            let error = check_layers(&root).unwrap_err();
            assert!(
                error.contains("LAYER-VIOLATION: cfdcraft-doc depends on"),
                "{deps}: {error}"
            );
        }
    }

    #[test]
    fn each_allowlist_accepts_its_names_and_rejects_a_neighbours() {
        for (name, allowed) in RULES {
            let root = named_workspace(name, &deps_of(allowed));
            assert!(check_layers(&root).is_ok(), "{name} rejected its own list");
        }
        for (name, foreign) in [
            ("cfdcraft-store", "zip"),
            ("cfdcraft-migrate", "zip"),
            ("cfdcraft-migrate", "craft-atomic"),
            ("cfdcraft-migrate", "cfdcraft-store"),
            ("cfdcraft-text", "blake3"),
            ("cfdcraft-pack", "cfdcraft-migrate"),
            ("cfdcraft-schema", "craft-atomic"),
            ("cfdcraft-keys", "zip"),
            ("cfdcraft-compare", "blake3"),
            ("cfdcraft-runs", "zip"),
            ("cfdcraft-capture", "cfdcraft-keys"),
            ("cfdcraft-scenarios", "blake3"),
            ("cfdcraft-sweep", "cfdcraft-model"),
            ("cfdcraft-diskuse", "zip"),
            ("cfdcraft-dictedit", "serde"),
            ("cfdcraft-queue", "cfdcraft-model"),
            ("cfdcraft-logscan", "serde_json"),
            ("cfdcraft-monitors", "cfdcraft-store"),
            ("cfdcraft-settle", "cfdcraft-logscan"),
            ("cfdcraft-assess", "cfdcraft-model"),
        ] {
            let root = named_workspace(name, &deps_of(&[foreign]));
            let error = check_layers(&root).unwrap_err();
            assert!(
                error.contains(&format!("LAYER-VIOLATION: {name} depends on {foreign}")),
                "{name} accepted {foreign}"
            );
        }
    }

    #[test]
    fn model_keeps_its_six_names_and_unlisted_crates_are_free() {
        let model = RULES
            .iter()
            .find(|(n, _)| *n == "cfdcraft-model")
            .unwrap()
            .1;
        assert_eq!(
            model,
            [
                "serde",
                "serde_json",
                "thiserror",
                "schemars",
                "serde_jcs",
                "blake3"
            ]
        );
        let root = named_workspace("cfdcraft-ops", &deps_of(&["tokio", "egui"]));
        assert!(check_layers(&root).is_ok());
    }

    #[test]
    fn rule_keys_are_exactly_the_constrained_crates() {
        let keys: Vec<&str> = RULES.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            keys,
            [
                "cfdcraft-model",
                "cfdcraft-store",
                "cfdcraft-migrate",
                "cfdcraft-text",
                "cfdcraft-doc",
                "cfdcraft-pack",
                "cfdcraft-schema",
                "cfdcraft-keys",
                "cfdcraft-compare",
                "cfdcraft-runs",
                "cfdcraft-capture",
                "cfdcraft-scenarios",
                "cfdcraft-sweep",
                "cfdcraft-diskuse",
                "cfdcraft-dictedit",
                "cfdcraft-queue",
                "cfdcraft-logscan",
                "cfdcraft-monitors",
                "cfdcraft-settle",
                "cfdcraft-assess",
            ]
        );
    }

    #[test]
    fn runs_may_use_settle_and_schema_may_use_queue() {
        for (name, dep) in [
            ("cfdcraft-runs", "cfdcraft-settle"),
            ("cfdcraft-schema", "cfdcraft-queue"),
        ] {
            let root = named_workspace(name, &deps_of(&[dep]));
            assert!(check_layers(&root).is_ok(), "{name} rejected {dep}");
        }
    }
}
