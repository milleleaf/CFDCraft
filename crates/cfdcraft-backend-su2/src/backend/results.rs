//! The result files of an SU2 run, presented under the paths the benchmark comparers read.
//!
//! The comparers read OpenFOAM-style monitor files below `postProcessing`. This view builds those
//! files at read time from the files the solver wrote (the history and the volume solution) and
//! writes nothing. A path whose quantity the native files cannot give is absent: the comparer
//! then reports missing output, and nothing is invented.
//!
//! What is presented, and from what:
//! - the cavity centre lines (`centreline`) and every velocity line of the study (`line_<n>`),
//!   from the volume solution, sampled at evenly spaced points by the bilinear map of the cell
//!   that holds each point (at a node this is the node's own value);
//! - the heat flux surfaces of the study (`surface_<n>`), from the temperature of the volume
//!   solution by the wall rule of `Volume::wall_gradient`;
//! - the drag and lift series of a study with exactly one force monitor (`force_<n>`), and the
//!   pressure series of the pressure probes (`probe_<n>`), from history columns, when the history
//!   has them. The case written today does not ask for those columns, so a real run has none yet.
//!
//! Not presented: pressure lines, velocity probes, flux and pressure surfaces, forces beyond the
//! first when a study has several. A file over the read limit gives nothing built from it.

use super::native::{Table, Volume, within_cap};
use crate::markers::{HISTORY_DRAG, HISTORY_FILE, HISTORY_LIFT, HISTORY_TIME, VOLUME_FILE};
use cfdcraft_backend::RunFiles;
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Face, MonitorField, ResolvedStudy, SurfaceQuantity};
use cfdcraft_shapes::Side;
use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
};

/// The most samples one line gets.
const MAX_SAMPLES: u64 = 100_000;

enum Source<'a> {
    Owned(Box<dyn ResultFiles>),
    Borrowed(&'a dyn ResultFiles),
}

/// The files of a run as the comparers read them.
pub struct NativeView<'a> {
    inner: Source<'a>,
    study: ResolvedStudy,
    built: OnceCell<BTreeMap<String, String>>,
}

impl NativeView<'static> {
    /// The view over a folder of stored results.
    pub fn owned(files: Box<dyn ResultFiles>, study: &ResolvedStudy) -> Self {
        Self {
            inner: Source::Owned(files),
            study: study.clone(),
            built: OnceCell::new(),
        }
    }
}

impl<'a> NativeView<'a> {
    /// The view over files that are only borrowed.
    pub fn over(files: &'a dyn ResultFiles, study: &ResolvedStudy) -> Self {
        Self {
            inner: Source::Borrowed(files),
            study: study.clone(),
            built: OnceCell::new(),
        }
    }

    fn files(&self) -> &dyn ResultFiles {
        match &self.inner {
            Source::Owned(files) => files.as_ref(),
            Source::Borrowed(files) => *files,
        }
    }

    /// A native file, unless it is over the read limit.
    fn native(&self, name: &str) -> Option<String> {
        self.files().read(name).filter(|text| within_cap(text))
    }

    fn table(&self, name: &str) -> Option<Table> {
        Table::parse(&self.native(name)?).ok()
    }

    fn built(&self) -> &BTreeMap<String, String> {
        self.built.get_or_init(|| self.build())
    }

    fn build(&self) -> BTreeMap<String, String> {
        let study = &self.study;
        let folder = study.solver.end_time.to_string();
        let history = self.table(HISTORY_FILE).filter(Table::all_finite);
        let volume = self
            .table(VOLUME_FILE)
            .and_then(|table| Volume::bind(&table, study).ok());
        let mut out = BTreeMap::new();
        if let Some(volume) = &volume {
            centrelines(volume, study, &folder, &mut out);
            lines(volume, study, &folder, &mut out);
            surfaces(volume, study, &folder, &mut out);
        }
        if let Some(history) = &history {
            series(history, study, &mut out);
        }
        out
    }
}

impl ResultFiles for NativeView<'_> {
    fn read(&self, rel: &str) -> Option<String> {
        self.built().get(rel).cloned().or_else(|| self.native(rel))
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        let prefix = if rel_dir.is_empty() {
            String::new()
        } else {
            format!("{}/", rel_dir.trim_end_matches('/'))
        };
        let mut names: BTreeSet<String> = self
            .built()
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .map(|rest| rest.split('/').next().unwrap_or(rest).to_owned())
            .collect();
        names.extend(self.files().list(rel_dir));
        names.into_iter().collect()
    }
}

/// The files of a run folder read as results, so the view can sit on top of them.
pub struct RunAsResults<'a>(pub &'a dyn RunFiles);

impl ResultFiles for RunAsResults<'_> {
    fn read(&self, rel: &str) -> Option<String> {
        self.0.read_text(rel)
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        self.0.list(rel_dir)
    }
}

type Points = Vec<([f64; 2], [f64; 2])>;

/// `n` points evenly spaced from `a` to `b`, with the end points included; `None` when one of them
/// lies outside the mesh.
fn along(volume: &Volume, a: [f64; 2], b: [f64; 2], n: u64) -> Option<Points> {
    if !(2..=MAX_SAMPLES).contains(&n) {
        return None;
    }
    let last = (n - 1) as f64;
    (0..n)
        .map(|i| {
            let f = i as f64 / last;
            let at = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
            Some((at, volume.sample(at[0], at[1])?))
        })
        .collect()
}

/// The raw set format of the centre lines: the coordinate along the line, then the three velocity
/// components.
fn raw_text(points: &Points, axis: usize) -> String {
    points
        .iter()
        .map(|(at, s)| format!("{:.12} {:.12} {:.12} {:.12}\n", at[axis], s[0], s[1], 0.0))
        .collect()
}

/// The six column format of the monitor lines: x, y, z, then the three velocity components.
fn xyz_text(points: &Points, z: f64) -> String {
    points
        .iter()
        .map(|(at, s)| {
            format!(
                "{:.12} {:.12} {z:.12} {:.12} {:.12} {:.12}\n",
                at[0], at[1], s[0], s[1], 0.0
            )
        })
        .collect()
}

fn centrelines(
    volume: &Volume,
    s: &ResolvedStudy,
    folder: &str,
    out: &mut BTreeMap<String, String>,
) {
    let n = s.monitors.centreline_u_sample_count;
    let (lx, ly) = (s.domain.length_x, s.domain.length_y);
    let sets = [
        ("vertical_U.xy", [lx / 2.0, 0.0], [lx / 2.0, ly], 1),
        ("horizontal_U.xy", [0.0, ly / 2.0], [lx, ly / 2.0], 0),
    ];
    for (name, a, b, axis) in sets {
        if let Some(points) = along(volume, a, b, n) {
            out.insert(
                format!("postProcessing/centreline/{folder}/{name}"),
                raw_text(&points, axis),
            );
        }
    }
}

fn lines(volume: &Volume, s: &ResolvedStudy, folder: &str, out: &mut BTreeMap<String, String>) {
    let z = s.domain.length_z / 2.0;
    for line in s.monitor_plan().lines {
        if line.field != MonitorField::Velocity {
            continue;
        }
        if let Some(points) = along(volume, [line.x0, line.y0], [line.x1, line.y1], line.samples) {
            out.insert(
                format!("postProcessing/{}/{folder}/line_U.xy", line.fo),
                xyz_text(&points, z),
            );
        }
    }
}

fn side(face: Face) -> Side {
    match face {
        Face::XMin => Side::XMin,
        Face::XMax => Side::XMax,
        Face::YMin => Side::YMin,
        Face::YMax => Side::YMax,
        Face::Obstacle => Side::Obstacle,
    }
}

fn surfaces(volume: &Volume, s: &ResolvedStudy, folder: &str, out: &mut BTreeMap<String, String>) {
    for surface in s.monitor_plan().surfaces {
        if surface.quantity != SurfaceQuantity::HeatFlux {
            continue;
        }
        let Some(boundary) = s.boundary_conditions.get(&surface.boundary) else {
            continue;
        };
        let sides: Vec<Side> = boundary.faces.iter().copied().map(side).collect();
        let Some(wall) = volume.wall_gradient(&sides) else {
            continue;
        };
        out.insert(
            format!("postProcessing/{}/{folder}/surfaceFieldValue.dat", surface.fo),
            format!(
                "# Region type : patch {}\n# Faces : {}\n# Area : {:.12}\n# Time areaIntegrate(gradT)\n{folder} ({:.12} {:.12} 0.000000000000)\n",
                surface.boundary, wall.edges, wall.area, wall.gradient[0], wall.gradient[1]
            ),
        );
    }
}

/// The series files built from history columns. All start in the folder `0`, the start time.
fn series(history: &Table, s: &ResolvedStudy, out: &mut BTreeMap<String, String>) {
    let Some(times) = history.values(HISTORY_TIME) else {
        return;
    };
    let plan = s.monitor_plan();
    // One drag column and one lift column cover every monitored boundary together, so they
    // belong to a force monitor only when the study has just one.
    if let ([force], Some(drag), Some(lift)) = (
        plan.forces.as_slice(),
        history.values(HISTORY_DRAG),
        history.values(HISTORY_LIFT),
    ) {
        let rows: String = times
            .iter()
            .zip(&drag)
            .zip(&lift)
            .map(|((t, cd), cl)| format!("{t:.12}\t{cd:.12}\t{cl:.12}\n"))
            .collect();
        out.insert(
            format!("postProcessing/{}/0/coefficient.dat", force.fo),
            format!(
                "# Force coefficients\n# dragDir      : (1 0 0)\n# liftDir      : (0 1 0)\n# Time\tCd\tCl\n{rows}"
            ),
        );
    }
    let z = s.domain.length_z / 2.0;
    for probe in plan
        .probes
        .iter()
        .filter(|p| p.field == MonitorField::Pressure)
    {
        let Some(values) = history.values(&probe.fo) else {
            continue;
        };
        let rows: String = times
            .iter()
            .zip(&values)
            .map(|(t, p)| format!("{t:.12}   {p:.12}\n"))
            .collect();
        out.insert(
            format!("postProcessing/{}/0/p", probe.fo),
            format!(
                "# Probe 0 ({:.12} {:.12} {z:.12})\n#       Probe             0\n#        Time\n{rows}",
                probe.x, probe.y
            ),
        );
    }
}
