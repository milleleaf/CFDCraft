//! Run records: what ran, what it stood on, and what it made.
//!
//! A run is one immutable record. It names the mesh and the result it recorded (each by key,
//! recipe, blob hash and size), the provenance it ran from (geometry, mesh, settings, tools,
//! host), its checks and its key numbers. A rerun is a record that repeats an earlier run and
//! carries the comparison with it.
//!
//! The log only grows: a record is appended and never edited. Host labels are the one thing
//! that may be cleared afterwards, so that a file can be shared without saying where it ran.
//!
//! The crate is pure: it has no file, store, clock or process access.

mod plan;
mod record;
mod status;
mod verify;

pub use plan::{Item, PlanError, pending, plan_update};
pub use record::{
    BlobUse, Check, Checkpoint, ContinuedFrom, Outcome, Output, Provenance, RunRecord, find_run,
    latest_mesh, latest_result, newest_mesh_output, newest_result_output, next_run_id,
};
pub use status::{StageState, Status, status, status_for};
pub use verify::{MAX_RUNS, RunsError, is_plain_name, redact_hosts, verify_runs};

pub use cfdcraft_compare::{Comparison, KeyNumbers, Tolerance, Verdict};
pub use cfdcraft_keys::{
    Axis, Change, GeometryRecipe, MeshRecipe, Residual, ResultRecipe, Stage, StageKey,
    StageRecipes, ToolIdentity, ToolPart,
};
