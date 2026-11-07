//! The probe answers of a lattice run, presented to the benchmark comparers under the paths and in
//! the formats they already read, so the comparers, their tolerances and the gate report stay as
//! they are.
//!
//! Nothing is written to disk. The view is built the first time it is read, from the stored
//! requests and replies, and every number in it comes from a reply: a quantity the replies do not
//! hold, or hold only in part, has no file. Two things are never served at all: anything built from
//! a reply file longer than the read limit (a cut file would give a partial series), and the force
//! on a wall of the box (the force on a wall of the lattice carries the rest pressure of the
//! lattice over its area, which cancels only over a closed body).
//!
//! The requests say what each answer is: a `solve.probe` call per answer, each its own `execute`
//! call. An answer is matched to a monitor by its geometry, never by a position in the file.

use super::judge::{Request, parse_replies, parse_requests, reply_to};
use crate::{
    commands::REFERENCE_DENSITY,
    markers::{
        FIELD_VELOCITY, PROBE_COMMAND, READ_CAP_BYTES, REPLY_CELL, REPLY_FORCES, REPLY_HEAT,
        REPLY_POINTS, REPLY_PRESSURE, REPLY_SOLID, REPLY_TIME, REPLY_TIMES, REPLY_VALUES,
        REPLY_VELOCITY, REPLY_XYZ,
    },
    obstacles::{self, Lattice},
    plan::{COMMANDS_FILE, REPLIES_FILE},
    thermal,
};
use cfdcraft_backend::RunFiles;
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Face, MonitorField, ResolvedStudy, SurfaceQuantity};
use serde_json::Value;
use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
};

/// A run folder read through [`RunFiles`], for the code that wants the comparers' view of it.
pub(super) struct FromRun<'a>(pub &'a dyn RunFiles);

impl ResultFiles for FromRun<'_> {
    fn read(&self, rel: &str) -> Option<String> {
        self.0.read_text(rel)
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        self.0.list(rel_dir)
    }
}

/// One value of a point probe: what the reply holds for it.
struct Sample {
    velocity: Option<[f64; 3]>,
    pressure: Option<f64>,
}

/// One answered `solve.probe` call.
enum Record {
    Points {
        time: Option<f64>,
        asked: Vec<[f64; 3]>,
        samples: Vec<Option<Sample>>,
    },
    Line {
        from: [f64; 2],
        to: [f64; 2],
        field: String,
        xyz: Vec<[f64; 3]>,
        values: Vec<f64>,
    },
    Forces {
        times: Vec<f64>,
        walls: BTreeMap<String, Vec<[f64; 3]>>,
    },
    Heat {
        times: Vec<f64>,
        walls: BTreeMap<String, Vec<f64>>,
    },
}

fn numbers<const N: usize>(value: &Value) -> Option<[f64; N]> {
    let items = value.as_array()?;
    if items.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = item.as_f64().filter(|v| v.is_finite())?;
    }
    Some(out)
}

fn series(value: &Value) -> Option<Vec<f64>> {
    value
        .as_array()?
        .iter()
        .map(|v| v.as_f64().filter(|x| x.is_finite()))
        .collect()
}

fn sample_of(point: &Value) -> Option<Sample> {
    let outside = point.get(REPLY_CELL).is_some_and(Value::is_null);
    let solid = point.get(REPLY_SOLID).and_then(Value::as_bool) == Some(true);
    if outside || solid {
        return None;
    }
    Some(Sample {
        velocity: point.get(REPLY_VELOCITY).and_then(numbers::<3>),
        pressure: point
            .get(REPLY_PRESSURE)
            .and_then(Value::as_f64)
            .filter(|p| p.is_finite()),
    })
}

fn points_record(params: &Value, answer: &Value) -> Option<Record> {
    let asked = params
        .get(REPLY_POINTS)?
        .as_array()?
        .iter()
        .map(numbers::<3>)
        .collect::<Option<Vec<_>>>()?;
    let given = answer.get(REPLY_POINTS)?.as_array()?;
    if given.len() != asked.len() {
        return None;
    }
    Some(Record::Points {
        time: answer
            .get(REPLY_TIME)
            .and_then(Value::as_f64)
            .filter(|t| t.is_finite()),
        asked,
        samples: given.iter().map(sample_of).collect(),
    })
}

fn line_record(params: &Value, answer: &Value) -> Option<Record> {
    let line = params.get("line")?;
    let end = |name: &str| {
        let [x, y, _] = numbers::<3>(line.get(name)?)?;
        Some([x, y])
    };
    let xyz = answer
        .get(REPLY_XYZ)?
        .as_array()?
        .iter()
        .map(numbers::<3>)
        .collect::<Option<Vec<_>>>()?;
    let values = series(answer.get(REPLY_VALUES)?)?;
    if xyz.len() != values.len() {
        return None;
    }
    Some(Record::Line {
        from: end("from")?,
        to: end("to")?,
        field: line.get("field")?.as_str()?.to_owned(),
        xyz,
        values,
    })
}

/// The times of an answer and, for each wall name, its history.
type Histories<T> = (Vec<f64>, BTreeMap<String, Vec<T>>);

/// The per-wall histories of a force or heat answer: every wall whose history is as long as the
/// times. A wall whose history is not is left out.
fn histories<T>(
    answer: &Value,
    member: &str,
    read: impl Fn(&Value) -> Option<T>,
) -> Option<Histories<T>> {
    let times = series(answer.get(REPLY_TIMES)?)?;
    let mut out = BTreeMap::new();
    for (name, history) in answer.get(member)?.as_object()? {
        let rows = history
            .as_array()
            .and_then(|items| items.iter().map(&read).collect::<Option<Vec<T>>>());
        if let Some(rows) = rows.filter(|rows| rows.len() == times.len()) {
            out.insert(name.clone(), rows);
        }
    }
    Some((times, out))
}

fn records(requests: &[Request], replies_text: &str) -> Vec<Record> {
    let replies = parse_replies(replies_text);
    let mut out = Vec::new();
    for request in requests {
        let Some((name, params)) = request.command() else {
            continue;
        };
        let answer = reply_to(&replies, request.id).and_then(|r| r.payload.as_ref());
        let (PROBE_COMMAND, Some(answer)) = (name, answer) else {
            continue;
        };
        let record = if params.get(REPLY_POINTS).is_some() {
            points_record(params, answer)
        } else if params.get("line").is_some() {
            line_record(params, answer)
        } else if params.get(REPLY_FORCES).is_some() {
            histories(answer, REPLY_FORCES, numbers::<3>)
                .map(|(times, walls)| Record::Forces { times, walls })
        } else if params.get(REPLY_HEAT).is_some() {
            histories(answer, REPLY_HEAT, Value::as_f64)
                .map(|(times, walls)| Record::Heat { times, walls })
        } else {
            None
        };
        out.extend(record);
    }
    out
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

fn near2(a: [f64; 2], b: [f64; 2]) -> bool {
    near(a[0], b[0]) && near(a[1], b[1])
}

/// The file of a velocity line: the distance from the start of the line and the three velocity
/// components, from the last answers for the line's three components. Missing when any component
/// is missing or the three do not share their sample points.
fn velocity_file(records: &[Record], from: [f64; 2], to: [f64; 2]) -> Option<String> {
    let component = |field: &str| {
        records.iter().rev().find_map(|record| match record {
            Record::Line {
                from: f,
                to: t,
                field: name,
                xyz,
                values,
            } if name == field && near2(*f, from) && near2(*t, to) => Some((xyz, values)),
            _ => None,
        })
    };
    let (xyz, ux) = component(FIELD_VELOCITY[0])?;
    let (xyz_y, uy) = component(FIELD_VELOCITY[1])?;
    let (xyz_z, uz) = component(FIELD_VELOCITY[2])?;
    let same = |other: &Vec<[f64; 3]>| {
        other.len() == xyz.len()
            && other
                .iter()
                .zip(xyz)
                .all(|(a, b)| near(a[0], b[0]) && near(a[1], b[1]))
    };
    if xyz.len() < 2 || !same(xyz_y) || !same(xyz_z) {
        return None;
    }
    let mut text = String::from("# distance Ux Uy Uz\n");
    for (i, point) in xyz.iter().enumerate() {
        let distance = (point[0] - from[0]).hypot(point[1] - from[1]);
        text.push_str(&format!("{distance} {} {} {}\n", ux[i], uy[i], uz[i]));
    }
    Some(text)
}

/// The name the solver gives a side of the box.
fn side(face: Face) -> Option<&'static str> {
    match face {
        Face::XMin => Some("x_min"),
        Face::XMax => Some("x_max"),
        Face::YMin => Some("y_min"),
        Face::YMax => Some("y_max"),
        Face::Obstacle => None,
    }
}

/// The text of a probe file for the point of a monitor: one row per answered frame, in time order.
fn probe_file(
    records: &[Record],
    at: [f64; 2],
    radius: f64,
    field: MonitorField,
) -> Option<String> {
    let mut rows: Vec<(f64, String)> = Vec::new();
    let mut place = None;
    for record in records {
        let Record::Points {
            time: Some(time),
            asked,
            samples,
        } = record
        else {
            continue;
        };
        let nearest = asked
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (p[0] - at[0]).hypot(p[1] - at[1])))
            .filter(|(_, d)| *d <= radius)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((i, _)) = nearest else { continue };
        let Some(Some(sample)) = samples.get(i) else {
            continue;
        };
        let value = match field {
            MonitorField::Pressure => sample.pressure.map(|p| (p / REFERENCE_DENSITY).to_string()),
            MonitorField::Velocity => sample
                .velocity
                .map(|u| format!("({} {} {})", u[0], u[1], u[2])),
        };
        if let Some(value) = value {
            place.get_or_insert(asked[i]);
            rows.push((*time, value));
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.dedup_by(|b, a| a.0 == b.0);
    let place = place?;
    let mut text = format!(
        "# Probe 0 ({} {} {})\n#       Probe             0\n#        Time\n",
        place[0], place[1], place[2]
    );
    for (time, value) in rows {
        text.push_str(&format!("{time} {value}\n"));
    }
    Some(text)
}

/// The history of the walls whose names are `keys`: their sum per time. Every key must be present
/// in the answer.
fn summed<T: Copy>(
    times: &[f64],
    walls: &BTreeMap<String, Vec<T>>,
    keys: &[&str],
    zero: T,
    add: impl Fn(T, T) -> T,
) -> Option<Vec<T>> {
    let histories = keys
        .iter()
        .map(|key| walls.get(*key))
        .collect::<Option<Vec<_>>>()?;
    if histories.is_empty() {
        return None;
    }
    Some(
        (0..times.len())
            .map(|i| histories.iter().fold(zero, |sum, h| add(sum, h[i])))
            .collect(),
    )
}

/// The force coefficients of a body: the drag along x and the lift along y over the dynamic
/// pressure at the monitor's reference speed and its reference length across the slab. The body's
/// walls are every wall of the answer named for the body.
fn coefficient_file(
    records: &[Record],
    body: &str,
    reference_velocity: f64,
    reference_length: f64,
    slab: f64,
) -> Option<String> {
    let dynamic = 0.5 * REFERENCE_DENSITY * reference_velocity * reference_velocity;
    let area = reference_length * slab;
    let scale = dynamic * area;
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let prefix = format!("{body}/");
    let (times, force) = records.iter().rev().find_map(|record| {
        let Record::Forces { times, walls } = record else {
            return None;
        };
        let keys: Vec<&str> = walls
            .keys()
            .map(String::as_str)
            .filter(|k| *k == body || k.starts_with(&prefix))
            .collect();
        let sum = summed(times, walls, &keys, [0.0; 3], |a, b| {
            [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
        })?;
        Some((times, sum))
    })?;
    if times.is_empty() || times.windows(2).any(|w| w[0] >= w[1]) {
        return None;
    }
    let mut text = String::from(
        "# Force coefficients\n# dragDir      : (1 0 0)\n# liftDir      : (0 1 0)\n# Time\tCd\tCl\n",
    );
    for (time, f) in times.iter().zip(&force) {
        text.push_str(&format!("{time}\t{}\t{}\n", f[0] / scale, f[1] / scale));
    }
    Some(text)
}

/// The sign of the outward normal of a side of the box along its own axis, and the axis (0 for x).
fn outward(face: Face) -> Option<(f64, usize)> {
    match face {
        Face::XMin => Some((-1.0, 0)),
        Face::XMax => Some((1.0, 0)),
        Face::YMin => Some((-1.0, 1)),
        Face::YMax => Some((1.0, 1)),
        Face::Obstacle => None,
    }
}

/// The heat surface of a wall of the box: the area integral of the temperature gradient along the
/// wall's axis, from the power into the wall. The power into a wall is `-k` times the integral of
/// the gradient along the outward normal, so the integral along the axis is `-n P / k` for a side
/// whose outward normal along the axis is `n`.
fn surface_file(
    records: &[Record],
    boundary: &str,
    face: Face,
    conductivity: f64,
    lattice: &Lattice,
    study: &ResolvedStudy,
) -> Option<String> {
    let (normal, axis) = outward(face)?;
    let key = side(face)?;
    let (times, power) = records.iter().rev().find_map(|record| {
        let Record::Heat { times, walls } = record else {
            return None;
        };
        let sum = summed(times, walls, &[key], 0.0, |a, b| a + b)?;
        Some((times, sum))
    })?;
    if times.is_empty() || !(conductivity.is_finite() && conductivity > 0.0) {
        return None;
    }
    let length = if axis == 0 {
        study.domain.length_y
    } else {
        study.domain.length_x
    };
    let area = length * lattice.slab();
    let faces = (length / lattice.spacing).round();
    let mut text = format!(
        "# Region type : patch {boundary}\n# Faces : {faces}\n# Area : {area}\n# Time areaIntegrate(gradT)\n"
    );
    for (time, p) in times.iter().zip(&power) {
        let integral = -normal * p / conductivity;
        let vector = if axis == 0 {
            format!("({integral} 0 0)")
        } else {
            format!("(0 {integral} 0)")
        };
        text.push_str(&format!("{time} {vector}\n"));
    }
    Some(text)
}

/// The virtual files of a study: `(path, text)`.
fn build(study: &ResolvedStudy, requests: &[Request], replies: &str) -> BTreeMap<String, String> {
    let records = records(requests, replies);
    let end = study.solver.end_time.to_string();
    let plan = study.monitor_plan();
    let lattice = obstacles::lattice(study).ok();
    let mut files = BTreeMap::new();
    let (lx, ly) = (study.domain.length_x, study.domain.length_y);
    let centrelines = [
        ("vertical_U.xy", [lx / 2.0, 0.0], [lx / 2.0, ly]),
        ("horizontal_U.xy", [0.0, ly / 2.0], [lx, ly / 2.0]),
    ];
    for (name, from, to) in centrelines {
        if let Some(text) = velocity_file(&records, from, to) {
            files.insert(format!("postProcessing/centreline/{end}/{name}"), text);
        }
    }
    for line in &plan.lines {
        if line.field != MonitorField::Velocity {
            continue;
        }
        let from = [line.x0, line.y0];
        let to = [line.x1, line.y1];
        if let Some(text) = velocity_file(&records, from, to) {
            files.insert(format!("postProcessing/{}/{end}/line_U.xy", line.fo), text);
        }
    }
    for probe in &plan.probes {
        // A probe on a surface samples the fluid cell nearest it: one cell is the reach.
        let reach = match (&probe.boundary, &lattice) {
            (Some(_), Some(lattice)) => lattice.spacing,
            _ => 1e-9,
        };
        let name = match probe.field {
            MonitorField::Pressure => "p",
            MonitorField::Velocity => "U",
        };
        if let Some(text) = probe_file(&records, [probe.x, probe.y], reach, probe.field) {
            files.insert(format!("postProcessing/{}/0/{name}", probe.fo), text);
        }
    }
    let Some(lattice) = lattice else {
        return files;
    };
    for force in &plan.forces {
        let on_body = study
            .boundary_conditions
            .get(&force.boundary)
            .is_some_and(|b| b.faces == [Face::Obstacle]);
        let Some(body) = lattice
            .obstacle
            .as_ref()
            .map(|o| o.name)
            .filter(|_| on_body)
        else {
            continue;
        };
        let text = coefficient_file(
            &records,
            body,
            force.reference_velocity,
            force.reference_length,
            lattice.slab(),
        );
        if let Some(text) = text {
            files.insert(
                format!("postProcessing/{}/0/coefficient.dat", force.fo),
                text,
            );
        }
    }
    let conductivity = thermal::material(study, REFERENCE_DENSITY)
        .ok()
        .flatten()
        .and_then(|m| m.get("conductivity").and_then(Value::as_f64));
    for surface in &plan.surfaces {
        let faces = study
            .boundary_conditions
            .get(&surface.boundary)
            .map(|b| b.faces.as_slice());
        let (SurfaceQuantity::HeatFlux, Some(&[face]), Some(k)) =
            (surface.quantity, faces, conductivity)
        else {
            continue;
        };
        let text = surface_file(&records, &surface.boundary, face, k, &lattice, study);
        if let Some(text) = text {
            files.insert(
                format!("postProcessing/{}/{end}/surfaceFieldValue.dat", surface.fo),
                text,
            );
        }
    }
    files
}

/// The names of the entries directly below a folder, among paths of a map.
fn children(paths: &BTreeMap<String, String>, dir: &str) -> BTreeSet<String> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    paths
        .keys()
        .filter_map(|path| path.strip_prefix(&prefix))
        .filter_map(|rest| rest.split('/').next())
        .map(str::to_owned)
        .collect()
}

/// A plain relative path: no absolute name, no `..` part.
fn plain(rel: &str) -> bool {
    !rel.starts_with('/') && !rel.contains('\\') && rel.split('/').all(|part| part != "..")
}

/// The comparers' view of a lattice run: the native files, and the virtual ones built from the
/// probe answers.
pub(super) struct View<'a> {
    native: Box<dyn ResultFiles + 'a>,
    study: ResolvedStudy,
    built: OnceCell<BTreeMap<String, String>>,
}

impl<'a> View<'a> {
    pub fn new(native: Box<dyn ResultFiles + 'a>, study: &ResolvedStudy) -> Self {
        Self {
            native,
            study: study.clone(),
            built: OnceCell::new(),
        }
    }

    fn virtual_files(&self) -> &BTreeMap<String, String> {
        self.built.get_or_init(|| {
            let cap = usize::try_from(READ_CAP_BYTES).unwrap_or(usize::MAX);
            let replies = self.native.read(REPLIES_FILE);
            let requests = self.native.read(COMMANDS_FILE);
            match (requests, replies) {
                (Some(requests), Some(replies))
                    if requests.len() <= cap && replies.len() <= cap =>
                {
                    build(&self.study, &parse_requests(&requests), &replies)
                }
                _ => BTreeMap::new(),
            }
        })
    }
}

impl ResultFiles for View<'_> {
    fn read(&self, rel: &str) -> Option<String> {
        if !plain(rel) {
            return None;
        }
        match self.virtual_files().get(rel) {
            Some(text) => Some(text.clone()),
            None => self.native.read(rel),
        }
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        if !plain(rel_dir) {
            return Vec::new();
        }
        let dir = rel_dir.trim_end_matches('/');
        let mut names = children(self.virtual_files(), dir);
        names.extend(self.native.list(dir));
        names.into_iter().collect()
    }
}
