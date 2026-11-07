//! Finding the monitors of the cylinder benchmark in a results tree and reading their series.

use cfdcraft_gate::ResultFiles;
use cfdcraft_model::ResolvedStudy;
use cfdcraft_monitors::{
    FieldKind, Series, coefficient_series, join_series, parse_coefficients, parse_probes,
    probe_series,
};

use crate::metrics::Refusal;

/// The label of the force monitor on the cylinder.
pub const FORCE: &str = "cylinder_forces";
/// The label of the pressure probe on the upstream side of the cylinder.
pub const FRONT: &str = "front";
/// The label of the pressure probe on the downstream side of the cylinder.
pub const REAR: &str = "rear";

/// The function objects of the three monitors, and the force's reference values.
pub(crate) struct Fos {
    pub force: String,
    pub front: String,
    pub rear: String,
    pub reference_velocity: f64,
    pub reference_length: f64,
}

fn absent(label: &str) -> Refusal {
    Refusal::Missing {
        file: format!("the monitor labelled {label}"),
    }
}

/// The function objects the study writes for its three labelled monitors. A study that lacks one
/// is refused by name; the written case numbers them by position in label order.
pub(crate) fn names(study: &ResolvedStudy) -> Result<Fos, Refusal> {
    let plan = study.monitor_plan();
    let probe = |label: &str| {
        plan.probes
            .iter()
            .find(|p| p.label.as_deref() == Some(label))
            .map(|p| p.fo.clone())
            .ok_or_else(|| absent(label))
    };
    let force = plan
        .forces
        .iter()
        .find(|f| f.label.as_deref() == Some(FORCE))
        .ok_or_else(|| absent(FORCE))?;
    Ok(Fos {
        force: force.fo.clone(),
        front: probe(FRONT)?,
        rear: probe(REAR)?,
        reference_velocity: force.reference_velocity,
        reference_length: force.reference_length,
    })
}

/// The series the comparison measures, each joined across the start-time folders of a continued
/// run.
pub(crate) struct Inputs {
    pub cd: Series,
    pub cl: Series,
    pub front: Series,
    pub rear: Series,
}

/// `(path, text)` of `file` in every start-time folder of a function object, oldest first. A
/// folder whose name is not a finite number is not a start time and is skipped.
fn pieces(files: &dyn ResultFiles, fo: &str, file: &str) -> Result<Vec<(String, String)>, Refusal> {
    let base = format!("postProcessing/{fo}");
    let mut folders: Vec<(f64, String)> = files
        .list(&base)
        .into_iter()
        .filter_map(|name| {
            let start = name.parse::<f64>().ok().filter(|t| t.is_finite())?;
            Some((start, name))
        })
        .collect();
    folders.sort_by(|a, b| a.0.total_cmp(&b.0));
    if folders.is_empty() {
        return Err(Refusal::Missing { file: base });
    }
    let mut out = Vec::new();
    for (_, name) in folders {
        let path = format!("{base}/{name}/{file}");
        let Some(text) = files.read(&path) else {
            return Err(Refusal::Missing { file: path });
        };
        out.push((path, text));
    }
    Ok(out)
}

/// True for a lift direction along y, to 1e-9.
fn along_y(direction: Option<[f64; 3]>) -> bool {
    matches!(direction, Some([x, y, z]) if x.abs() < 1e-9 && (y - 1.0).abs() < 1e-9 && z.abs() < 1e-9)
}

fn coefficients(files: &dyn ResultFiles, fo: &str) -> Result<(Series, Series), Refusal> {
    let (mut cd, mut cl) = (Vec::new(), Vec::new());
    for (path, text) in pieces(files, fo, "coefficient.dat")? {
        let unreadable = || Refusal::Unreadable { file: path.clone() };
        let file = parse_coefficients(&text).map_err(|_| unreadable())?;
        // A coefficient file written with the lift along another axis holds a lift that is not
        // the force across the flow, so a number read from it would be silently wrong.
        if !along_y(file.lift_dir) {
            return Err(Refusal::LiftDirection);
        }
        let series = coefficient_series(&file, fo);
        let column = |suffix: &str| {
            let name = format!("{fo}.{suffix}");
            series.iter().find(|s| s.name == name).cloned()
        };
        cd.push(column("Cd").ok_or_else(unreadable)?);
        cl.push(column("Cl").ok_or_else(unreadable)?);
    }
    Ok((join_series(cd), join_series(cl)))
}

fn pressure(files: &dyn ResultFiles, fo: &str) -> Result<Series, Refusal> {
    let mut parts = Vec::new();
    for (path, text) in pieces(files, fo, "p")? {
        let unreadable = || Refusal::Unreadable { file: path.clone() };
        let file = parse_probes(&text).map_err(|_| unreadable())?;
        let series = probe_series(&file, fo, 0, FieldKind::Pressure)
            .into_iter()
            .find(|s| !s.times.is_empty());
        parts.push(series.ok_or_else(unreadable)?);
    }
    Ok(join_series(parts))
}

pub(crate) fn load(files: &dyn ResultFiles, fos: &Fos) -> Result<Inputs, Refusal> {
    let (cd, cl) = coefficients(files, &fos.force)?;
    Ok(Inputs {
        cd,
        cl,
        front: pressure(files, &fos.front)?,
        rear: pressure(files, &fos.rear)?,
    })
}
