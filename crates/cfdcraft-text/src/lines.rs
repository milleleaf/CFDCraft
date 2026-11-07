//! The sentences the document layer shows a person, as data.
//!
//! A [`Line`] holds only values (names, paths, counts). The wording lives here, once, as an
//! English source text that doubles as the lookup key of the translation catalogs.

const MIGRATED: &str = "This file was written by an older version (schema {from}). It was updated to schema {to} in memory; the file on disk is unchanged until you save. {summary}";
const SAVED: &str = "Saved {path}.";
const OPENED: &str = "Opened {path}.";
const NEWER_SCHEMA: &str = "This file was written by a newer version of CFDCraft (schema {found}). This version reads schemas up to {supported}, so nothing was read from the file.";
const NOT_A_STUDY_FILE: &str = "{path} is not a CFDCraft study file: {reason}";
const LINK_MISSING: &str = "The linked file {name} is missing. It was expected at {expected}.";
const LINK_CHANGED: &str = "The linked file {name} has changed since it was linked.";
const LINK_REFUSED: &str = "The link to {name} cannot be followed: {reason}";
const LINK_ADDED: &str = "Linked {name} as {path}.";
const RESULT_MISSING: &str = "A stored result ({hash}) is missing from the store next to the file.";
const RESULT_CORRUPT: &str = "A stored result ({hash}) does not match its recorded fingerprint.";
const PACKED_ONE: &str = "Packed {n} stored result into {path}.";
const PACKED_OTHER: &str = "Packed {n} stored results into {path}.";
const UNPACKED_ONE: &str = "Unpacked {n} stored result into {path}.";
const UNPACKED_OTHER: &str = "Unpacked {n} stored results into {path}.";
const LINKS_NOT_PACKED_ONE: &str =
    "The pack does not include {n} linked file; copy it next to the unpacked study.";
const LINKS_NOT_PACKED_OTHER: &str =
    "The pack does not include {n} linked files; copy them next to the unpacked study.";
const TARGET_EXISTS: &str = "{path} already exists. Choose another name or remove it first.";
const PACK_MISSING: &str =
    "Stored result {hash} is not in the store, so the study cannot be packed.";
const PACK_DAMAGED: &str =
    "Stored result {hash} in the pack does not match its fingerprint. No study file was written.";
const PACK_INCOMPLETE: &str =
    "The pack is missing stored result {hash}. No study file was written.";
const NOT_WRITTEN: &str = "{path} could not be written: {reason}";
const LINK_NOT_ADDED: &str = "{name} could not be linked: {reason}";
const STALE_BECAUSE: &str = "{stage} stale because {reason}";
const REASON_LENGTH: &str = "domain length in {axis} {from} → {to} m";
const REASON_DOMAIN_OPENED: &str = "the domain was opened";
const REASON_DOMAIN_CLOSED: &str = "the domain was closed";
const REASON_CELLS: &str = "cells in {axis} {from} → {to}";
const REASON_LAYOUT: &str = "the boundary layout changed";
const REASON_VISCOSITY: &str = "viscosity {from} → {to} m²/s";
const REASON_BOUNDARY_VELOCITY: &str = "{boundary} velocity {from} → {to} m/s";
const REASON_BOUNDARY_KIND: &str = "{boundary} kind {from} → {to}";
const REASON_BOUNDARY_FACES: &str = "{boundary} faces {from} → {to}";
const REASON_BOUNDARY_ADDED: &str = "boundary {boundary} was added";
const REASON_BOUNDARY_REMOVED: &str = "boundary {boundary} was removed";
const REASON_END_TIME: &str = "end time {from} → {to} s";
const REASON_WRITE_INTERVAL: &str = "write interval {from} → {to} s";
const REASON_PRESSURE_LIMIT: &str = "pressure residual limit {from} → {to}";
const REASON_VELOCITY_LIMIT: &str = "velocity residual limit {from} → {to}";
const REASON_SAMPLES: &str = "centreline samples {from} → {to}";
const REASON_IMAGE: &str = "solver image {from} → {to}";
const REASON_DIALECT: &str = "solver dialect {from} → {to}";
const REASON_SIDECAR: &str = "geometry sidecar lock {from} → {to}";
const REASON_WRITER: &str = "CFDCraft version {from} → {to}";
const REASON_MESH: &str = "its mesh changed";
const STAGE_UP_TO_DATE: &str = "The {stage} is up to date (made by {run}).";
const STAGE_NEVER_MADE: &str = "No {stage} has been made yet.";
const STAGE_BLOB_MISSING: &str =
    "The stored {stage} ({hash}) is missing from the store, so it counts as out of date.";
const RUN_RECORDED: &str = "Recorded {run}: {stages}.";
const NOTHING_TO_RUN: &str = "Nothing to run: the mesh and the result are up to date.";
const RERUN_REPRODUCED: &str = "{run} reproduced {of}: every checked quantity is identical.";
const RERUN_WITHIN: &str =
    "{run} is within tolerance of {of}; the largest difference is {max_diff}.";
const RERUN_DIFFERS: &str = "{run} differs from {of}; quantities outside the tolerance: {count}.";
const TOOL_MISMATCH: &str =
    "This result was made with a different tool ({reasons}), so it cannot be rerun as recorded.";
const UNKNOWN_RUN: &str = "There is no run {run} in this file.";
const NO_RUNS: &str = "This file has no runs yet.";
const RUN_HAS_NO_RESULT: &str =
    "{run} recorded no result, so there is nothing to rerun or compare.";
const RECORDS_INCONSISTENT: &str = "The record of {run} is inconsistent: {reason}";
const NOT_PENDING: &str = "{item} is up to date, so there is nothing to update.";
const UNKNOWN_ITEM: &str = "There is no item {item}; an item is written name:mesh or name:result.";
const NEEDS_UPSTREAM: &str =
    "{item} is made from {needs}, which is also out of date; update {needs} too, and first.";
const ORDER_MISMATCH: &str = "The order must list each selected item exactly once.";
const ORDER_BREAKS_DEPENDENCY: &str = "{item} cannot come before {needs}, which it is made from.";
const ITEM_UPDATED: &str = "Updated {item} in {run}.";
const NOTHING_TO_UPDATE: &str = "Nothing is out of date.";
const MIGRATED_AND_SAVED: &str = "This file was written by an older version (schema {from}) and was saved at schema {to}. {summary}";
const OUTPUT_NOT_RECORDED: &str = "The output of the run could not be recorded: {reason}";
const DUPLICATE_NAME: &str = "Two documents are called {name}; give them different file names.";
const SCENARIO_CREATED: &str = "Created variant {scenario} of {parent}.";
const SCENARIO_UPDATED: &str = "Updated variant {scenario}.";
const SCENARIO_LABELLED: &str = "Updated the name, pin or notes of {scenario}.";
const SCENARIO_UNKNOWN: &str = "There is no variant {scenario} in this file.";
const SCENARIO_REFUSED: &str = "The variants cannot be saved: {reason}";
const OVERRIDE_NOT_APPLIED: &str =
    "The change to {setting} in {scenario} does not apply to the study: {reason}";
const SCENARIO_DIFFERS: &str = "{to} differs from {from}: {reasons}.";
const SCENARIO_SAME: &str = "{from} and {to} make the same case.";
const REASON_RAW_OVERRIDE: &str = "raw override {id} {from} → {to}";
const REASON_RAW_MESH: &str = "a raw mesh override changed";
const SWEEP_TOO_LARGE: &str =
    "Sweep {sweep} would make {points} points, more than the {max} allowed.";
const SWEEP_REFUSED: &str = "Sweep {sweep} cannot be made: {reason}";
const SWEEP_CREATED: &str = "Created sweep {sweep}: {points} variants.";
const SWEEP_UNKNOWN: &str = "There is no sweep {sweep} in this file.";
const PREFLIGHT_POINT_FAILED: &str = "{point} fails the checks: {reason}";
const PREFLIGHT_PASSED: &str =
    "All {points} points of {sweep} pass the checks; they need {meshes} different meshes.";
const PREFLIGHT_FAILED: &str =
    "{failed} of {points} points of {sweep} fail the checks; nothing was solved.";
const DISK_TOTAL: &str = "The stored results use {bytes} bytes across {scenarios} variants.";
const PRUNE_NOTHING: &str = "Nothing to prune.";
const PRUNE_DRY_RUN: &str = "Pruning would remove {blobs} stored results and free {bytes} bytes. Nothing was removed; run again with apply to remove them.";
const PRUNED: &str = "Removed {blobs} stored results and freed {bytes} bytes. The key numbers of every run are kept.";
const STAGE_PRUNED: &str =
    "The stored {stage} of {run} was pruned to free disk; its key numbers are kept.";
const PRUNE_REFUSED: &str = "Pruning was refused: {reason}";
const PRUNE_NOT_REMOVED: &str = "A stored result could not be removed: {reason}";
const QUEUE_ADDED: &str = "Added {jobs} jobs to the queue; {queued} are waiting.";
const QUEUE_NOTHING_TO_ADD: &str =
    "Nothing to add: everything asked for is up to date or already queued.";
const QUEUE_REFUSED: &str = "The queue request was refused: {reason}";
const QUEUE_UNKNOWN_JOB: &str = "There is no job {job} in the queue.";
const QUEUE_NOT_ALLOWED: &str = "Job {job} is {state}, so it cannot be {action}.";
const QUEUE_REORDERED: &str = "Moved {job} to position {position} among the waiting jobs.";
const QUEUE_CANCELLED: &str = "Cancelled {job}.";
const QUEUE_STOP_REQUESTED: &str = "Asked {job} to write its latest results and stop.";
const QUEUE_DRAIN_STARTED: &str =
    "Started the queue runner; it keeps running if you close this program.";
const QUEUE_DRAIN_RUNNING: &str = "The queue runner is already running.";
const QUEUE_DAMAGED: &str = "The queue file cannot be used: {reason}";
const JOB_QUEUED: &str = "{job} is waiting.";
const JOB_STARTING: &str = "{job} is starting on the compute host.";
const JOB_RUNNING: &str = "{job} is running: {line}";
const JOB_STOPPING: &str = "{job} is stopping ({cause}).";
const JOB_FINISHED: &str = "{job} finished and was recorded as {run}.";
const JOB_STOPPED: &str =
    "{job} stopped at time {time} and was recorded as {run}; it can be continued.";
const JOB_FAILED: &str = "{job} failed: {reason}";
const JOB_CANCELLED: &str = "{job} was cancelled before it started.";
const JOB_INTERRUPTED: &str = "{job} was interrupted: {reason}";
const JOB_UNREACHABLE: &str =
    "The compute host cannot be reached; {job} keeps running there and will be picked up again.";
const JOB_REATTACHED: &str = "Picked up {job} again where it left off.";
const CONTINUE_FROM: &str = "Continuing {run} from time {time}.";
const CONTINUE_REFUSED: &str = "The run cannot be continued: {reason}";
const PARALLEL_RUN: &str = "Running on {np} processes ({cells} cells).";
const STOPPED_RUN_NOTE: &str =
    "{run} is a stopped run (latest time {time}); it holds no finished result.";
const RUN_DIVERGED: &str = "The run diverged ({code}): {field} reached {value}, over the limit {limit}. It was stopped without writing the unstable fields.";
const SOLVER_MISSING_FILE: &str = "The solver stopped because a file it needs is missing ({code}). Export the case again, and check that every file the study refers to exists.";
const SOLVER_NUMERICAL_BLOWUP: &str = "The solution blew up ({code}): the numbers became unstable. Lower the relaxation factors, check the mesh quality, and check the boundary values.";
const SOLVER_UNKNOWN_TYPE: &str = "The solver does not know a name the case uses ({code}). Remove the raw override or monitor that uses it.";
const SOLVER_MISSING_KEYWORD: &str = "A required entry is missing in a case file ({code}). Add the entry, or remove the override that replaced a whole block.";
const SOLVER_LINEAR_NOT_CONVERGED: &str = "A linear solver did not reach its tolerance ({code}). Tighten the settings, improve the mesh, or relax more.";
const SOLVER_POOR_MESH: &str =
    "The mesh failed its quality checks ({code}). Refine it or fix the geometry, then run again.";
const SOLVER_OUT_OF_MEMORY: &str = "The solver ran out of memory ({code}). Use a smaller mesh, or ask for a host with more memory.";
const SOLVER_DISK_FULL: &str = "The disk on the compute host is full ({code}). Free space there; the disk floor should have stopped the run earlier.";
const SOLVER_DECOMPOSITION: &str = "The parallel decomposition does not match the case ({code}). Run the decomposition again for the same number of processes.";
const SOLVER_NOT_ENOUGH_SLOTS: &str = "The parallel run asked for more processes than the host allows ({code}). Lower the number of processes or raise the host's core cap.";
const SOLVER_SEGFAULT: &str =
    "The solver crashed ({code}). Report it with the last lines of the log.";
const SOLVER_UNKNOWN: &str = "The solver stopped with an error this program does not recognise (CFD-SOLVER-099). The last lines of its log:\n{tail}\nCheck the setup and the mesh, then run it again.";
const DISK_BELOW_FLOOR: &str = "The compute host has {free} GB free, below its floor of {floor} GB, so the run was not started.";
const DISK_STOPPED: &str = "The run was stopped because free disk on the compute host fell to {free} GB, the floor being {floor} GB. Its latest results were kept.";
const MONITOR_LINE: &str = "{name}: last {last}, tail mean {mean}, spread {spread}: {state}.";
const MONITOR_SETTLED: &str = "settled";
const MONITOR_DRIFTING: &str = "drifting";
const MONITOR_OSCILLATING: &str = "oscillating";
const MONITOR_TOO_EARLY: &str = "too early to tell";
const MONITOR_TOO_FEW_SAMPLES: &str = "too few samples to tell";
const MASS_BALANCE_LINE: &str = "Mass balance: {state} (imbalance {imbalance} of the inlet flux).";
const ENERGY_BALANCE_LINE: &str =
    "Energy balance: {state} (imbalance {imbalance} of the reference).";
const VERDICT_CONVERGED: &str =
    "Converged: every monitored quantity has settled and the balances close.";
const VERDICT_NOT_CONVERGED: &str = "Not converged: {reasons}.";
const VERDICT_NOT_JUDGED: &str = "Convergence cannot be judged: {reason}.";
const REASON_MONITOR: &str = "monitor {id} {from} → {to}";
const GCI_SUMMARY: &str = "Mesh independence of {quantity}: observed order {order}, GCI of the fine mesh {gci_percent} % ({class}).";
const GCI_ORDER_NONE: &str = "No grid convergence index for {quantity}: the three values are {class}, so no positive order of convergence exists.";
const GCI_WARN_OSCILLATORY: &str = "{quantity} converges in an oscillating way, so its index is less reliable than a monotone one.";
const GCI_WARN_ORDER: &str = "The observed order {order} of {quantity} is outside 0.5 to 2.5; check that the meshes are in the asymptotic range.";
const GCI_WARN_ASYMPTOTIC: &str = "The asymptotic ratio {ratio} of {quantity} is outside 0.9 to 1.1; the meshes may not be in the asymptotic range.";
const GCI_WARN_TRANSIENT: &str = "{quantity} comes from a time-dependent run; the index does not include the error of sampling a finite number of cycles.";
const GCI_WARN_WALL_FUNCTION: &str = "This study uses wall functions, so the near-wall cell size changes with the mesh and the order of {quantity} is less reliable.";
const GCI_WARN_NOISE: &str = "The noise in {quantity} is large against its change between meshes; the study is noise limited.";
const GCI_WARN_NOT_REFINED: &str =
    "The meshes for {quantity} are not refined by the same ratio each time.";
const GCI_CREATED: &str = "Created the mesh independence sweep {sweep} with {points} meshes.";
const GCI_WAITING: &str =
    "The mesh independence sweep {sweep} is not finished: {missing} have no recorded result.";
const BENCHMARK_PASSED: &str =
    "{benchmark}: every metric is within its tolerance ({metrics} checked).";
const BENCHMARK_FAILED: &str = "{benchmark}: outside tolerance on {failed}.";
const BENCHMARK_NOT_MATCHING: &str = "This study is not the {benchmark} benchmark: {differences}.";
const BENCHMARK_LISTED: &str = "{id}: {title} ({reference}).";
const BENCHMARK_CREATED: &str = "Wrote the {benchmark} study at the {mesh} mesh to {path}.";
const BENCHMARK_RESULTS_INCOMPLETE: &str =
    "The results lack {file}, which the {benchmark} comparison needs.";
const BENCHMARK_OVERRIDE_USED: &str =
    "A tolerance override was used for {benchmark}; the table shows the tolerance in force.";
const BENCHMARK_NOT_CONVERGED: &str = "Warning: the run behind this {benchmark} verdict is NOT CONVERGED ({reasons}). The verdict compares the numbers as they stand; it does not say they have settled.";
const BENCHMARK_CONVERGENCE_UNKNOWN: &str = "Warning: whether the run behind this {benchmark} verdict had settled is UNKNOWN ({reasons}). The verdict compares the numbers as they stand; it does not say they have settled.";
const REASON_MESH_LEVEL: &str = "mesh level {from} → {to}";
const REASON_GRADING: &str = "grading in {axis} {from} → {to}";
const REASON_SHAPE: &str = "domain shape {from} → {to}";
const REASON_INLET_PROFILE: &str = "{boundary} inlet profile {from} → {to}";
const REASON_PHYSICS: &str = "{what} {from} → {to}";
const REASON_THERMAL: &str = "{boundary} thermal condition {from} → {to}";

/// Every English source text: the plain templates and both forms of each plural pair.
pub const SOURCES: &[&str] = &[
    MIGRATED,
    SAVED,
    OPENED,
    NEWER_SCHEMA,
    NOT_A_STUDY_FILE,
    LINK_MISSING,
    LINK_CHANGED,
    LINK_REFUSED,
    LINK_ADDED,
    RESULT_MISSING,
    RESULT_CORRUPT,
    PACKED_ONE,
    PACKED_OTHER,
    UNPACKED_ONE,
    UNPACKED_OTHER,
    LINKS_NOT_PACKED_ONE,
    LINKS_NOT_PACKED_OTHER,
    TARGET_EXISTS,
    PACK_MISSING,
    PACK_DAMAGED,
    PACK_INCOMPLETE,
    NOT_WRITTEN,
    LINK_NOT_ADDED,
    STALE_BECAUSE,
    REASON_LENGTH,
    REASON_DOMAIN_OPENED,
    REASON_DOMAIN_CLOSED,
    REASON_CELLS,
    REASON_LAYOUT,
    REASON_VISCOSITY,
    REASON_BOUNDARY_VELOCITY,
    REASON_BOUNDARY_KIND,
    REASON_BOUNDARY_FACES,
    REASON_BOUNDARY_ADDED,
    REASON_BOUNDARY_REMOVED,
    REASON_END_TIME,
    REASON_WRITE_INTERVAL,
    REASON_PRESSURE_LIMIT,
    REASON_VELOCITY_LIMIT,
    REASON_SAMPLES,
    REASON_IMAGE,
    REASON_DIALECT,
    REASON_SIDECAR,
    REASON_WRITER,
    REASON_MESH,
    STAGE_UP_TO_DATE,
    STAGE_NEVER_MADE,
    STAGE_BLOB_MISSING,
    RUN_RECORDED,
    NOTHING_TO_RUN,
    RERUN_REPRODUCED,
    RERUN_WITHIN,
    RERUN_DIFFERS,
    TOOL_MISMATCH,
    UNKNOWN_RUN,
    NO_RUNS,
    RUN_HAS_NO_RESULT,
    RECORDS_INCONSISTENT,
    NOT_PENDING,
    UNKNOWN_ITEM,
    NEEDS_UPSTREAM,
    ORDER_MISMATCH,
    ORDER_BREAKS_DEPENDENCY,
    ITEM_UPDATED,
    NOTHING_TO_UPDATE,
    MIGRATED_AND_SAVED,
    OUTPUT_NOT_RECORDED,
    DUPLICATE_NAME,
    SCENARIO_CREATED,
    SCENARIO_UPDATED,
    SCENARIO_LABELLED,
    SCENARIO_UNKNOWN,
    SCENARIO_REFUSED,
    OVERRIDE_NOT_APPLIED,
    SCENARIO_DIFFERS,
    SCENARIO_SAME,
    REASON_RAW_OVERRIDE,
    REASON_RAW_MESH,
    SWEEP_TOO_LARGE,
    SWEEP_REFUSED,
    SWEEP_CREATED,
    SWEEP_UNKNOWN,
    PREFLIGHT_POINT_FAILED,
    PREFLIGHT_PASSED,
    PREFLIGHT_FAILED,
    DISK_TOTAL,
    PRUNE_NOTHING,
    PRUNE_DRY_RUN,
    PRUNED,
    STAGE_PRUNED,
    PRUNE_REFUSED,
    PRUNE_NOT_REMOVED,
    QUEUE_ADDED,
    QUEUE_NOTHING_TO_ADD,
    QUEUE_REFUSED,
    QUEUE_UNKNOWN_JOB,
    QUEUE_NOT_ALLOWED,
    QUEUE_REORDERED,
    QUEUE_CANCELLED,
    QUEUE_STOP_REQUESTED,
    QUEUE_DRAIN_STARTED,
    QUEUE_DRAIN_RUNNING,
    QUEUE_DAMAGED,
    JOB_QUEUED,
    JOB_STARTING,
    JOB_RUNNING,
    JOB_STOPPING,
    JOB_FINISHED,
    JOB_STOPPED,
    JOB_FAILED,
    JOB_CANCELLED,
    JOB_INTERRUPTED,
    JOB_UNREACHABLE,
    JOB_REATTACHED,
    CONTINUE_FROM,
    CONTINUE_REFUSED,
    PARALLEL_RUN,
    STOPPED_RUN_NOTE,
    RUN_DIVERGED,
    SOLVER_MISSING_FILE,
    SOLVER_NUMERICAL_BLOWUP,
    SOLVER_UNKNOWN_TYPE,
    SOLVER_MISSING_KEYWORD,
    SOLVER_LINEAR_NOT_CONVERGED,
    SOLVER_POOR_MESH,
    SOLVER_OUT_OF_MEMORY,
    SOLVER_DISK_FULL,
    SOLVER_DECOMPOSITION,
    SOLVER_NOT_ENOUGH_SLOTS,
    SOLVER_SEGFAULT,
    SOLVER_UNKNOWN,
    DISK_BELOW_FLOOR,
    DISK_STOPPED,
    MONITOR_LINE,
    MONITOR_SETTLED,
    MONITOR_DRIFTING,
    MONITOR_OSCILLATING,
    MONITOR_TOO_EARLY,
    MONITOR_TOO_FEW_SAMPLES,
    MASS_BALANCE_LINE,
    ENERGY_BALANCE_LINE,
    VERDICT_CONVERGED,
    VERDICT_NOT_CONVERGED,
    VERDICT_NOT_JUDGED,
    REASON_MONITOR,
    GCI_SUMMARY,
    GCI_ORDER_NONE,
    GCI_WARN_OSCILLATORY,
    GCI_WARN_ORDER,
    GCI_WARN_ASYMPTOTIC,
    GCI_WARN_TRANSIENT,
    GCI_WARN_WALL_FUNCTION,
    GCI_WARN_NOISE,
    GCI_WARN_NOT_REFINED,
    GCI_CREATED,
    GCI_WAITING,
    BENCHMARK_PASSED,
    BENCHMARK_FAILED,
    BENCHMARK_NOT_MATCHING,
    BENCHMARK_LISTED,
    BENCHMARK_CREATED,
    BENCHMARK_RESULTS_INCOMPLETE,
    BENCHMARK_OVERRIDE_USED,
    BENCHMARK_NOT_CONVERGED,
    BENCHMARK_CONVERGENCE_UNKNOWN,
    REASON_MESH_LEVEL,
    REASON_GRADING,
    REASON_SHAPE,
    REASON_INLET_PROFILE,
    REASON_PHYSICS,
    REASON_THERMAL,
];

/// One thing the document layer tells a person. Data only: the sentence is made by
/// [`Text::render`](crate::Text::render).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// An older file was brought up to the current schema in memory.
    Migrated {
        from: u32,
        to: u32,
        summary: String,
    },
    Saved {
        path: String,
    },
    Opened {
        path: String,
    },
    /// The file is from a newer version than this one reads.
    NewerSchema {
        found: u32,
        supported: u32,
    },
    NotAStudyFile {
        path: String,
        reason: String,
    },
    LinkMissing {
        name: String,
        expected: String,
    },
    LinkChanged {
        name: String,
    },
    LinkRefused {
        name: String,
        reason: String,
    },
    LinkAdded {
        name: String,
        path: String,
    },
    /// A stored result named by its fingerprint is not in the store.
    ResultMissing {
        hash: String,
    },
    /// A stored result does not match its fingerprint.
    ResultCorrupt {
        hash: String,
    },
    Packed {
        count: u64,
        path: String,
    },
    Unpacked {
        count: u64,
        path: String,
    },
    LinksNotPacked {
        count: u64,
    },
    TargetExists {
        path: String,
    },
    PackMissing {
        hash: String,
    },
    PackDamaged {
        hash: String,
    },
    PackIncomplete {
        hash: String,
    },
    /// A file could not be written.
    NotWritten {
        path: String,
        reason: String,
    },
    /// A file could not be linked to the study.
    LinkNotAdded {
        name: String,
        reason: String,
    },
    /// A stage is out of date; `reason` is the joined reason sentences.
    StaleBecause {
        stage: String,
        reason: String,
    },
    ReasonLength {
        axis: String,
        from: String,
        to: String,
    },
    ReasonDomainOpened,
    ReasonDomainClosed,
    ReasonCells {
        axis: String,
        from: String,
        to: String,
    },
    ReasonLayout,
    ReasonViscosity {
        from: String,
        to: String,
    },
    ReasonBoundaryVelocity {
        boundary: String,
        from: String,
        to: String,
    },
    ReasonBoundaryKind {
        boundary: String,
        from: String,
        to: String,
    },
    ReasonBoundaryFaces {
        boundary: String,
        from: String,
        to: String,
    },
    ReasonBoundaryAdded {
        boundary: String,
    },
    ReasonBoundaryRemoved {
        boundary: String,
    },
    ReasonEndTime {
        from: String,
        to: String,
    },
    ReasonWriteInterval {
        from: String,
        to: String,
    },
    ReasonPressureLimit {
        from: String,
        to: String,
    },
    ReasonVelocityLimit {
        from: String,
        to: String,
    },
    ReasonSamples {
        from: String,
        to: String,
    },
    ReasonImage {
        from: String,
        to: String,
    },
    ReasonDialect {
        from: String,
        to: String,
    },
    ReasonSidecar {
        from: String,
        to: String,
    },
    ReasonWriter {
        from: String,
        to: String,
    },
    ReasonMesh,
    StageUpToDate {
        stage: String,
        run: String,
    },
    StageNeverMade {
        stage: String,
    },
    /// A recorded stage output is not in the store.
    StageBlobMissing {
        stage: String,
        hash: String,
    },
    RunRecorded {
        run: String,
        stages: String,
    },
    NothingToRun,
    RerunReproduced {
        run: String,
        of: String,
    },
    RerunWithin {
        run: String,
        of: String,
        max_diff: String,
    },
    /// `count` is the number of quantities outside the tolerance.
    RerunDiffers {
        run: String,
        of: String,
        count: u64,
    },
    ToolMismatch {
        reasons: String,
    },
    UnknownRun {
        run: String,
    },
    NoRuns,
    RunHasNoResult {
        run: String,
    },
    RecordsInconsistent {
        run: String,
        reason: String,
    },
    NotPending {
        item: String,
    },
    UnknownItem {
        item: String,
    },
    NeedsUpstream {
        item: String,
        needs: String,
    },
    OrderMismatch,
    OrderBreaksDependency {
        item: String,
        needs: String,
    },
    ItemUpdated {
        item: String,
        run: String,
    },
    NothingToUpdate,
    /// An older file was brought up to the current schema and saved.
    MigratedAndSaved {
        from: u32,
        to: u32,
        summary: String,
    },
    OutputNotRecorded {
        reason: String,
    },
    DuplicateName {
        name: String,
    },
    /// A variant was made from its parent.
    ScenarioCreated {
        scenario: String,
        parent: String,
    },
    /// An existing variant's changes were replaced.
    ScenarioUpdated {
        scenario: String,
    },
    /// The name, pin or notes of a variant changed.
    ScenarioLabelled {
        scenario: String,
    },
    ScenarioUnknown {
        scenario: String,
    },
    /// The list of variants broke a rule.
    ScenarioRefused {
        reason: String,
    },
    /// A change of a variant does not fit the study it is applied to.
    OverrideNotApplied {
        scenario: String,
        setting: String,
        reason: String,
    },
    /// Two variants differ; `reasons` is the joined reason sentences.
    ScenarioDiffers {
        from: String,
        to: String,
        reasons: String,
    },
    ScenarioSame {
        from: String,
        to: String,
    },
    /// A raw override changed; each side is its text or `none`.
    ReasonRawOverride {
        id: String,
        from: String,
        to: String,
    },
    ReasonRawMesh,
    SweepTooLarge {
        sweep: String,
        points: String,
        max: String,
    },
    SweepRefused {
        sweep: String,
        reason: String,
    },
    SweepCreated {
        sweep: String,
        points: String,
    },
    SweepUnknown {
        sweep: String,
    },
    /// One point of a sweep fails the checks made before solving.
    PreflightPointFailed {
        point: String,
        reason: String,
    },
    PreflightPassed {
        sweep: String,
        points: String,
        meshes: String,
    },
    PreflightFailed {
        sweep: String,
        failed: String,
        points: String,
    },
    DiskTotal {
        bytes: String,
        scenarios: String,
    },
    PruneNothing,
    /// A pruning that was only planned.
    PruneDryRun {
        blobs: String,
        bytes: String,
    },
    /// A pruning that was carried out.
    Pruned {
        blobs: String,
        bytes: String,
    },
    /// A stage whose stored output was pruned, so it is not out of date.
    StagePruned {
        stage: String,
        run: String,
    },
    PruneRefused {
        reason: String,
    },
    PruneNotRemoved {
        reason: String,
    },
    QueueAdded {
        jobs: String,
        queued: String,
    },
    QueueNothingToAdd,
    QueueRefused {
        reason: String,
    },
    QueueUnknownJob {
        job: String,
    },
    QueueNotAllowed {
        job: String,
        state: String,
        action: String,
    },
    QueueReordered {
        job: String,
        position: String,
    },
    QueueCancelled {
        job: String,
    },
    QueueStopRequested {
        job: String,
    },
    QueueDrainStarted,
    QueueDrainRunning,
    QueueDamaged {
        reason: String,
    },
    JobQueued {
        job: String,
    },
    JobStarting {
        job: String,
    },
    JobRunning {
        job: String,
        line: String,
    },
    JobStopping {
        job: String,
        cause: String,
    },
    JobFinished {
        job: String,
        run: String,
    },
    JobStopped {
        job: String,
        run: String,
        time: String,
    },
    JobFailed {
        job: String,
        reason: String,
    },
    JobCancelled {
        job: String,
    },
    JobInterrupted {
        job: String,
        reason: String,
    },
    JobUnreachable {
        job: String,
    },
    JobReattached {
        job: String,
    },
    ContinueFrom {
        run: String,
        time: String,
    },
    ContinueRefused {
        reason: String,
    },
    ParallelRun {
        np: String,
        cells: String,
    },
    StoppedRunNote {
        run: String,
        time: String,
    },
    RunDiverged {
        code: String,
        field: String,
        value: String,
        limit: String,
    },
    SolverMissingFile {
        code: String,
    },
    SolverNumericalBlowup {
        code: String,
    },
    SolverUnknownType {
        code: String,
    },
    SolverMissingKeyword {
        code: String,
    },
    SolverLinearNotConverged {
        code: String,
    },
    SolverPoorMesh {
        code: String,
    },
    SolverOutOfMemory {
        code: String,
    },
    SolverDiskFull {
        code: String,
    },
    SolverDecomposition {
        code: String,
    },
    SolverNotEnoughSlots {
        code: String,
    },
    SolverSegfault {
        code: String,
    },
    SolverUnknown {
        tail: String,
    },
    DiskBelowFloor {
        free: String,
        floor: String,
    },
    DiskStopped {
        free: String,
        floor: String,
    },
    MonitorLine {
        name: String,
        last: String,
        mean: String,
        spread: String,
        state: String,
    },
    MonitorSettled,
    MonitorDrifting,
    MonitorOscillating,
    MonitorTooEarly,
    MonitorTooFewSamples,
    MassBalanceLine {
        state: String,
        imbalance: String,
    },
    EnergyBalanceLine {
        state: String,
        imbalance: String,
    },
    VerdictConverged,
    VerdictNotConverged {
        reasons: String,
    },
    VerdictNotJudged {
        reason: String,
    },
    ReasonMonitor {
        id: String,
        from: String,
        to: String,
    },
    GciSummary {
        quantity: String,
        order: String,
        gci_percent: String,
        class: String,
    },
    GciOrderNone {
        quantity: String,
        class: String,
    },
    GciWarnOscillatory {
        quantity: String,
    },
    GciWarnOrder {
        quantity: String,
        order: String,
    },
    GciWarnAsymptotic {
        quantity: String,
        ratio: String,
    },
    GciWarnTransient {
        quantity: String,
    },
    GciWarnWallFunction {
        quantity: String,
    },
    GciWarnNoise {
        quantity: String,
    },
    GciWarnNotRefined {
        quantity: String,
    },
    GciCreated {
        sweep: String,
        points: String,
    },
    GciWaiting {
        sweep: String,
        missing: String,
    },
    BenchmarkPassed {
        benchmark: String,
        metrics: String,
    },
    BenchmarkFailed {
        benchmark: String,
        failed: String,
    },
    BenchmarkNotMatching {
        benchmark: String,
        differences: String,
    },
    BenchmarkListed {
        id: String,
        title: String,
        reference: String,
    },
    BenchmarkCreated {
        benchmark: String,
        mesh: String,
        path: String,
    },
    BenchmarkResultsIncomplete {
        benchmark: String,
        file: String,
    },
    BenchmarkOverrideUsed {
        benchmark: String,
    },
    /// The run a benchmark verdict was made on is not converged by its own assessment.
    BenchmarkNotConverged {
        benchmark: String,
        reasons: String,
    },
    /// The run a benchmark verdict was made on has no usable convergence assessment.
    BenchmarkConvergenceUnknown {
        benchmark: String,
        reasons: String,
    },
    ReasonMeshLevel {
        from: String,
        to: String,
    },
    ReasonGrading {
        axis: String,
        from: String,
        to: String,
    },
    ReasonShape {
        from: String,
        to: String,
    },
    ReasonInletProfile {
        boundary: String,
        from: String,
        to: String,
    },
    ReasonPhysics {
        what: String,
        from: String,
        to: String,
    },
    ReasonThermal {
        boundary: String,
        from: String,
        to: String,
    },
}

impl Line {
    /// The English source text, which is also the catalog key. For a line with a count this is
    /// the singular form; see [`Line::plural_source`].
    pub fn source(&self) -> &'static str {
        match self {
            Line::Migrated { .. } => MIGRATED,
            Line::Saved { .. } => SAVED,
            Line::Opened { .. } => OPENED,
            Line::NewerSchema { .. } => NEWER_SCHEMA,
            Line::NotAStudyFile { .. } => NOT_A_STUDY_FILE,
            Line::LinkMissing { .. } => LINK_MISSING,
            Line::LinkChanged { .. } => LINK_CHANGED,
            Line::LinkRefused { .. } => LINK_REFUSED,
            Line::LinkAdded { .. } => LINK_ADDED,
            Line::ResultMissing { .. } => RESULT_MISSING,
            Line::ResultCorrupt { .. } => RESULT_CORRUPT,
            Line::Packed { .. } => PACKED_ONE,
            Line::Unpacked { .. } => UNPACKED_ONE,
            Line::LinksNotPacked { .. } => LINKS_NOT_PACKED_ONE,
            Line::TargetExists { .. } => TARGET_EXISTS,
            Line::PackMissing { .. } => PACK_MISSING,
            Line::PackDamaged { .. } => PACK_DAMAGED,
            Line::PackIncomplete { .. } => PACK_INCOMPLETE,
            Line::NotWritten { .. } => NOT_WRITTEN,
            Line::LinkNotAdded { .. } => LINK_NOT_ADDED,
            Line::StaleBecause { .. } => STALE_BECAUSE,
            Line::ReasonLength { .. } => REASON_LENGTH,
            Line::ReasonDomainOpened => REASON_DOMAIN_OPENED,
            Line::ReasonDomainClosed => REASON_DOMAIN_CLOSED,
            Line::ReasonCells { .. } => REASON_CELLS,
            Line::ReasonLayout => REASON_LAYOUT,
            Line::ReasonViscosity { .. } => REASON_VISCOSITY,
            Line::ReasonBoundaryVelocity { .. } => REASON_BOUNDARY_VELOCITY,
            Line::ReasonBoundaryKind { .. } => REASON_BOUNDARY_KIND,
            Line::ReasonBoundaryFaces { .. } => REASON_BOUNDARY_FACES,
            Line::ReasonBoundaryAdded { .. } => REASON_BOUNDARY_ADDED,
            Line::ReasonBoundaryRemoved { .. } => REASON_BOUNDARY_REMOVED,
            Line::ReasonEndTime { .. } => REASON_END_TIME,
            Line::ReasonWriteInterval { .. } => REASON_WRITE_INTERVAL,
            Line::ReasonPressureLimit { .. } => REASON_PRESSURE_LIMIT,
            Line::ReasonVelocityLimit { .. } => REASON_VELOCITY_LIMIT,
            Line::ReasonSamples { .. } => REASON_SAMPLES,
            Line::ReasonImage { .. } => REASON_IMAGE,
            Line::ReasonDialect { .. } => REASON_DIALECT,
            Line::ReasonSidecar { .. } => REASON_SIDECAR,
            Line::ReasonWriter { .. } => REASON_WRITER,
            Line::ReasonMesh => REASON_MESH,
            Line::StageUpToDate { .. } => STAGE_UP_TO_DATE,
            Line::StageNeverMade { .. } => STAGE_NEVER_MADE,
            Line::StageBlobMissing { .. } => STAGE_BLOB_MISSING,
            Line::RunRecorded { .. } => RUN_RECORDED,
            Line::NothingToRun => NOTHING_TO_RUN,
            Line::RerunReproduced { .. } => RERUN_REPRODUCED,
            Line::RerunWithin { .. } => RERUN_WITHIN,
            Line::RerunDiffers { .. } => RERUN_DIFFERS,
            Line::ToolMismatch { .. } => TOOL_MISMATCH,
            Line::UnknownRun { .. } => UNKNOWN_RUN,
            Line::NoRuns => NO_RUNS,
            Line::RunHasNoResult { .. } => RUN_HAS_NO_RESULT,
            Line::RecordsInconsistent { .. } => RECORDS_INCONSISTENT,
            Line::NotPending { .. } => NOT_PENDING,
            Line::UnknownItem { .. } => UNKNOWN_ITEM,
            Line::NeedsUpstream { .. } => NEEDS_UPSTREAM,
            Line::OrderMismatch => ORDER_MISMATCH,
            Line::OrderBreaksDependency { .. } => ORDER_BREAKS_DEPENDENCY,
            Line::ItemUpdated { .. } => ITEM_UPDATED,
            Line::NothingToUpdate => NOTHING_TO_UPDATE,
            Line::MigratedAndSaved { .. } => MIGRATED_AND_SAVED,
            Line::OutputNotRecorded { .. } => OUTPUT_NOT_RECORDED,
            Line::DuplicateName { .. } => DUPLICATE_NAME,
            Line::ScenarioCreated { .. } => SCENARIO_CREATED,
            Line::ScenarioUpdated { .. } => SCENARIO_UPDATED,
            Line::ScenarioLabelled { .. } => SCENARIO_LABELLED,
            Line::ScenarioUnknown { .. } => SCENARIO_UNKNOWN,
            Line::ScenarioRefused { .. } => SCENARIO_REFUSED,
            Line::OverrideNotApplied { .. } => OVERRIDE_NOT_APPLIED,
            Line::ScenarioDiffers { .. } => SCENARIO_DIFFERS,
            Line::ScenarioSame { .. } => SCENARIO_SAME,
            Line::ReasonRawOverride { .. } => REASON_RAW_OVERRIDE,
            Line::ReasonRawMesh => REASON_RAW_MESH,
            Line::SweepTooLarge { .. } => SWEEP_TOO_LARGE,
            Line::SweepRefused { .. } => SWEEP_REFUSED,
            Line::SweepCreated { .. } => SWEEP_CREATED,
            Line::SweepUnknown { .. } => SWEEP_UNKNOWN,
            Line::PreflightPointFailed { .. } => PREFLIGHT_POINT_FAILED,
            Line::PreflightPassed { .. } => PREFLIGHT_PASSED,
            Line::PreflightFailed { .. } => PREFLIGHT_FAILED,
            Line::DiskTotal { .. } => DISK_TOTAL,
            Line::PruneNothing => PRUNE_NOTHING,
            Line::PruneDryRun { .. } => PRUNE_DRY_RUN,
            Line::Pruned { .. } => PRUNED,
            Line::StagePruned { .. } => STAGE_PRUNED,
            Line::PruneRefused { .. } => PRUNE_REFUSED,
            Line::PruneNotRemoved { .. } => PRUNE_NOT_REMOVED,
            Line::QueueAdded { .. } => QUEUE_ADDED,
            Line::QueueNothingToAdd => QUEUE_NOTHING_TO_ADD,
            Line::QueueRefused { .. } => QUEUE_REFUSED,
            Line::QueueUnknownJob { .. } => QUEUE_UNKNOWN_JOB,
            Line::QueueNotAllowed { .. } => QUEUE_NOT_ALLOWED,
            Line::QueueReordered { .. } => QUEUE_REORDERED,
            Line::QueueCancelled { .. } => QUEUE_CANCELLED,
            Line::QueueStopRequested { .. } => QUEUE_STOP_REQUESTED,
            Line::QueueDrainStarted => QUEUE_DRAIN_STARTED,
            Line::QueueDrainRunning => QUEUE_DRAIN_RUNNING,
            Line::QueueDamaged { .. } => QUEUE_DAMAGED,
            Line::JobQueued { .. } => JOB_QUEUED,
            Line::JobStarting { .. } => JOB_STARTING,
            Line::JobRunning { .. } => JOB_RUNNING,
            Line::JobStopping { .. } => JOB_STOPPING,
            Line::JobFinished { .. } => JOB_FINISHED,
            Line::JobStopped { .. } => JOB_STOPPED,
            Line::JobFailed { .. } => JOB_FAILED,
            Line::JobCancelled { .. } => JOB_CANCELLED,
            Line::JobInterrupted { .. } => JOB_INTERRUPTED,
            Line::JobUnreachable { .. } => JOB_UNREACHABLE,
            Line::JobReattached { .. } => JOB_REATTACHED,
            Line::ContinueFrom { .. } => CONTINUE_FROM,
            Line::ContinueRefused { .. } => CONTINUE_REFUSED,
            Line::ParallelRun { .. } => PARALLEL_RUN,
            Line::StoppedRunNote { .. } => STOPPED_RUN_NOTE,
            Line::RunDiverged { .. } => RUN_DIVERGED,
            Line::SolverMissingFile { .. } => SOLVER_MISSING_FILE,
            Line::SolverNumericalBlowup { .. } => SOLVER_NUMERICAL_BLOWUP,
            Line::SolverUnknownType { .. } => SOLVER_UNKNOWN_TYPE,
            Line::SolverMissingKeyword { .. } => SOLVER_MISSING_KEYWORD,
            Line::SolverLinearNotConverged { .. } => SOLVER_LINEAR_NOT_CONVERGED,
            Line::SolverPoorMesh { .. } => SOLVER_POOR_MESH,
            Line::SolverOutOfMemory { .. } => SOLVER_OUT_OF_MEMORY,
            Line::SolverDiskFull { .. } => SOLVER_DISK_FULL,
            Line::SolverDecomposition { .. } => SOLVER_DECOMPOSITION,
            Line::SolverNotEnoughSlots { .. } => SOLVER_NOT_ENOUGH_SLOTS,
            Line::SolverSegfault { .. } => SOLVER_SEGFAULT,
            Line::SolverUnknown { .. } => SOLVER_UNKNOWN,
            Line::DiskBelowFloor { .. } => DISK_BELOW_FLOOR,
            Line::DiskStopped { .. } => DISK_STOPPED,
            Line::MonitorLine { .. } => MONITOR_LINE,
            Line::MonitorSettled => MONITOR_SETTLED,
            Line::MonitorDrifting => MONITOR_DRIFTING,
            Line::MonitorOscillating => MONITOR_OSCILLATING,
            Line::MonitorTooEarly => MONITOR_TOO_EARLY,
            Line::MonitorTooFewSamples => MONITOR_TOO_FEW_SAMPLES,
            Line::MassBalanceLine { .. } => MASS_BALANCE_LINE,
            Line::EnergyBalanceLine { .. } => ENERGY_BALANCE_LINE,
            Line::VerdictConverged => VERDICT_CONVERGED,
            Line::VerdictNotConverged { .. } => VERDICT_NOT_CONVERGED,
            Line::VerdictNotJudged { .. } => VERDICT_NOT_JUDGED,
            Line::ReasonMonitor { .. } => REASON_MONITOR,
            Line::GciSummary { .. } => GCI_SUMMARY,
            Line::GciOrderNone { .. } => GCI_ORDER_NONE,
            Line::GciWarnOscillatory { .. } => GCI_WARN_OSCILLATORY,
            Line::GciWarnOrder { .. } => GCI_WARN_ORDER,
            Line::GciWarnAsymptotic { .. } => GCI_WARN_ASYMPTOTIC,
            Line::GciWarnTransient { .. } => GCI_WARN_TRANSIENT,
            Line::GciWarnWallFunction { .. } => GCI_WARN_WALL_FUNCTION,
            Line::GciWarnNoise { .. } => GCI_WARN_NOISE,
            Line::GciWarnNotRefined { .. } => GCI_WARN_NOT_REFINED,
            Line::GciCreated { .. } => GCI_CREATED,
            Line::GciWaiting { .. } => GCI_WAITING,
            Line::BenchmarkPassed { .. } => BENCHMARK_PASSED,
            Line::BenchmarkFailed { .. } => BENCHMARK_FAILED,
            Line::BenchmarkNotMatching { .. } => BENCHMARK_NOT_MATCHING,
            Line::BenchmarkListed { .. } => BENCHMARK_LISTED,
            Line::BenchmarkCreated { .. } => BENCHMARK_CREATED,
            Line::BenchmarkResultsIncomplete { .. } => BENCHMARK_RESULTS_INCOMPLETE,
            Line::BenchmarkOverrideUsed { .. } => BENCHMARK_OVERRIDE_USED,
            Line::BenchmarkNotConverged { .. } => BENCHMARK_NOT_CONVERGED,
            Line::BenchmarkConvergenceUnknown { .. } => BENCHMARK_CONVERGENCE_UNKNOWN,
            Line::ReasonMeshLevel { .. } => REASON_MESH_LEVEL,
            Line::ReasonGrading { .. } => REASON_GRADING,
            Line::ReasonShape { .. } => REASON_SHAPE,
            Line::ReasonInletProfile { .. } => REASON_INLET_PROFILE,
            Line::ReasonPhysics { .. } => REASON_PHYSICS,
            Line::ReasonThermal { .. } => REASON_THERMAL,
        }
    }

    /// The plural form of the source text for a line with a count, else `None`.
    pub fn plural_source(&self) -> Option<&'static str> {
        match self {
            Line::Packed { .. } => Some(PACKED_OTHER),
            Line::Unpacked { .. } => Some(UNPACKED_OTHER),
            Line::LinksNotPacked { .. } => Some(LINKS_NOT_PACKED_OTHER),
            _ => None,
        }
    }

    /// The count of a plural line.
    pub(crate) fn count(&self) -> Option<u64> {
        match self {
            Line::Packed { count, .. }
            | Line::Unpacked { count, .. }
            | Line::LinksNotPacked { count } => Some(*count),
            _ => None,
        }
    }

    /// The values to put into the placeholders, by name (the count is filled separately).
    pub(crate) fn args(&self) -> Vec<(&'static str, String)> {
        let s = |v: &String| v.clone();
        match self {
            Line::Migrated { from, to, summary } => vec![
                ("from", from.to_string()),
                ("to", to.to_string()),
                ("summary", s(summary)),
            ],
            Line::Saved { path } | Line::Opened { path } | Line::TargetExists { path } => {
                vec![("path", s(path))]
            }
            Line::NewerSchema { found, supported } => vec![
                ("found", found.to_string()),
                ("supported", supported.to_string()),
            ],
            Line::NotAStudyFile { path, reason } | Line::NotWritten { path, reason } => {
                vec![("path", s(path)), ("reason", s(reason))]
            }
            Line::LinkMissing { name, expected } => {
                vec![("name", s(name)), ("expected", s(expected))]
            }
            Line::LinkChanged { name } => vec![("name", s(name))],
            Line::LinkRefused { name, reason } | Line::LinkNotAdded { name, reason } => {
                vec![("name", s(name)), ("reason", s(reason))]
            }
            Line::LinkAdded { name, path } => vec![("name", s(name)), ("path", s(path))],
            Line::ResultMissing { hash }
            | Line::ResultCorrupt { hash }
            | Line::PackMissing { hash }
            | Line::PackDamaged { hash }
            | Line::PackIncomplete { hash } => vec![("hash", s(hash))],
            Line::Packed { path, .. } | Line::Unpacked { path, .. } => vec![("path", s(path))],
            Line::LinksNotPacked { .. } => Vec::new(),
            Line::StaleBecause { stage, reason } => {
                vec![("stage", s(stage)), ("reason", s(reason))]
            }
            Line::ReasonLength { axis, from, to } | Line::ReasonCells { axis, from, to } => {
                vec![("axis", s(axis)), ("from", s(from)), ("to", s(to))]
            }
            Line::ReasonBoundaryVelocity { boundary, from, to }
            | Line::ReasonBoundaryKind { boundary, from, to }
            | Line::ReasonBoundaryFaces { boundary, from, to } => {
                vec![("boundary", s(boundary)), ("from", s(from)), ("to", s(to))]
            }
            Line::ReasonBoundaryAdded { boundary } | Line::ReasonBoundaryRemoved { boundary } => {
                vec![("boundary", s(boundary))]
            }
            Line::ReasonViscosity { from, to }
            | Line::ReasonEndTime { from, to }
            | Line::ReasonWriteInterval { from, to }
            | Line::ReasonPressureLimit { from, to }
            | Line::ReasonVelocityLimit { from, to }
            | Line::ReasonSamples { from, to }
            | Line::ReasonImage { from, to }
            | Line::ReasonDialect { from, to }
            | Line::ReasonSidecar { from, to }
            | Line::ReasonWriter { from, to } => vec![("from", s(from)), ("to", s(to))],
            Line::ReasonDomainOpened
            | Line::ReasonDomainClosed
            | Line::ReasonLayout
            | Line::ReasonMesh => Vec::new(),
            Line::StageUpToDate { stage, run } => vec![("stage", s(stage)), ("run", s(run))],
            Line::StageNeverMade { stage } => vec![("stage", s(stage))],
            Line::StageBlobMissing { stage, hash } => {
                vec![("stage", s(stage)), ("hash", s(hash))]
            }
            Line::RunRecorded { run, stages } => vec![("run", s(run)), ("stages", s(stages))],
            Line::RerunReproduced { run, of } => vec![("run", s(run)), ("of", s(of))],
            Line::RerunWithin { run, of, max_diff } => {
                vec![("run", s(run)), ("of", s(of)), ("max_diff", s(max_diff))]
            }
            Line::RerunDiffers { run, of, count } => {
                vec![("run", s(run)), ("of", s(of)), ("count", count.to_string())]
            }
            Line::ToolMismatch { reasons } => vec![("reasons", s(reasons))],
            Line::UnknownRun { run } | Line::RunHasNoResult { run } => vec![("run", s(run))],
            Line::RecordsInconsistent { run, reason } => {
                vec![("run", s(run)), ("reason", s(reason))]
            }
            Line::NotPending { item } | Line::UnknownItem { item } => vec![("item", s(item))],
            Line::NeedsUpstream { item, needs } | Line::OrderBreaksDependency { item, needs } => {
                vec![("item", s(item)), ("needs", s(needs))]
            }
            Line::ItemUpdated { item, run } => vec![("item", s(item)), ("run", s(run))],
            Line::MigratedAndSaved { from, to, summary } => vec![
                ("from", from.to_string()),
                ("to", to.to_string()),
                ("summary", s(summary)),
            ],
            Line::OutputNotRecorded { reason } => vec![("reason", s(reason))],
            Line::DuplicateName { name } => vec![("name", s(name))],
            Line::NothingToRun | Line::NoRuns | Line::OrderMismatch | Line::NothingToUpdate => {
                Vec::new()
            }
            Line::ScenarioCreated { scenario, parent } => {
                vec![("scenario", s(scenario)), ("parent", s(parent))]
            }
            Line::ScenarioUpdated { scenario }
            | Line::ScenarioLabelled { scenario }
            | Line::ScenarioUnknown { scenario } => vec![("scenario", s(scenario))],
            Line::ScenarioRefused { reason }
            | Line::PruneRefused { reason }
            | Line::PruneNotRemoved { reason } => vec![("reason", s(reason))],
            Line::OverrideNotApplied {
                scenario,
                setting,
                reason,
            } => vec![
                ("scenario", s(scenario)),
                ("setting", s(setting)),
                ("reason", s(reason)),
            ],
            Line::ScenarioDiffers { from, to, reasons } => {
                vec![("from", s(from)), ("to", s(to)), ("reasons", s(reasons))]
            }
            Line::ScenarioSame { from, to } => vec![("from", s(from)), ("to", s(to))],
            Line::ReasonRawOverride { id, from, to } => {
                vec![("id", s(id)), ("from", s(from)), ("to", s(to))]
            }
            Line::ReasonRawMesh | Line::PruneNothing => Vec::new(),
            Line::SweepTooLarge { sweep, points, max } => {
                vec![("sweep", s(sweep)), ("points", s(points)), ("max", s(max))]
            }
            Line::SweepRefused { sweep, reason } => {
                vec![("sweep", s(sweep)), ("reason", s(reason))]
            }
            Line::SweepCreated { sweep, points } => {
                vec![("sweep", s(sweep)), ("points", s(points))]
            }
            Line::SweepUnknown { sweep } => vec![("sweep", s(sweep))],
            Line::PreflightPointFailed { point, reason } => {
                vec![("point", s(point)), ("reason", s(reason))]
            }
            Line::PreflightPassed {
                sweep,
                points,
                meshes,
            } => vec![
                ("sweep", s(sweep)),
                ("points", s(points)),
                ("meshes", s(meshes)),
            ],
            Line::PreflightFailed {
                sweep,
                failed,
                points,
            } => vec![
                ("sweep", s(sweep)),
                ("failed", s(failed)),
                ("points", s(points)),
            ],
            Line::DiskTotal { bytes, scenarios } => {
                vec![("bytes", s(bytes)), ("scenarios", s(scenarios))]
            }
            Line::PruneDryRun { blobs, bytes } | Line::Pruned { blobs, bytes } => {
                vec![("blobs", s(blobs)), ("bytes", s(bytes))]
            }
            Line::StagePruned { stage, run } => vec![("stage", s(stage)), ("run", s(run))],
            Line::QueueAdded { jobs, queued } => vec![("jobs", s(jobs)), ("queued", s(queued))],
            Line::QueueNothingToAdd | Line::QueueDrainStarted | Line::QueueDrainRunning => {
                Vec::new()
            }
            Line::QueueRefused { reason }
            | Line::QueueDamaged { reason }
            | Line::ContinueRefused { reason } => vec![("reason", s(reason))],
            Line::QueueUnknownJob { job }
            | Line::QueueCancelled { job }
            | Line::QueueStopRequested { job }
            | Line::JobQueued { job }
            | Line::JobStarting { job }
            | Line::JobCancelled { job }
            | Line::JobUnreachable { job }
            | Line::JobReattached { job } => vec![("job", s(job))],
            Line::QueueNotAllowed { job, state, action } => {
                vec![("job", s(job)), ("state", s(state)), ("action", s(action))]
            }
            Line::QueueReordered { job, position } => {
                vec![("job", s(job)), ("position", s(position))]
            }
            Line::JobRunning { job, line } => vec![("job", s(job)), ("line", s(line))],
            Line::JobStopping { job, cause } => vec![("job", s(job)), ("cause", s(cause))],
            Line::JobFinished { job, run } => vec![("job", s(job)), ("run", s(run))],
            Line::JobStopped { job, run, time } => {
                vec![("job", s(job)), ("run", s(run)), ("time", s(time))]
            }
            Line::JobFailed { job, reason } | Line::JobInterrupted { job, reason } => {
                vec![("job", s(job)), ("reason", s(reason))]
            }
            Line::ContinueFrom { run, time } | Line::StoppedRunNote { run, time } => {
                vec![("run", s(run)), ("time", s(time))]
            }
            Line::ParallelRun { np, cells } => vec![("np", s(np)), ("cells", s(cells))],
            Line::RunDiverged {
                code,
                field,
                value,
                limit,
            } => vec![
                ("code", s(code)),
                ("field", s(field)),
                ("value", s(value)),
                ("limit", s(limit)),
            ],
            Line::SolverMissingFile { code }
            | Line::SolverNumericalBlowup { code }
            | Line::SolverUnknownType { code }
            | Line::SolverMissingKeyword { code }
            | Line::SolverLinearNotConverged { code }
            | Line::SolverPoorMesh { code }
            | Line::SolverOutOfMemory { code }
            | Line::SolverDiskFull { code }
            | Line::SolverDecomposition { code }
            | Line::SolverNotEnoughSlots { code }
            | Line::SolverSegfault { code } => vec![("code", s(code))],
            Line::SolverUnknown { tail } => vec![("tail", s(tail))],
            Line::DiskBelowFloor { free, floor } | Line::DiskStopped { free, floor } => {
                vec![("free", s(free)), ("floor", s(floor))]
            }
            Line::MonitorLine {
                name,
                last,
                mean,
                spread,
                state,
            } => vec![
                ("name", s(name)),
                ("last", s(last)),
                ("mean", s(mean)),
                ("spread", s(spread)),
                ("state", s(state)),
            ],
            Line::MonitorSettled
            | Line::MonitorDrifting
            | Line::MonitorOscillating
            | Line::MonitorTooEarly
            | Line::MonitorTooFewSamples
            | Line::VerdictConverged => Vec::new(),
            Line::MassBalanceLine { state, imbalance }
            | Line::EnergyBalanceLine { state, imbalance } => {
                vec![("state", s(state)), ("imbalance", s(imbalance))]
            }
            Line::VerdictNotConverged { reasons } => vec![("reasons", s(reasons))],
            Line::VerdictNotJudged { reason } => vec![("reason", s(reason))],
            Line::ReasonMonitor { id, from, to } => {
                vec![("id", s(id)), ("from", s(from)), ("to", s(to))]
            }
            Line::GciSummary {
                quantity,
                order,
                gci_percent,
                class,
            } => vec![
                ("quantity", s(quantity)),
                ("order", s(order)),
                ("gci_percent", s(gci_percent)),
                ("class", s(class)),
            ],
            Line::GciOrderNone { quantity, class } => {
                vec![("quantity", s(quantity)), ("class", s(class))]
            }
            Line::GciWarnOscillatory { quantity } => vec![("quantity", s(quantity))],
            Line::GciWarnOrder { quantity, order } => {
                vec![("quantity", s(quantity)), ("order", s(order))]
            }
            Line::GciWarnAsymptotic { quantity, ratio } => {
                vec![("quantity", s(quantity)), ("ratio", s(ratio))]
            }
            Line::GciWarnTransient { quantity } => vec![("quantity", s(quantity))],
            Line::GciWarnWallFunction { quantity } => vec![("quantity", s(quantity))],
            Line::GciWarnNoise { quantity } => vec![("quantity", s(quantity))],
            Line::GciWarnNotRefined { quantity } => vec![("quantity", s(quantity))],
            Line::GciCreated { sweep, points } => vec![("sweep", s(sweep)), ("points", s(points))],
            Line::GciWaiting { sweep, missing } => {
                vec![("sweep", s(sweep)), ("missing", s(missing))]
            }
            Line::BenchmarkPassed { benchmark, metrics } => {
                vec![("benchmark", s(benchmark)), ("metrics", s(metrics))]
            }
            Line::BenchmarkFailed { benchmark, failed } => {
                vec![("benchmark", s(benchmark)), ("failed", s(failed))]
            }
            Line::BenchmarkNotMatching {
                benchmark,
                differences,
            } => vec![("benchmark", s(benchmark)), ("differences", s(differences))],
            Line::BenchmarkListed {
                id,
                title,
                reference,
            } => vec![
                ("id", s(id)),
                ("title", s(title)),
                ("reference", s(reference)),
            ],
            Line::BenchmarkCreated {
                benchmark,
                mesh,
                path,
            } => vec![
                ("benchmark", s(benchmark)),
                ("mesh", s(mesh)),
                ("path", s(path)),
            ],
            Line::BenchmarkResultsIncomplete { benchmark, file } => {
                vec![("benchmark", s(benchmark)), ("file", s(file))]
            }
            Line::BenchmarkOverrideUsed { benchmark } => vec![("benchmark", s(benchmark))],
            Line::BenchmarkNotConverged { benchmark, reasons }
            | Line::BenchmarkConvergenceUnknown { benchmark, reasons } => {
                vec![("benchmark", s(benchmark)), ("reasons", s(reasons))]
            }
            Line::ReasonMeshLevel { from, to } => vec![("from", s(from)), ("to", s(to))],
            Line::ReasonGrading { axis, from, to } => {
                vec![("axis", s(axis)), ("from", s(from)), ("to", s(to))]
            }
            Line::ReasonShape { from, to } => vec![("from", s(from)), ("to", s(to))],
            Line::ReasonInletProfile { boundary, from, to } => {
                vec![("boundary", s(boundary)), ("from", s(from)), ("to", s(to))]
            }
            Line::ReasonPhysics { what, from, to } => {
                vec![("what", s(what)), ("from", s(from)), ("to", s(to))]
            }
            Line::ReasonThermal { boundary, from, to } => {
                vec![("boundary", s(boundary)), ("from", s(from)), ("to", s(to))]
            }
        }
    }

    /// One sample of every variant, for tests.
    #[doc(hidden)]
    pub fn samples() -> Vec<Line> {
        let t = |v: &str| v.to_string();
        vec![
            Line::Migrated {
                from: 0,
                to: 1,
                summary: t("Moved the lid."),
            },
            Line::Saved {
                path: t("a.cfdcraft"),
            },
            Line::Opened {
                path: t("a.cfdcraft"),
            },
            Line::NewerSchema {
                found: 9,
                supported: 1,
            },
            Line::NotAStudyFile {
                path: t("a.zip"),
                reason: t("no manifest"),
            },
            Line::LinkMissing {
                name: t("part.step"),
                expected: t("geometry/part.step"),
            },
            Line::LinkChanged {
                name: t("part.step"),
            },
            Line::LinkRefused {
                name: t("part.step"),
                reason: t("it leaves the project folder"),
            },
            Line::LinkAdded {
                name: t("part.step"),
                path: t("geometry/part.step"),
            },
            Line::ResultMissing { hash: t("ab12") },
            Line::ResultCorrupt { hash: t("ab12") },
            Line::Packed {
                count: 2,
                path: t("a.pack"),
            },
            Line::Unpacked {
                count: 2,
                path: t("a-dir"),
            },
            Line::LinksNotPacked { count: 2 },
            Line::TargetExists {
                path: t("a.cfdcraft"),
            },
            Line::PackMissing { hash: t("ab12") },
            Line::PackDamaged { hash: t("ab12") },
            Line::PackIncomplete { hash: t("ab12") },
            Line::NotWritten {
                path: t("a.cfdcraft"),
                reason: t("the disk is full"),
            },
            Line::LinkNotAdded {
                name: t("part.step"),
                reason: t("a link named part already exists"),
            },
            Line::StaleBecause {
                stage: t("result"),
                reason: t("lid velocity 0.1 → 0.2 m/s"),
            },
            Line::ReasonLength {
                axis: t("x"),
                from: t("1"),
                to: t("2"),
            },
            Line::ReasonDomainOpened,
            Line::ReasonDomainClosed,
            Line::ReasonCells {
                axis: t("y"),
                from: t("64"),
                to: t("128"),
            },
            Line::ReasonLayout,
            Line::ReasonViscosity {
                from: t("0.01"),
                to: t("0.02"),
            },
            Line::ReasonBoundaryVelocity {
                boundary: t("lid"),
                from: t("1"),
                to: t("2"),
            },
            Line::ReasonBoundaryKind {
                boundary: t("lid"),
                from: t("wall"),
                to: t("inlet"),
            },
            Line::ReasonBoundaryFaces {
                boundary: t("lid"),
                from: t("top"),
                to: t("top, left"),
            },
            Line::ReasonBoundaryAdded { boundary: t("lid") },
            Line::ReasonBoundaryRemoved { boundary: t("lid") },
            Line::ReasonEndTime {
                from: t("10"),
                to: t("20"),
            },
            Line::ReasonWriteInterval {
                from: t("1"),
                to: t("2"),
            },
            Line::ReasonPressureLimit {
                from: t("0.001"),
                to: t("0.0001"),
            },
            Line::ReasonVelocityLimit {
                from: t("0.001"),
                to: t("0.0001"),
            },
            Line::ReasonSamples {
                from: t("17"),
                to: t("33"),
            },
            Line::ReasonImage {
                from: t("sha256:aa"),
                to: t("sha256:bb"),
            },
            Line::ReasonDialect {
                from: t("esi-v2512"),
                to: t("esi-v2606"),
            },
            Line::ReasonSidecar {
                from: t("none"),
                to: t("lock-1"),
            },
            Line::ReasonWriter {
                from: t("0.1.0"),
                to: t("0.2.0"),
            },
            Line::ReasonMesh,
            Line::StageUpToDate {
                stage: t("mesh"),
                run: t("run-1"),
            },
            Line::StageNeverMade { stage: t("result") },
            Line::StageBlobMissing {
                stage: t("mesh"),
                hash: t("ab12"),
            },
            Line::RunRecorded {
                run: t("run-2"),
                stages: t("mesh, result"),
            },
            Line::NothingToRun,
            Line::RerunReproduced {
                run: t("run-3"),
                of: t("run-1"),
            },
            Line::RerunWithin {
                run: t("run-3"),
                of: t("run-1"),
                max_diff: t("1e-10"),
            },
            Line::RerunDiffers {
                run: t("run-3"),
                of: t("run-1"),
                count: 2,
            },
            Line::ToolMismatch {
                reasons: t("solver image sha256:aa → sha256:bb"),
            },
            Line::UnknownRun { run: t("run-9") },
            Line::NoRuns,
            Line::RunHasNoResult { run: t("run-1") },
            Line::RecordsInconsistent {
                run: t("run-2"),
                reason: t("its result key does not match its recipe"),
            },
            Line::NotPending {
                item: t("cavity:mesh"),
            },
            Line::UnknownItem { item: t("cavity") },
            Line::NeedsUpstream {
                item: t("cavity:result"),
                needs: t("cavity:mesh"),
            },
            Line::OrderMismatch,
            Line::OrderBreaksDependency {
                item: t("cavity:result"),
                needs: t("cavity:mesh"),
            },
            Line::ItemUpdated {
                item: t("cavity:mesh"),
                run: t("run-4"),
            },
            Line::NothingToUpdate,
            Line::MigratedAndSaved {
                from: 1,
                to: 2,
                summary: t("Added runs."),
            },
            Line::OutputNotRecorded {
                reason: t("a result file is a link"),
            },
            Line::DuplicateName { name: t("cavity") },
            Line::ScenarioCreated {
                scenario: t("fast-lid"),
                parent: t("base"),
            },
            Line::ScenarioUpdated {
                scenario: t("fast-lid"),
            },
            Line::ScenarioLabelled {
                scenario: t("fast-lid"),
            },
            Line::ScenarioUnknown {
                scenario: t("fast-lid"),
            },
            Line::ScenarioRefused {
                reason: t("a parent must come first"),
            },
            Line::OverrideNotApplied {
                scenario: t("fast-lid"),
                setting: t("/mesh/cells_x"),
                reason: t("there is no such setting"),
            },
            Line::ScenarioDiffers {
                from: t("base"),
                to: t("fast-lid"),
                reasons: t("lid velocity 0.1 → 0.2 m/s; cells in x 128 → 256"),
            },
            Line::ScenarioSame {
                from: t("base"),
                to: t("copy"),
            },
            Line::ReasonRawOverride {
                id: t("relax-u"),
                from: t("esi system/fvSolution relaxationFactors/equations/U = 0.7"),
                to: t("esi system/fvSolution relaxationFactors/equations/U = 0.5"),
            },
            Line::ReasonRawMesh,
            Line::SweepTooLarge {
                sweep: t("grid"),
                points: t("1000"),
                max: t("200"),
            },
            Line::SweepRefused {
                sweep: t("wind"),
                reason: t("a parameter has no values"),
            },
            Line::SweepCreated {
                sweep: t("wind"),
                points: t("9"),
            },
            Line::SweepUnknown { sweep: t("wind") },
            Line::PreflightPointFailed {
                point: t("wind-004"),
                reason: t("cells in x is zero"),
            },
            Line::PreflightPassed {
                sweep: t("wind"),
                points: t("9"),
                meshes: t("3"),
            },
            Line::PreflightFailed {
                sweep: t("wind"),
                failed: t("2"),
                points: t("9"),
            },
            Line::DiskTotal {
                bytes: t("4096"),
                scenarios: t("3"),
            },
            Line::PruneNothing,
            Line::PruneDryRun {
                blobs: t("3"),
                bytes: t("1024"),
            },
            Line::Pruned {
                blobs: t("3"),
                bytes: t("1024"),
            },
            Line::StagePruned {
                stage: t("result"),
                run: t("run-3"),
            },
            Line::PruneRefused {
                reason: t("a variant is unknown"),
            },
            Line::PruneNotRemoved {
                reason: t("permission denied"),
            },
            Line::QueueAdded {
                jobs: t("3"),
                queued: t("3"),
            },
            Line::QueueNothingToAdd,
            Line::QueueRefused {
                reason: t("the queue is full"),
            },
            Line::QueueUnknownJob { job: t("q-009") },
            Line::QueueNotAllowed {
                job: t("q-003"),
                state: t("running"),
                action: t("reordered"),
            },
            Line::QueueReordered {
                job: t("q-004"),
                position: t("1"),
            },
            Line::QueueCancelled { job: t("q-005") },
            Line::QueueStopRequested { job: t("q-003") },
            Line::QueueDrainStarted,
            Line::QueueDrainRunning,
            Line::QueueDamaged {
                reason: t("a line is not valid"),
            },
            Line::JobQueued { job: t("q-003") },
            Line::JobStarting { job: t("q-003") },
            Line::JobRunning {
                job: t("q-003"),
                line: t("Time = 120"),
            },
            Line::JobStopping {
                job: t("q-003"),
                cause: t("stop requested"),
            },
            Line::JobFinished {
                job: t("q-003"),
                run: t("run-4"),
            },
            Line::JobStopped {
                job: t("q-003"),
                run: t("run-4"),
                time: t("1200"),
            },
            Line::JobFailed {
                job: t("q-003"),
                reason: t("the solver exited with an error"),
            },
            Line::JobCancelled { job: t("q-005") },
            Line::JobInterrupted {
                job: t("q-003"),
                reason: t("the queue runner was stopped"),
            },
            Line::JobUnreachable { job: t("q-003") },
            Line::JobReattached { job: t("q-003") },
            Line::ContinueFrom {
                run: t("run-4"),
                time: t("1200"),
            },
            Line::ContinueRefused {
                reason: t("it is not a stopped run"),
            },
            Line::ParallelRun {
                np: t("4"),
                cells: t("40000"),
            },
            Line::StoppedRunNote {
                run: t("run-4"),
                time: t("1200"),
            },
            Line::RunDiverged {
                code: t("CFD-RUN-004"),
                field: t("Ux"),
                value: t("5200"),
                limit: t("1e4"),
            },
            Line::SolverMissingFile {
                code: t("CFD-SOLVER-001"),
            },
            Line::SolverNumericalBlowup {
                code: t("CFD-SOLVER-002"),
            },
            Line::SolverUnknownType {
                code: t("CFD-SOLVER-003"),
            },
            Line::SolverMissingKeyword {
                code: t("CFD-SOLVER-004"),
            },
            Line::SolverLinearNotConverged {
                code: t("CFD-SOLVER-005"),
            },
            Line::SolverPoorMesh {
                code: t("CFD-SOLVER-006"),
            },
            Line::SolverOutOfMemory {
                code: t("CFD-SOLVER-007"),
            },
            Line::SolverDiskFull {
                code: t("CFD-SOLVER-008"),
            },
            Line::SolverDecomposition {
                code: t("CFD-SOLVER-009"),
            },
            Line::SolverNotEnoughSlots {
                code: t("CFD-SOLVER-010"),
            },
            Line::SolverSegfault {
                code: t("CFD-SOLVER-011"),
            },
            Line::SolverUnknown {
                tail: t("cannot open file system/fvSolution | FOAM exiting"),
            },
            Line::DiskBelowFloor {
                free: t("8"),
                floor: t("20"),
            },
            Line::DiskStopped {
                free: t("19"),
                floor: t("20"),
            },
            Line::MonitorLine {
                name: t("probe_1.Ux"),
                last: t("0.0123"),
                mean: t("0.0121"),
                spread: t("0.0004"),
                state: t("settled"),
            },
            Line::MonitorSettled,
            Line::MonitorDrifting,
            Line::MonitorOscillating,
            Line::MonitorTooEarly,
            Line::MonitorTooFewSamples,
            Line::MassBalanceLine {
                state: t("closes"),
                imbalance: t("0.2 %"),
            },
            Line::EnergyBalanceLine {
                state: t("does not close"),
                imbalance: t("7 %"),
            },
            Line::VerdictConverged,
            Line::VerdictNotConverged {
                reasons: t("monitor probe_1 drifting"),
            },
            Line::VerdictNotJudged {
                reason: t("no monitors are defined"),
            },
            Line::ReasonMonitor {
                id: t("probe_1"),
                from: t("none"),
                to: t("x 0.05 m, y 0.05 m, velocity"),
            },
            Line::GciSummary {
                quantity: t("bench.step.reattachment"),
                order: t("1.53"),
                gci_percent: t("2.17"),
                class: t("monotone"),
            },
            Line::GciOrderNone {
                quantity: t("bench.step.reattachment"),
                class: t("divergent"),
            },
            Line::GciWarnOscillatory {
                quantity: t("bench.step.reattachment"),
            },
            Line::GciWarnOrder {
                quantity: t("bench.step.reattachment"),
                order: t("3.1"),
            },
            Line::GciWarnAsymptotic {
                quantity: t("bench.step.reattachment"),
                ratio: t("1.3"),
            },
            Line::GciWarnTransient {
                quantity: t("bench.cylinder.st"),
            },
            Line::GciWarnWallFunction {
                quantity: t("bench.step.reattachment"),
            },
            Line::GciWarnNoise {
                quantity: t("bench.cylinder.cd_max"),
            },
            Line::GciWarnNotRefined {
                quantity: t("bench.step.reattachment"),
            },
            Line::GciCreated {
                sweep: t("levels"),
                points: t("3"),
            },
            Line::GciWaiting {
                sweep: t("levels"),
                missing: t("level-1, level-2"),
            },
            Line::BenchmarkPassed {
                benchmark: t("step"),
                metrics: t("2"),
            },
            Line::BenchmarkFailed {
                benchmark: t("step"),
                failed: t("reattachment"),
            },
            Line::BenchmarkNotMatching {
                benchmark: t("step"),
                differences: t("the step height differs"),
            },
            Line::BenchmarkListed {
                id: t("step"),
                title: t("Backward-facing step"),
                reference: t("Driver and Seegmiller 1985"),
            },
            Line::BenchmarkCreated {
                benchmark: t("step"),
                mesh: t("medium"),
                path: t("step.json"),
            },
            Line::BenchmarkResultsIncomplete {
                benchmark: t("step"),
                file: t("log.simpleFoam"),
            },
            Line::BenchmarkOverrideUsed {
                benchmark: t("step"),
            },
            Line::BenchmarkNotConverged {
                benchmark: t("step"),
                reasons: t("not_settled:floor"),
            },
            Line::BenchmarkConvergenceUnknown {
                benchmark: t("step"),
                reasons: t("unknown: the run recorded no assessment"),
            },
            Line::ReasonMeshLevel {
                from: t("0"),
                to: t("1"),
            },
            Line::ReasonGrading {
                axis: t("x"),
                from: t("1"),
                to: t("1.2"),
            },
            Line::ReasonShape {
                from: t("box"),
                to: t("backward step"),
            },
            Line::ReasonInletProfile {
                boundary: t("feed"),
                from: t("uniform"),
                to: t("parabolic"),
            },
            Line::ReasonPhysics {
                what: t("turbulence"),
                from: t("laminar"),
                to: t("k-omega SST"),
            },
            Line::ReasonThermal {
                boundary: t("hot"),
                from: t("none"),
                to: t("fixed 307.8 K"),
            },
        ]
    }
}
