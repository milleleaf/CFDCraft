//! Which settings of a study need a new mesh when a variant changes them.
//!
//! A setting is geometry when changing it can change the mesh and setting-only otherwise. The
//! table here is the declared class, what a person sees in a list of variants; the mesh key is
//! the fact. A test runs every row against the mesh key so the two cannot drift apart.

use cfdcraft_model::RawOverride;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::overrides::{Override, parse_setting};

/// Whether changing a setting needs a new mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingClass {
    /// The change can alter the mesh, so the variant gets its own.
    Geometry,
    /// The change leaves the mesh as it is, so the variant reuses its parent's.
    Setting,
}

use SettingClass::{Geometry, Setting};

/// Every setting a variant can change, with its class. `<label>` stands for the label of one
/// boundary and `<id>` for the label of one raw override; each matches exactly one part.
pub const KNOWN_SETTINGS: &[(&str, SettingClass)] = &[
    ("/domain/length_x", Geometry),
    ("/domain/length_x/value", Geometry),
    ("/domain/length_x/unit", Geometry),
    ("/domain/length_y", Geometry),
    ("/domain/length_y/value", Geometry),
    ("/domain/length_y/unit", Geometry),
    ("/domain/length_z", Geometry),
    ("/domain/length_z/value", Geometry),
    ("/domain/length_z/unit", Geometry),
    ("/domain/open", Geometry),
    ("/mesh/cells_x", Geometry),
    ("/mesh/cells_y", Geometry),
    ("/mesh/cells_z", Geometry),
    ("/boundary_conditions/<label>/kind", Geometry),
    ("/boundary_conditions/<label>/faces", Geometry),
    ("/boundary_conditions/<label>/velocity", Setting),
    ("/boundary_conditions/<label>/velocity/value", Setting),
    ("/fluid/kinematic_viscosity", Setting),
    ("/fluid/kinematic_viscosity/value", Setting),
    ("/solver/solver", Setting),
    ("/solver/end_time", Setting),
    ("/solver/end_time/value", Setting),
    ("/solver/write_interval", Setting),
    ("/solver/write_interval/value", Setting),
    ("/solver/convergence", Setting),
    ("/solver/convergence/p", Setting),
    ("/solver/convergence/U", Setting),
    ("/monitors/centreline_u_sample_count", Setting),
];

const RAW_OVERRIDES: &str = "raw_overrides";
const BOUNDARY_CONDITIONS: &str = "boundary_conditions";

fn is_placeholder(part: &str) -> bool {
    part == "<label>" || part == "<id>"
}

fn row_matches(row: &str, parts: &[String]) -> bool {
    let want: Vec<&str> = row.split('/').skip(1).collect();
    want.len() == parts.len()
        && want
            .iter()
            .zip(parts)
            .all(|(w, h)| is_placeholder(w) || *w == h.as_str())
}

/// The class of a setting the table knows, or `None` for any other.
///
/// A whole boundary entry (`/boundary_conditions/<label>`) is geometry on purpose: it may change
/// the patch layout. Raw overrides have no class here because it depends on the value or on the
/// study; [`classify`] decides them. A malformed pointer has no class.
pub fn classify_setting(setting: &str) -> Option<SettingClass> {
    let parts = parse_setting(setting).ok()?;
    if parts.len() == 2 && parts[0] == BOUNDARY_CONDITIONS {
        return Some(Geometry);
    }
    KNOWN_SETTINGS
        .iter()
        .find(|(row, _)| row_matches(row, &parts))
        .map(|(_, class)| *class)
}

fn class_of(mesh: bool) -> SettingClass {
    if mesh { Geometry } else { Setting }
}

fn entry_is_mesh(entry: &Value) -> bool {
    serde_json::from_value::<RawOverride>(entry.clone()).is_ok_and(|raw| raw.affects_mesh())
}

fn file_is_mesh(file: &str) -> bool {
    RawOverride {
        backend: cfdcraft_model::RawBackend::Esi,
        file: file.to_string(),
        entry: String::new(),
        value: String::new(),
    }
    .affects_mesh()
}

/// The class of one override of a raw override entry, judged against the study it applies to.
///
/// A change is geometry when the entry the study holds, or the entry the change writes, is
/// mesh-class: dropping or rewriting a mesh dictionary entry moves the mesh as much as adding one.
fn raw_class(
    parent: &Value,
    id: &str,
    member: Option<&str>,
    value: Option<&Value>,
) -> SettingClass {
    let held = parent
        .get(RAW_OVERRIDES)
        .and_then(|map| map.get(id))
        .is_some_and(entry_is_mesh);
    let written = match (member, value) {
        // The whole entry is written: it must read as a raw override, else the safe reading holds.
        (None, Some(value)) => match serde_json::from_value::<RawOverride>(value.clone()) {
            Ok(raw) => raw.affects_mesh(),
            Err(_) => true,
        },
        (Some("file"), Some(value)) => value.as_str().is_some_and(file_is_mesh),
        _ => false,
    };
    class_of(held || written)
}

fn override_class(parent: &Value, item: &Override) -> SettingClass {
    let (setting, value) = match item {
        Override::Set { setting, value } => (setting, Some(value)),
        Override::Remove { setting } => (setting, None),
    };
    if let Some(class) = classify_setting(setting) {
        return class;
    }
    let Ok(parts) = parse_setting(setting) else {
        return Geometry;
    };
    let [root, rest @ ..] = parts.as_slice() else {
        return Geometry;
    };
    if root.as_str() != RAW_OVERRIDES {
        return Geometry;
    }
    match rest {
        [id] => raw_class(parent, id, None, value),
        [id, member] => raw_class(parent, id, Some(member.as_str()), value),
        _ => Geometry,
    }
}

/// The class of a variant: geometry when any of its overrides is, setting-only otherwise.
///
/// `parent` is the study the overrides apply to, written as JSON; it is read only to judge raw
/// overrides. A setting the table does not know is read as geometry, the safe reading. The class
/// is a label: whether a mesh can be reused is always decided by comparing mesh keys.
pub fn classify(parent: &Value, overrides: &[Override]) -> SettingClass {
    let geometry = overrides
        .iter()
        .any(|item| override_class(parent, item) == Geometry);
    class_of(geometry)
}
