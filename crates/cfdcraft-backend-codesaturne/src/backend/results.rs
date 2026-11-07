//! The view of a finished run that the benchmark comparers read.
//!
//! The comparers read OpenFOAM's layout: `postProcessing/<monitor>/<time>/<file>`. The solver
//! writes its monitoring histories as two CSV files with one column group per monitoring point, so
//! this view builds the text of each path the comparers know from those files at the moment it is
//! asked. Nothing is written, and a path whose quantity the files cannot give does not exist:
//! surface integrals and force coefficients are not written for this solver, so no comparer that
//! needs them finds a file.
//!
//! A profile holds the final values at its points exactly as the solver recorded them: the points
//! were placed where the profile is, and the solver interpolates to them, so no value here is
//! interpolated or invented. A history file longer than the read limit gives no path at all, never
//! a partial series.

use crate::{
    markers::{
        MONITOR_PRESSURE_COLUMNS, MONITOR_TIME_COLUMNS, MONITOR_VELOCITY_COLUMNS,
        PROBES_PRESSURE_FILE, PROBES_VELOCITY_FILE, READ_CAP_BYTES,
    },
    probes::{CENTRELINE_HORIZONTAL, CENTRELINE_VERTICAL, ProbePoint, ProbeSet, probe_set},
};
use cfdcraft_backend::RunFiles;
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{MonitorField, ResolvedStudy};
use std::{collections::BTreeSet, fmt::Write as _};

/// The folder every point history is put in, as OpenFOAM names the first start time.
const SERIES_FOLDER: &str = "0";

/// A history: a time and the values of one point at that time, row by row.
type History = Vec<(f64, Vec<f64>)>;

/// A comma separated file: the last header line before the data, and the data rows.
pub(super) struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<f64>>,
}

/// Why a file is not a table of finite numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableError {
    /// No data row.
    Empty,
    /// A value that is not a number, or is not finite, or a header after the data began.
    NotFinite,
}

/// Read a comma separated file. A line before the first data row whose first field is not a number,
/// or that starts with `#`, is a header; every other line is data and every field of it must be a
/// finite number.
pub(super) fn parse_table(text: &str) -> Result<Table, TableError> {
    let mut header = Vec::new();
    let mut rows: Vec<Vec<f64>> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let body = line.trim_start_matches('#');
        let fields: Vec<&str> = body.split(',').map(str::trim).collect();
        let numeric = !line.starts_with('#')
            && fields
                .first()
                .is_some_and(|first| first.parse::<f64>().is_ok());
        if !numeric {
            if !rows.is_empty() {
                return Err(TableError::NotFinite);
            }
            header = fields.iter().map(|f| (*f).to_owned()).collect();
            continue;
        }
        let row = fields
            .iter()
            .map(|f| f.parse::<f64>().ok().filter(|v| v.is_finite()))
            .collect::<Option<Vec<f64>>>()
            .ok_or(TableError::NotFinite)?;
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(TableError::Empty);
    }
    Ok(Table { header, rows })
}

/// A run folder read as result files, so the same view serves a stored result and a judge.
pub(super) struct RunSide<'a>(pub &'a dyn RunFiles);

impl ResultFiles for RunSide<'_> {
    fn read(&self, rel: &str) -> Option<String> {
        self.0.read_text(rel)
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        self.0.list(rel_dir)
    }
}

/// Where the first column of a profile line comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    /// The x of the point.
    X,
    /// The y of the point.
    Y,
    /// The distance from the start of the line.
    From(f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    /// The final velocity along a run of points: distance, Ux, Uy, Uz on every row.
    Profile { run: String, origin: Origin },
    /// The history of one point, in the layout of an OpenFOAM probe file.
    Series { run: String, field: MonitorField },
}

/// One path of the view and what it is built from.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    monitor: String,
    folder: String,
    file: String,
    kind: Kind,
}

impl Entry {
    fn path(&self) -> String {
        format!(
            "postProcessing/{}/{}/{}",
            self.monitor, self.folder, self.file
        )
    }
}

/// The result files of a run as the comparers read them, over the files as the solver wrote them.
pub(super) struct ResultView<'a> {
    inner: Box<dyn ResultFiles + 'a>,
    set: ProbeSet,
    entries: Vec<Entry>,
}

impl<'a> ResultView<'a> {
    pub(super) fn new(inner: Box<dyn ResultFiles + 'a>, study: &ResolvedStudy) -> Self {
        // A study whose points cannot be placed has no profile to read: the view then serves
        // only the native files.
        let set = probe_set(study).unwrap_or_default();
        let entries = entries_of(study, &set);
        Self {
            inner,
            set,
            entries,
        }
    }

    /// A monitoring history, or `None` when it is missing, past the read limit or not a table of
    /// finite numbers.
    fn history(&self, rel: &str) -> Option<Table> {
        let text = self.inner.read(rel)?;
        if text.len() as u64 > READ_CAP_BYTES {
            return None;
        }
        parse_table(&text).ok()
    }

    /// The values of one point in one row, `None` when the row is too short to hold the point.
    fn point_values(row: &[f64], index: usize, width: usize) -> Option<&[f64]> {
        let from = MONITOR_TIME_COLUMNS + index * width;
        row.get(from..from + width)
    }

    fn profile(&self, run: &str, origin: Origin) -> Option<String> {
        let first = self.set.run(run)?.first;
        let points = self.set.points_of(run)?;
        let table = self.history(PROBES_VELOCITY_FILE)?;
        let row = table.rows.last()?;
        let mut text = String::new();
        for (at, point) in points.iter().enumerate() {
            let u = Self::point_values(row, first + at, MONITOR_VELOCITY_COLUMNS)?;
            let distance = match origin {
                Origin::X => point.x,
                Origin::Y => point.y,
                Origin::From(x0, y0) => (point.x - x0).hypot(point.y - y0),
            };
            let _ = writeln!(
                text,
                "{distance:.12} {:.12} {:.12} {:.12}",
                u[0], u[1], u[2]
            );
        }
        Some(text)
    }

    /// The point of a single-point run and its history: a time, then the values of the field
    /// (one for a pressure, three for a velocity) on every row.
    fn series_rows(&self, run: &str, field: MonitorField) -> Option<(ProbePoint, History)> {
        let first = self.set.run(run)?.first;
        let point = *self.set.points_of(run)?.first()?;
        let (file, width) = match field {
            MonitorField::Velocity => (PROBES_VELOCITY_FILE, MONITOR_VELOCITY_COLUMNS),
            MonitorField::Pressure => (PROBES_PRESSURE_FILE, MONITOR_PRESSURE_COLUMNS),
        };
        let table = self.history(file)?;
        let rows = table
            .rows
            .iter()
            .map(|row| {
                let values = Self::point_values(row, first, width)?;
                Some((*row.first()?, values.to_vec()))
            })
            .collect::<Option<Vec<_>>>()?;
        Some((point, rows))
    }

    fn series(&self, run: &str, field: MonitorField) -> Option<String> {
        let (point, rows) = self.series_rows(run, field)?;
        let mut text = format!(
            "# Probe 0 ({:.12} {:.12} {:.12})\n#       Probe             0\n#        Time\n",
            point.x, point.y, point.z
        );
        for (time, values) in rows {
            let _ = match values.as_slice() {
                [p] => writeln!(text, "{time:.12}   {p:.12}"),
                [x, y, z] => writeln!(text, "{time:.12}   ({x:.12} {y:.12} {z:.12})"),
                _ => return None,
            };
        }
        Some(text)
    }

    /// The history of one scalar of a monitored point as (times, values): the pressure of a
    /// pressure point, and the transverse velocity (Uy) of a velocity point.
    pub(super) fn scalar_series(
        &self,
        run: &str,
        field: MonitorField,
    ) -> Option<(Vec<f64>, Vec<f64>)> {
        let (_, rows) = self.series_rows(run, field)?;
        let at = match field {
            MonitorField::Velocity => 1,
            MonitorField::Pressure => 0,
        };
        rows.iter()
            .map(|(time, values)| Some((*time, *values.get(at)?)))
            .collect::<Option<Vec<_>>>()
            .map(|pairs| pairs.into_iter().unzip())
    }

    fn build(&self, entry: &Entry) -> Option<String> {
        match &entry.kind {
            Kind::Profile { run, origin } => self.profile(run, *origin),
            Kind::Series { run, field } => self.series(run, *field),
        }
    }
}

/// Every path the comparers read that the study's points can give.
fn entries_of(study: &ResolvedStudy, set: &ProbeSet) -> Vec<Entry> {
    let end = format!("{}", study.solver.end_time);
    let mut entries = Vec::new();
    for (run, file, origin) in [
        (CENTRELINE_VERTICAL, "vertical_U.xy", Origin::Y),
        (CENTRELINE_HORIZONTAL, "horizontal_U.xy", Origin::X),
    ] {
        if set.run(run).is_some() {
            entries.push(Entry {
                monitor: "centreline".to_owned(),
                folder: end.clone(),
                file: file.to_owned(),
                kind: Kind::Profile {
                    run: run.to_owned(),
                    origin,
                },
            });
        }
    }
    let plan = study.monitor_plan();
    for line in plan
        .lines
        .iter()
        .filter(|l| l.field == MonitorField::Velocity)
    {
        entries.push(Entry {
            monitor: line.fo.clone(),
            folder: end.clone(),
            file: "line_U.xy".to_owned(),
            kind: Kind::Profile {
                run: line.fo.clone(),
                origin: Origin::From(line.x0, line.y0),
            },
        });
    }
    for probe in &plan.probes {
        let file = match probe.field {
            MonitorField::Velocity => "U",
            MonitorField::Pressure => "p",
        };
        entries.push(Entry {
            monitor: probe.fo.clone(),
            folder: SERIES_FOLDER.to_owned(),
            file: file.to_owned(),
            kind: Kind::Series {
                run: probe.fo.clone(),
                field: probe.field,
            },
        });
    }
    entries
}

impl ResultFiles for ResultView<'_> {
    fn read(&self, rel: &str) -> Option<String> {
        match self.entries.iter().find(|e| e.path() == rel) {
            Some(entry) => self.build(entry),
            None => self.inner.read(rel),
        }
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        let dir = rel_dir.trim_matches('/');
        let mut names: BTreeSet<String> = self.inner.list(rel_dir).into_iter().collect();
        for entry in &self.entries {
            let path = entry.path();
            let rest = if dir.is_empty() {
                Some(path.as_str())
            } else {
                path.strip_prefix(dir).and_then(|r| r.strip_prefix('/'))
            };
            // An entry is listed only when it can be built, so a missing or cut file shows nothing.
            if let Some(next) = rest.and_then(|r| r.split('/').next())
                && self.build(entry).is_some()
            {
                names.insert(next.to_owned());
            }
        }
        names.into_iter().collect()
    }
}
