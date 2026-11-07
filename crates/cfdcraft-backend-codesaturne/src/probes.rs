//! The monitoring points of a case.
//!
//! The solver has no line sampler, so every profile a benchmark compares is a run of monitoring
//! points along a line. A point is placed exactly where it is asked and the solver is told to
//! interpolate to it, so the value is the field's value there and not that of the nearest cell
//! centre. The set is public: the reader of a finished run finds each profile by the run of
//! points that belongs to it.
//!
//! A box carries two centre lines, as the cavity benchmark reads them, with a point at every
//! node of the mesh along the line. Every other line and point is the study's own, in the
//! numbering the study's monitor plan gives it (`line_1`, `probe_1`).

use crate::{
    mesh::layout_of,
    models::Run,
    setup::{Writer, invalid, number, unsupported},
};
use cfdcraft_backend::CompileFailure;
use cfdcraft_model::ResolvedStudy;
use cfdcraft_shapes::edges;

/// The run of points along the vertical centre line of a box, `x` half way.
pub const CENTRELINE_VERTICAL: &str = "centreline_vertical";

/// The run of points along the horizontal centre line of a box, `y` half way.
pub const CENTRELINE_HORIZONTAL: &str = "centreline_horizontal";

/// The most monitoring points a case may hold. The setup is read back as text with a size limit,
/// and every point is a line of text.
pub const MAX_POINTS: usize = 20_000;

/// One monitoring point, in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbePoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// The points of one profile, in order along it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeRun {
    /// `centreline_vertical`, `centreline_horizontal`, or the monitor plan's `line_<n>` or
    /// `probe_<n>`.
    pub name: String,
    /// The index of the first point of the run in [`ProbeSet::points`].
    pub first: usize,
    /// The number of points in the run.
    pub count: usize,
}

/// Every monitoring point of a case. The point at index `i` is probe number `i + 1` in the setup.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProbeSet {
    pub points: Vec<ProbePoint>,
    pub runs: Vec<ProbeRun>,
}

impl ProbeSet {
    fn push_run(&mut self, name: &str, points: Vec<ProbePoint>) {
        self.runs.push(ProbeRun {
            name: name.to_owned(),
            first: self.points.len(),
            count: points.len(),
        });
        self.points.extend(points);
    }

    /// The run called `name`.
    pub fn run(&self, name: &str) -> Option<&ProbeRun> {
        self.runs.iter().find(|r| r.name == name)
    }

    /// The points of the run called `name`.
    pub fn points_of(&self, name: &str) -> Option<&[ProbePoint]> {
        let run = self.run(name)?;
        self.points.get(run.first..run.first + run.count)
    }
}

/// The nodes of the mesh along the two axes of a box, measured from the corner.
fn box_nodes(study: &ResolvedStudy) -> Result<(Vec<f64>, Vec<f64>), CompileFailure> {
    let layout = layout_of(study)?;
    let [block] = layout.blocks.as_slice() else {
        return Err(invalid("a box is laid out as one block"));
    };
    let xs = edges(&block.grading[0], block.cells[0], study.domain.length_x);
    let ys = edges(&block.grading[1], block.cells[1], study.domain.length_y);
    Ok((xs, ys))
}

/// A line of `samples` points evenly spaced from one end to the other, ends included.
fn evenly(
    from: (f64, f64),
    to: (f64, f64),
    samples: u64,
    z: f64,
) -> Result<Vec<ProbePoint>, CompileFailure> {
    if samples < 2 {
        return Err(invalid("a line of monitoring points needs two samples"));
    }
    let last = (samples - 1) as f64;
    Ok((0..samples)
        .map(|i| {
            let t = i as f64 / last;
            ProbePoint {
                x: from.0 + (to.0 - from.0) * t,
                y: from.1 + (to.1 - from.1) * t,
                z,
            }
        })
        .collect())
}

/// The monitoring points of a study.
pub fn probe_set(study: &ResolvedStudy) -> Result<ProbeSet, CompileFailure> {
    let monitors = &study.monitors;
    if !monitors.surfaces.is_empty() || !monitors.forces.is_empty() {
        return Err(unsupported(
            "monitors",
            "surfaces and force coefficients are not written for this solver",
        ));
    }
    let z = study.domain.length_z / 2.0;
    let mut set = ProbeSet::default();
    if study.domain.shape.is_none() {
        let (xs, ys) = box_nodes(study)?;
        let (cx, cy) = (study.domain.length_x / 2.0, study.domain.length_y / 2.0);
        set.push_run(
            CENTRELINE_VERTICAL,
            ys.iter().map(|y| ProbePoint { x: cx, y: *y, z }).collect(),
        );
        set.push_run(
            CENTRELINE_HORIZONTAL,
            xs.iter().map(|x| ProbePoint { x: *x, y: cy, z }).collect(),
        );
    }
    let plan = study.monitor_plan();
    for line in &plan.lines {
        let points = evenly((line.x0, line.y0), (line.x1, line.y1), line.samples, z)?;
        set.push_run(&line.fo, points);
    }
    for probe in &plan.probes {
        if probe.boundary.is_some() {
            return Err(unsupported(
                "probe on a boundary",
                "a monitoring point on a boundary is not written for this solver",
            ));
        }
        set.push_run(
            &probe.fo,
            vec![ProbePoint {
                x: probe.x,
                y: probe.y,
                z,
            }],
        );
    }
    if set.points.len() > MAX_POINTS {
        return Err(invalid(&format!(
            "the case has {} monitoring points, above the limit of {MAX_POINTS}",
            set.points.len()
        )));
    }
    let finite = set
        .points
        .iter()
        .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
    if !finite {
        return Err(invalid("a monitoring point is not a finite position"));
    }
    Ok(set)
}

/// How many iterations pass between two records: the write interval of the study counted in
/// iterations. The run must end on a record, so the last values are the ones kept.
pub fn recording_frequency(study: &ResolvedStudy, run: &Run) -> Result<u64, CompileFailure> {
    let count = study.solver.write_interval / run.dt.unwrap_or(1.0);
    let nearest = count.round();
    let whole = count.is_finite() && nearest >= 1.0 && (count - nearest).abs() <= 1e-9 * nearest;
    if !whole {
        return Err(invalid(
            "the write interval must be a whole number of iterations",
        ));
    }
    let frequency = nearest as u64;
    if frequency > run.iterations || !run.iterations.is_multiple_of(frequency) {
        return Err(invalid(
            "the run must end on a write interval, so the last values are recorded",
        ));
    }
    Ok(frequency)
}

/// The fields every monitoring point records.
pub(crate) fn write_recorded_variables(w: &mut Writer) {
    w.open("velocity_pressure", &[]);
    for (name, label, dimension) in [("velocity", "Velocity", "3"), ("pressure", "Pressure", "1")] {
        w.open(
            "variable",
            &[("dimension", dimension), ("label", label), ("name", name)],
        );
        w.empty("probes_recording", &[("status", "on")]);
        w.close("variable");
    }
    w.close("velocity_pressure");
}

/// The output node: one entry per point, how often they are recorded, and how.
pub(crate) fn write_output(
    w: &mut Writer,
    set: &ProbeSet,
    frequency: u64,
) -> Result<(), CompileFailure> {
    w.open("output", &[]);
    for (index, point) in set.points.iter().enumerate() {
        let id = (index + 1).to_string();
        w.open(
            "probe",
            &[("id", id.as_str()), ("name", id.as_str()), ("status", "on")],
        );
        w.leaf("probe_x", &[], &number(point.x)?);
        w.leaf("probe_y", &[], &number(point.y)?);
        w.leaf("probe_z", &[], &number(point.z)?);
        w.close("probe");
    }
    w.leaf("probe_recording_frequency", &[], &frequency.to_string());
    w.empty("probe_format", &[("choice", "CSV")]);
    w.empty("probes_snap", &[("choice", "none")]);
    w.empty("probes_interpolation", &[("choice", "yes")]);
    w.close("output");
    Ok(())
}
