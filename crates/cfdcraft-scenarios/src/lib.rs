//! Scenarios: variants of one study.
//!
//! A scenario stores only what it changes from the study it descends from. Each change is an
//! override: a typed value put at a setting, or a member taken away, where a setting is a JSON
//! Pointer into the study written as JSON. The effective study of a variant is the document's base
//! study with the overrides of its ancestors applied first and its own after; the result must read
//! back as a valid study, so the study format stays the only definition of a valid setting and a
//! variant never copies its parent.
//!
//! Each setting is also classed as geometry or setting-only, so a person can see which variants
//! need a new mesh; the class is a label and the mesh key stays the fact.
//!
//! The crate is pure: it has no file, store, clock or process access.

mod chain;
mod class;
mod overrides;

pub use chain::{
    BASE, MAX_DEPTH, MAX_NAME, MAX_NOTES, MAX_OVERRIDES, MAX_SCENARIOS, Origin, Scenario,
    ScenarioError, chain, check_scenarios, effective_study,
};
pub use class::{KNOWN_SETTINGS, SettingClass, classify, classify_setting};
pub use overrides::{ApplyError, Override, apply_overrides};
