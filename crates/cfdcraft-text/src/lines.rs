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
        ]
    }
}
