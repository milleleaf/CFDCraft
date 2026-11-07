//! The verbs for variants: make or change one, list them, show one, say how two differ, and
//! name, pin or annotate one. Each calls the matching operation and prints its outcome.
//!
//! `list` and `show` answer with data: their text output is the JSON data of the report on
//! standard output. `set`, `diff` and `label` print the sentence of their outcome.

use std::path::{Path, PathBuf};

use cfdcraft_ops::scenario_ops::{
    LabelUpdate, scenario_diff, scenario_label, scenario_list, scenario_set, scenario_show,
};
use cfdcraft_scenarios::Override;

use super::{Error, emit_doc, print_data};

/// The longest piece of an argument that an error message quotes.
const SHOWN: usize = 60;

fn quoted(arg: &str) -> String {
    let shown: String = arg.chars().take(SHOWN).collect();
    if shown.len() < arg.len() {
        format!("{shown}...")
    } else {
        shown
    }
}

/// Reads a `--set` argument, written `SETTING=JSON`: the setting is everything before the first
/// `=` and the value is the JSON text after it (`0.2`, `"mm"`, `{"value":1,"unit":"m"}`).
pub fn parse_set(arg: &str) -> Result<Override, Error> {
    let Some((setting, text)) = arg.split_once('=') else {
        return Err(format!(
            "--set {} is not written SETTING=JSON: there is no \"=\"",
            quoted(arg)
        )
        .into());
    };
    if setting.is_empty() {
        return Err(format!("--set {} names no setting before the \"=\"", quoted(arg)).into());
    }
    let value = serde_json::from_str(text).map_err(|e| {
        format!(
            "--set {} does not hold a JSON value after the \"=\": {e}",
            quoted(arg)
        )
    })?;
    Ok(Override::Set {
        setting: setting.to_owned(),
        value,
    })
}

/// What the `scenario set` verb was asked.
pub struct SetArgs {
    pub doc: PathBuf,
    pub id: String,
    pub parent: Option<String>,
    /// The `--set SETTING=JSON` arguments, as typed.
    pub sets: Vec<String>,
    /// The settings to remove from the variant.
    pub unsets: Vec<String>,
    pub name: Option<String>,
    pub json: bool,
}

/// Make the variant, or change the changes of the one that exists.
pub fn set(args: &SetArgs) -> Result<i32, Error> {
    let mut changes = Vec::with_capacity(args.sets.len() + args.unsets.len());
    for arg in &args.sets {
        changes.push(parse_set(arg)?);
    }
    for setting in &args.unsets {
        if setting.is_empty() {
            return Err("--unset needs the name of a setting".into());
        }
        changes.push(Override::Remove {
            setting: setting.clone(),
        });
    }
    let outcome = scenario_set(
        &args.doc,
        &args.id,
        args.parent.as_deref(),
        &changes,
        args.name.as_deref(),
    );
    Ok(emit_doc(&outcome, args.json))
}

/// The variants of the study file, the study itself first.
pub fn list(doc: &Path, json: bool) -> Result<i32, Error> {
    print_data(&scenario_list(doc), json)
}

/// One variant: its changes, raw changes and keys.
pub fn show(doc: &Path, id: &str, json: bool) -> Result<i32, Error> {
    print_data(&scenario_show(doc, id), json)
}

/// How variant `b` differs from variant `a`, in the sentence a person reads.
pub fn diff(doc: &Path, a: &str, b: &str, json: bool) -> Result<i32, Error> {
    Ok(emit_doc(&scenario_diff(doc, a, b), json))
}

/// What the `scenario label` verb was asked.
pub struct LabelArgs {
    pub doc: PathBuf,
    pub id: String,
    pub name: Option<String>,
    pub pin: bool,
    pub unpin: bool,
    pub notes: Option<String>,
    pub json: bool,
}

/// Name, pin or annotate a variant. A name or notes given as an empty text are cleared.
pub fn label(args: &LabelArgs) -> Result<i32, Error> {
    let pinned = match (args.pin, args.unpin) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    };
    let update = LabelUpdate {
        name: args.name.clone(),
        pinned,
        notes: args.notes.clone(),
    };
    Ok(emit_doc(
        &scenario_label(&args.doc, &args.id, &update),
        args.json,
    ))
}
