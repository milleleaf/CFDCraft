//! Disk use per scenario, and a plan for pruning, from the blobs the run records name.
//!
//! The crate is pure: it reads no file and removes none. The caller builds one `BlobHold`
//! for every blob a run record names, tagged with the scenario the run was made for, and
//! the crate adds the bytes up. A blob that several runs or several scenarios name is one
//! blob on disk, so it is counted once in a total and once for each scenario that holds it.
//!
//! Planning never removes anything. The caller shows the plan first and only then acts.

mod prune;
mod usage;

pub use prune::{PrunePlan, Removal, plan_prune};
pub use usage::{BlobHold, DiskUse, ScenarioUse, disk_use};
