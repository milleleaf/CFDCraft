//! Solver-neutral, unit-carrying study inputs and resolved SI recipes.

pub mod diagnostics;
pub mod hash;
pub mod migrate;
pub mod quantity;
pub mod raw;
pub mod resolve;
pub mod roundtrip;
pub mod study;
pub mod units;

pub use diagnostics::{
    Capabilities, Diagnostic, Location, Severity, diagnose, has_errors, is_known_code, render_text,
};
pub use hash::canonical_hash;
pub use migrate::MigrationNote;
pub use quantity::{Quantity, Unit};
pub use raw::{MAX_RAW_OVERRIDES, RawBackend, RawOverride};
pub use resolve::{
    MonitorPlan, PlannedLine, PlannedProbe, PlannedSurface, ResolveError, ResolvedBoundary,
    ResolvedLine, ResolvedMonitors, ResolvedProbe, ResolvedStudy, ResolvedSurface, resolve,
    speed_from_vector, velocity_vector,
};
pub use study::{
    BoundaryCondition, BoundaryKind, Face, LineMonitor, MonitorField, Monitors, Probe,
    SettleSettings, Study, StudyError, SurfaceMonitor, SurfaceQuantity,
};
