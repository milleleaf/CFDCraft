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
    Assessment, Files, ForceSpec, HeatSpec, LineSpec, PatchSpec, ProbeSpec, Spec,
    SurfaceQuantityKind, SurfaceSpec, assess,
};
use cfdcraft_backend_esi::{SolverPlan, monitors::balance_patches};
use cfdcraft_capture::{entry_names, read_entry};
use cfdcraft_model::{
    Face, MonitorField, PlannedSurface, ResolvedStudy, ResolvedThermal, Study, SurfaceQuantity,
};
use cfdcraft_monitors::FieldKind;
use cfdcraft_settle::Settings;

/// The most bytes one monitor file may hold.
const MONITOR_FILE_CAP: u64 = 8 * 1024 * 1024;
/// The most bytes the solver log may hold.
const LOG_CAP: u64 = 64 * 1024 * 1024;
/// The most files read for one function object.
const FILES_PER_OBJECT: usize = 200;
/// The files a monitor function object writes into its time folders.
const MONITOR_FILES: [&str; 6] = [
    "U",
    "p",
    "line_U.xy",
    "line_p.xy",
    "surfaceFieldValue.dat",
    "coefficient.dat",
];

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
        SurfaceQuantity::HeatFlux => SurfaceQuantityKind::HeatFlux,
    }
}

/// The part of a vector column a heat-flux surface reads: the axis its wall faces along (x for a
/// side wall, y for a floor or a ceiling). `None` for a wall that faces neither, or for another
/// quantity.
fn heat_component(resolved: &ResolvedStudy, surface: &PlannedSurface) -> Option<usize> {
    if surface.quantity != SurfaceQuantity::HeatFlux {
        return None;
    }
    let boundary = resolved.boundary_conditions.get(&surface.boundary)?;
    match boundary.faces.first()? {
        Face::XMin | Face::XMax => Some(0),
        Face::YMin | Face::YMax => Some(1),
        Face::Obstacle => None,
    }
}

/// The two heat-flux surfaces whose flows must balance: the study has exactly two, both on walls
/// held at fixed and different temperatures. The hotter wall's object is `hot`.
fn heat_pair(resolved: &ResolvedStudy, surfaces: &[PlannedSurface]) -> Option<HeatSpec> {
    let heat: Vec<&PlannedSurface> = surfaces
        .iter()
        .filter(|s| s.quantity == SurfaceQuantity::HeatFlux)
        .collect();
    let [a, b] = heat.as_slice() else {
        return None;
    };
    let (a, b) = (*a, *b);
    let temperature = |s: &PlannedSurface| {
        let boundary = resolved.boundary_conditions.get(&s.boundary)?;
        match boundary.thermal? {
            ResolvedThermal::FixedTemperature { value } => Some(value),
            ResolvedThermal::Adiabatic => None,
        }
    };
    let (ta, tb) = (temperature(a)?, temperature(b)?);
    if ta > tb {
        Some(HeatSpec {
            hot: a.fo.clone(),
            cold: b.fo.clone(),
        })
    } else if tb > ta {
        Some(HeatSpec {
            hot: b.fo.clone(),
            cold: a.fo.clone(),
        })
    } else {
        None
    }
}

/// What a run of this study monitors: the probes, lines, surfaces and forces of its monitor plan
/// (the default probes when the study lists none), one flux per patch for the mass balance, the
/// pair of wall heat flows for the energy balance, and the solver's log, pressure field and
/// residual limits. A steady run is held to the study's limits; a transient run has none and is
/// judged on periodic settledness. A monitor with no label of its own is shown by its
/// function-object name.
pub fn spec_for(resolved: &ResolvedStudy) -> Spec {
    let plan = resolved.monitor_plan();
    let solver = SolverPlan::for_study(resolved);
    let shown = |label: Option<String>, fo: &str| label.unwrap_or_else(|| fo.to_owned());
    let heat = heat_pair(resolved, &plan.surfaces);
    let surfaces = plan
        .surfaces
        .iter()
        .map(|s| SurfaceSpec {
            label: shown(s.label.clone(), &s.fo),
            fo: s.fo.clone(),
            quantity: surface_kind(s.quantity),
            component: heat_component(resolved, s),
        })
        .collect();
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
        surfaces,
        forces: plan
            .forces
            .into_iter()
            .map(|f| ForceSpec {
                label: shown(f.label, &f.fo),
                fo: f.fo,
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
        heat,
        solves_energy: solver.buoyant,
        temperature: solver.buoyant,
        residual_limits: (!solver.residual_checks.is_empty())
            .then_some((resolved.solver.convergence.p, resolved.solver.convergence.u)),
        transient: solver.transient,
        periodic: solver.transient,
        pressure_field: solver.pressure_field,
        log_file: solver.log_file,
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
        .chain(spec.forces.iter().map(|f| f.fo.as_str()))
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

/// Whether `name` is one plain file name: no separator, no dot parts, no control character.
fn plain_name(name: &str) -> bool {
    !matches!(name, "" | "." | "..")
        && !name
            .chars()
            .any(|c| c == '/' || c == '\\' || c.is_control())
}

/// The log and the monitor files of the results folder `dir`, for the function objects `spec` names.
/// The log is the one `spec.log_file` names. Nothing is followed through a link, and a file over
/// the size cap is left out.
pub fn files_from_dir(dir: &Path, spec: &Spec) -> Files {
    let log = plain_name(&spec.log_file)
        .then(|| read_plain(&dir.join(&spec.log_file), LOG_CAP))
        .flatten()
        .unwrap_or_default();
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
    let log = text_of(&spec.log_file).unwrap_or_default();
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
