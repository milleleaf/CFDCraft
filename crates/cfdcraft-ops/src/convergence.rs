//! The convergence verdict of a finished run, made from the files it left.
//!
//! The specification of what a run monitors comes from the resolved study, the tolerances from the
//! study's settle settings, and the files from a results folder or from a stored result. The
//! verdict itself is made by the assess crate, the same function the live view uses, so the word
//! recorded with a result is the word a person watching the run saw. Nothing here blocks a
//! record: a run that is not converged is recorded with a verdict that says so.
//!
//! The results folder came from a compute host, so every read uses the file type of the entry
//! itself and never follows a link; a link or a file over the size cap is left out, and a quantity
//! with no file can only be too early to judge.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use cfdcraft_assess::{
    Assessment, Files, LineSpec, PatchSpec, ProbeSpec, Spec, SurfaceQuantityKind, SurfaceSpec,
    assess,
};
use cfdcraft_backend_esi::monitors::balance_patches;
use cfdcraft_capture::{entry_names, read_entry};
use cfdcraft_model::{MonitorField, ResolvedStudy, Study, SurfaceQuantity};
use cfdcraft_monitors::FieldKind;
use cfdcraft_settle::Settings;

/// The log the solver writes, relative to the results folder.
const SOLVER_LOG: &str = "log.simpleFoam";
/// The most bytes one monitor file may hold.
const MONITOR_FILE_CAP: u64 = 8 * 1024 * 1024;
/// The most bytes the solver log may hold.
const LOG_CAP: u64 = 64 * 1024 * 1024;
/// The most files read for one function object.
const FILES_PER_OBJECT: usize = 200;
/// The files a monitor function object writes into its time folders.
const MONITOR_FILES: [&str; 5] = ["U", "p", "line_U.xy", "line_p.xy", "surfaceFieldValue.dat"];

fn field_kind(field: MonitorField) -> FieldKind {
    match field {
        MonitorField::Velocity => FieldKind::Velocity,
        MonitorField::Pressure => FieldKind::Pressure,
    }
}

fn surface_kind(quantity: SurfaceQuantity) -> SurfaceQuantityKind {
    match quantity {
        SurfaceQuantity::Flux => SurfaceQuantityKind::Flux,
        SurfaceQuantity::PressureAverage => SurfaceQuantityKind::PressureAverage,
        SurfaceQuantity::PressureIntegral => SurfaceQuantityKind::PressureIntegral,
    }
}

/// What a run of this study monitors: the probes, lines and surfaces of its monitor plan (the
/// default probes when the study lists none), one flux per patch for the mass balance, and the
/// residual limits the study asks for. A monitor with no label of its own is shown by its
/// function-object name.
pub fn spec_for(resolved: &ResolvedStudy) -> Spec {
    let plan = resolved.monitor_plan();
    let shown = |label: Option<String>, fo: &str| label.unwrap_or_else(|| fo.to_owned());
    Spec {
        probes: plan
            .probes
            .into_iter()
            .map(|p| ProbeSpec {
                label: shown(p.label, &p.fo),
                fo: p.fo,
                field: field_kind(p.field),
            })
            .collect(),
        lines: plan
            .lines
            .into_iter()
            .map(|l| LineSpec {
                label: shown(l.label, &l.fo),
                fo: l.fo,
                field: field_kind(l.field),
            })
            .collect(),
        surfaces: plan
            .surfaces
            .into_iter()
            .map(|s| SurfaceSpec {
                label: shown(s.label, &s.fo),
                fo: s.fo,
                quantity: surface_kind(s.quantity),
            })
            .collect(),
        patches: balance_patches(resolved)
            .into_iter()
            .map(|b| PatchSpec {
                fo: b.fo,
                patch: b.patch.to_owned(),
                inlet: b.inlet,
            })
            .collect(),
        solves_energy: false,
        residual_limits: Some((resolved.solver.convergence.p, resolved.solver.convergence.u)),
        transient: false,
    }
}

/// The tolerances a run of this study is judged by: the defaults, with each setting the study
/// gives applied. A combination outside the allowed ranges is not used; the defaults stand.
pub fn settings_for(study: &Study) -> Settings {
    let mut settings = Settings::default();
    if let Some(given) = &study.monitors.settle {
        if let Some(v) = given.tail_fraction {
            settings.tail_fraction = v;
        }
        if let Some(v) = given.tail_floor {
            settings.tail_floor = v;
        }
        if let Some(v) = given.spread_rel {
            settings.spread_rel = v;
        }
        if let Some(v) = given.drift_rel {
            settings.drift_rel = v;
        }
    }
    if settings.check().is_err() {
        return Settings::default();
    }
    settings
}

/// The text of the regular file at `path`, or none when it is a link, a folder, over `cap` bytes
/// or cannot be read.
fn read_plain(path: &Path, cap: u64) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() > cap {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Whether `path` is a folder itself, not a link to one.
fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}

/// The names of the function objects the spec reads.
fn objects(spec: &Spec) -> Vec<&str> {
    spec.probes
        .iter()
        .map(|p| p.fo.as_str())
        .chain(spec.lines.iter().map(|l| l.fo.as_str()))
        .chain(spec.surfaces.iter().map(|s| s.fo.as_str()))
        .chain(spec.patches.iter().map(|p| p.fo.as_str()))
        .collect()
}

/// One monitor file found: its function object, its time folder and its text.
type Found = (String, String, String);
/// The pieces of one function object: the folder as a number, the folder name and the text.
type Pieces = Vec<(f64, String, String)>;

/// The files in folder order (the folder name read as a number) for each function object, at most
/// [`FILES_PER_OBJECT`] of them, the newest. A folder whose name is not a number is left out.
fn gather(log: String, found: Vec<Found>) -> Files {
    let mut by_object: BTreeMap<String, Pieces> = BTreeMap::new();
    for (object, folder, text) in found {
        let Some(time) = folder.parse::<f64>().ok().filter(|t| t.is_finite()) else {
            continue;
        };
        by_object
            .entry(object)
            .or_default()
            .push((time, folder, text));
    }
    let monitor_files: BTreeMap<String, Vec<(String, String)>> = by_object
        .into_iter()
        .map(|(object, mut pieces)| {
            pieces.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            // The newest pieces are the ones kept: the tail of a series is what is judged.
            let extra = pieces.len().saturating_sub(FILES_PER_OBJECT);
            pieces.drain(..extra);
            let pieces: Vec<(String, String)> = pieces
                .into_iter()
                .map(|(_, folder, text)| (folder, text))
                .collect();
            (object, pieces)
        })
        .collect();
    Files { log, monitor_files }
}

/// The log and the monitor files of the results folder `dir`, for the function objects `spec` names.
/// Nothing is followed through a link, and a file over the size cap is left out.
pub fn files_from_dir(dir: &Path, spec: &Spec) -> Files {
    let log = read_plain(&dir.join(SOLVER_LOG), LOG_CAP).unwrap_or_default();
    let mut found = Vec::new();
    let post = dir.join("postProcessing");
    if !is_real_dir(&post) {
        return gather(log, found);
    }
    for object in objects(spec) {
        let base = post.join(object);
        if !is_real_dir(&base) {
            continue;
        }
        let Ok(listing) = fs::read_dir(&base) else {
            continue;
        };
        for entry in listing.flatten() {
            let Ok(folder) = entry.file_name().into_string() else {
                continue;
            };
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            for name in MONITOR_FILES {
                if let Some(text) = read_plain(&entry.path().join(name), MONITOR_FILE_CAP) {
                    found.push((object.to_owned(), folder.clone(), text));
                }
            }
        }
    }
    gather(log, found)
}

/// The log and the monitor files held in a stored result, laid out as in the results folder it
/// was made from.
pub fn files_from_bundle(bytes: &[u8], spec: &Spec) -> Files {
    let text_of = |name: &str| {
        read_entry(bytes, name)
            .ok()
            .map(|data| String::from_utf8_lossy(&data).into_owned())
    };
    let log = text_of(SOLVER_LOG).unwrap_or_default();
    let wanted = objects(spec);
    let mut found = Vec::new();
    for name in entry_names(bytes).unwrap_or_default() {
        let parts: Vec<&str> = name.split('/').collect();
        let [root, object, folder, file] = parts.as_slice() else {
            continue;
        };
        if *root != "postProcessing" || !wanted.contains(object) || !MONITOR_FILES.contains(file) {
            continue;
        }
        if let Some(text) = text_of(&name) {
            found.push(((*object).to_owned(), (*folder).to_owned(), text));
        }
    }
    gather(log, found)
}

/// The numbers and the verdict for the results folder `dir`.
pub fn assess_dir(dir: &Path, spec: &Spec, settings: &Settings) -> Assessment {
    assess(spec, &files_from_dir(dir, spec), settings)
}
