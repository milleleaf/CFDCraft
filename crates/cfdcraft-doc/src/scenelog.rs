//! The variants, sweeps and pruned results inside the artifact index: the rules that tie them to
//! each other, to the run history and to the list of stored results.

use std::collections::{HashMap, HashSet};

use cfdcraft_scenarios::{BASE, check_scenarios};
use cfdcraft_store::ContentHash;
use cfdcraft_sweep::point_count;

use crate::model::ArtifactIndex;

/// The most sweeps one file may hold.
pub const MAX_SWEEPS: usize = 200;

/// A text taken from the file, safe to put in a sentence: kept only when it is short and made of
/// letters, digits, hyphens and underscores.
fn shown(text: &str) -> String {
    let plain = text.len() <= 40
        && !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if plain {
        text.to_owned()
    } else {
        "(not a plain name)".to_owned()
    }
}

/// Whether the variants, sweeps, run tags and pruned list hold together. The reason is a plain
/// sentence made of ids and counts and never names a path.
///
/// The rules, in this order: the variants pass their own check; there are at most [`MAX_SWEEPS`]
/// sweeps, each passes its own check, has an id of its own and starts from `base` or a listed
/// variant; a variant that names the sweep it came from names a listed sweep and a point it has;
/// a run that names a variant names a listed one; the pruned list is sorted, has no repeat and
/// holds only hashes of stored results.
pub fn check_scenario_records(index: &ArtifactIndex) -> Result<(), String> {
    check_scenarios(&index.scenarios).map_err(|e| e.to_string())?;

    let variants: HashSet<&str> = index.scenarios.iter().map(|s| s.id.as_str()).collect();
    if index.sweeps.len() > MAX_SWEEPS {
        return Err(format!(
            "the file holds {} sweeps, but at most {MAX_SWEEPS} are allowed",
            index.sweeps.len()
        ));
    }
    // The number of points of every sweep, by id.
    let mut counts: HashMap<&str, usize> = HashMap::with_capacity(index.sweeps.len());
    for sweep in &index.sweeps {
        let count = point_count(sweep).map_err(|e| format!("sweep {}: {e}", shown(&sweep.id)))?;
        if counts.insert(sweep.id.as_str(), count).is_some() {
            return Err(format!("the sweep {} is listed twice", shown(&sweep.id)));
        }
        if sweep.base != BASE && !variants.contains(sweep.base.as_str()) {
            return Err(format!(
                "sweep {} starts from {}, which is neither the study nor a listed variant",
                shown(&sweep.id),
                shown(&sweep.base)
            ));
        }
    }

    for scenario in &index.scenarios {
        let Some(origin) = &scenario.origin else {
            continue;
        };
        let Some(&count) = counts.get(origin.sweep.as_str()) else {
            return Err(format!(
                "variant {} comes from the sweep {}, which is not listed",
                scenario.id,
                shown(&origin.sweep)
            ));
        };
        if !usize::try_from(origin.index).is_ok_and(|at| at < count) {
            return Err(format!(
                "variant {} comes from point {} of sweep {}, which has {count} points",
                scenario.id,
                origin.index,
                shown(&origin.sweep)
            ));
        }
    }

    for run in &index.runs {
        if let Some(tag) = &run.scenario
            && !variants.contains(tag.as_str())
        {
            return Err(format!(
                "run {} was made for the variant {}, which is not listed",
                run.id,
                shown(tag)
            ));
        }
    }

    let stored: HashSet<ContentHash> = index.blobs.iter().map(|b| b.hash).collect();
    for pair in index.pruned.windows(2) {
        if pair[0] == pair[1] {
            return Err(format!("the pruned list names {} twice", pair[0]));
        }
        if pair[0] > pair[1] {
            return Err("the pruned list is not sorted".to_owned());
        }
    }
    if let Some(hash) = index.pruned.iter().find(|h| !stored.contains(*h)) {
        return Err(format!(
            "the pruned list names {hash}, which the list of stored results does not"
        ));
    }
    Ok(())
}
