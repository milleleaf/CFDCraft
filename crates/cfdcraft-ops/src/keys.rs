//! The stage keys of a study, reached through the same gate chain as every other operation.

use crate::prepare::prepare;
use crate::report::Report;
use cfdcraft_backend_esi::Dialect;
use cfdcraft_keys::{StageRecipes, ToolIdentity};

/// The tools that make a case in `dialect`: its image digest, its identifier, no geometry
/// sidecar, and the version of this build.
pub fn tool_identity(dialect: Dialect) -> ToolIdentity {
    let image = dialect.image();
    let digest = image.split_once('@').map_or(image, |(_, digest)| digest);
    ToolIdentity::new(digest, dialect.id(), None, env!("CARGO_PKG_VERSION"))
}

/// The geometry hash, the mesh key and the result key of a study, in that order as artifacts.
/// A study that fails any gate gives its failing report and no key.
pub fn stage_keys(study_json: &str) -> Report {
    let dialect = Dialect::EsiV2512;
    let prepared = match prepare(study_json, dialect) {
        Ok(prepared) => prepared,
        Err(report) => return report.for_operation("keys"),
    };
    let recipes = StageRecipes::from_resolved(&prepared.resolved, &tool_identity(dialect));
    let keys = recipes.keys();
    Report::from_diagnostics("keys", prepared.warnings, Some(prepared.hash))
        .with_migration(prepared.migration)
        .with_artifact("geometry_hash", recipes.mesh.geometry.hash().to_string())
        .with_artifact("mesh_key", keys.mesh.to_string())
        .with_artifact("result_key", keys.result.to_string())
}
