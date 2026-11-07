//! The stage keys of a study, reached through the same gate chain as every other operation.

use crate::backends::{by_id, default_row, identity::tool_identity_for};
use crate::prepare::{Prepared, prepare_row};
use crate::report::Report;
use cfdcraft_backend::CompileOptions;
use cfdcraft_backend_esi::Dialect;
use cfdcraft_keys::{StageRecipes, ToolIdentity};
use cfdcraft_store::ContentHash;

/// How many hexadecimal characters of the writer fingerprint the version carries.
const FINGERPRINT_LEN: usize = 12;

/// The version of the case writer: the version of this build, then `+` and a fingerprint of the
/// source text that decides what the writer emits. The fingerprint moves with the text of those
/// files, so a writer that emits a different case has a different identity even when nobody
/// raised the version. It is deliberately coarse: an edit that changes no emitted byte (a comment)
/// also moves it, which makes a result look out of date and never the other way round.
pub(crate) fn writer_version_of(version: &str, sources: &[(&str, &str)]) -> String {
    let mut text = String::new();
    for (name, source) in sources {
        text.push_str(name);
        text.push('\0');
        // Line endings must not tell two checkouts apart.
        text.push_str(&source.replace('\r', ""));
        text.push('\0');
    }
    let hash = ContentHash::of(text.as_bytes()).to_string();
    format!("{version}+{}", &hash[..FINGERPRINT_LEN])
}

/// The tools that make a case in `dialect`: the identity of the registry row with its id (see
/// [`tool_identity_for`]).
#[doc(hidden)]
pub fn tool_identity(dialect: Dialect) -> ToolIdentity {
    let row = by_id(dialect.id()).expect("every dialect has a registry row");
    tool_identity_for(row.backend)
}

/// The tools of the backend used when none is named.
pub(crate) fn default_tool_identity() -> ToolIdentity {
    tool_identity_for(default_row().backend)
}

/// The gate chain for the backend used when none is named.
// The report is the error on purpose: a failed chain hands back the finished, classified result.
#[allow(clippy::result_large_err)]
pub(crate) fn prepare_default(study_json: &str) -> Result<Prepared, Report> {
    prepare_row(study_json, &default_row(), &CompileOptions::default())
}

/// The geometry hash, the mesh key and the result key of a study, in that order as artifacts.
/// A study that fails any gate gives its failing report and no key.
pub fn stage_keys(study_json: &str) -> Report {
    let prepared = match prepare_default(study_json) {
        Ok(prepared) => prepared,
        Err(report) => return report.for_operation("keys"),
    };
    let recipes = StageRecipes::from_resolved(&prepared.resolved, &default_tool_identity());
    let keys = recipes.keys();
    Report::from_diagnostics("keys", prepared.warnings, Some(prepared.hash))
        .with_migration(prepared.migration)
        .with_artifact("geometry_hash", recipes.mesh.geometry.hash().to_string())
        .with_artifact("mesh_key", keys.mesh.to_string())
        .with_artifact("result_key", keys.result.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_writer_version_follows_the_writer_sources_and_not_their_line_endings() {
        let base = [("case.rs", "fn a() {}\n"), ("lib.rs", "fn b() {}\n")];
        let version = writer_version_of("0.1.0", &base);
        assert!(version.starts_with("0.1.0+"), "{version}");
        assert_eq!(version.len(), "0.1.0+".len() + FINGERPRINT_LEN);

        let edited = [("case.rs", "fn a() { 1; }\n"), ("lib.rs", "fn b() {}\n")];
        assert_ne!(writer_version_of("0.1.0", &edited), version);
        let crlf = [("case.rs", "fn a() {}\r\n"), ("lib.rs", "fn b() {}\r\n")];
        assert_eq!(writer_version_of("0.1.0", &crlf), version);
        // The same text in another file is another writer.
        let moved = [("case.rs", "fn b() {}\n"), ("lib.rs", "fn a() {}\n")];
        assert_ne!(writer_version_of("0.1.0", &moved), version);
    }

    #[test]
    fn the_writer_version_covers_the_override_step_and_the_dictionary_editor() {
        let sources = default_row().backend.writer_sources();
        let earlier = [
            "block_mesh.rs",
            "case.rs",
            "control_dict.rs",
            "export.rs",
            "lib.rs",
            "transport.rs",
        ];
        let kept: Vec<(&str, &str)> = sources
            .iter()
            .filter(|(name, _)| earlier.contains(name))
            .copied()
            .collect();
        assert_eq!(kept.len(), earlier.len());
        let version = env!("CARGO_PKG_VERSION");
        assert_ne!(
            writer_version_of(version, &kept),
            writer_version_of(version, sources)
        );
        assert_eq!(
            tool_identity(Dialect::ALL[0]).writer_version,
            writer_version_of(version, sources)
        );
    }

    #[test]
    fn the_identity_of_a_dialect_is_the_identity_of_its_row() {
        let default = tool_identity(Dialect::ALL[0]);
        assert_eq!(default, default_tool_identity());
        assert_eq!(default, tool_identity_for(default_row().backend));
        let later = tool_identity(Dialect::EsiV2606);
        assert_eq!(later.writer_version, default.writer_version);
        assert_ne!(later.image_digest, default.image_digest);
        assert_ne!(later.dialect, default.dialect);
    }
}
