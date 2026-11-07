//! The turbulent backward-facing step against the experiment of Driver and Seegmiller.

use cfdcraft_gate::{
    Benchmark, GateMetric, GateReport, GateRow, MetricKind, Mismatch, ResultFiles,
};
use cfdcraft_model::{InletProfile, ResolvedStudy};

use crate::data::{DsProfiles, DsReference, LoadError, STATIONS, Station, Tolerances};
use crate::lines::{
    FLOOR, floor_line_mismatch, inlet, interpolate, line_samples, mismatch, near, nominal_scalar,
    step_geometry,
};
use crate::reattach::reattachment_length;

/// The id of the benchmark.
pub const ID: &str = "step";
/// How far a profile line may lie from its station, in step heights.
const STATION_TOLERANCE: f64 = 0.05;

/// The label of the vertical line at a station.
fn profile_label(x_over_h: f64) -> String {
    format!("profile_x{x_over_h}")
}

/// The comparison of a turbulent step run with the published reattachment length and profiles.
#[derive(Debug, Clone)]
pub struct StepBenchmark {
    reference: DsReference,
    profiles: DsProfiles,
    tolerances: Tolerances,
    override_note: Option<String>,
}

impl StepBenchmark {
    /// The benchmark with the shipped reference and tolerances.
    pub fn new() -> Result<Self, LoadError> {
        let reference = DsReference::load()?;
        let tolerances = reference.tolerances();
        Ok(Self {
            reference,
            profiles: DsProfiles::load()?,
            tolerances,
            override_note: None,
        })
    }

    /// The same benchmark with tolerances a user chose; the report names the override and its
    /// justification.
    pub fn with_tolerances(mut self, tolerances: Tolerances, justification: &str) -> Self {
        self.tolerances = tolerances;
        self.override_note = Some(justification.to_owned());
        self
    }

    /// The profile at one station: the study's velocity along the vertical line, interpolated at
    /// the published heights.
    fn profile_metric(
        &self,
        files: &dyn ResultFiles,
        r: &ResolvedStudy,
        (h, uref): (f64, f64),
        station: &Station,
    ) -> Result<GateMetric, String> {
        let label = profile_label(station.x_over_h);
        let study: Vec<(f64, f64)> = line_samples(files, r, &label, 1)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(y, u)| (y / h, u / uref))
            .collect();
        let rows: Vec<GateRow> = station
            .y_over_h
            .iter()
            .zip(&station.u_over_uref)
            .map(|(&y, &reference)| {
                let computed = interpolate(&study, y);
                GateRow {
                    station: y,
                    computed,
                    reference,
                    abs_dev: (computed - reference).abs(),
                }
            })
            .collect();
        let worst = rows.iter().map(|row| row.abs_dev).fold(0.0, f64::max);
        let tolerance = self.tolerances.profile_u_max_abs;
        Ok(GateMetric {
            name: format!("u/Uref x/H={}", station.x_over_h),
            max_abs_dev: worst,
            tolerance,
            passed: worst <= tolerance,
            rows,
            kind: MetricKind::Profile,
            reference_range: None,
            unit: Some("Uref".to_owned()),
            gating: true,
        })
    }

    fn judge(&self, files: &dyn ResultFiles, r: &ResolvedStudy) -> Result<GateReport, String> {
        let (h, step_x) = step_geometry(r).ok_or("the study has no backward step shape")?;
        let uref = inlet(r)
            .and_then(|b| b.velocity)
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or("the study has no velocity inlet with a speed")?;
        let floor = line_samples(files, r, FLOOR, 0).map_err(|e| e.to_string())?;
        let length = reattachment_length(&floor, step_x).ok_or(
            "the streamwise velocity along the floor line never changes from negative to positive",
        )?;
        let mut metrics = vec![nominal_scalar(
            "x_R/H",
            length / h,
            self.reference.reattachment.x_over_h,
            self.reference.reattachment.uncertainty,
            self.tolerances.x_over_h,
            "step heights",
        )];
        for station in &self.profiles.stations {
            metrics.push(self.profile_metric(files, r, (h, uref), station)?);
        }
        let mut report = GateReport::from_metrics(ID, metrics);
        if let Some(note) = &self.override_note {
            report.note_override(note);
        }
        Ok(report)
    }
}

impl Benchmark for StepBenchmark {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Turbulent backward-facing step, Driver and Seegmiller"
    }

    fn reference(&self) -> &'static str {
        "Driver and Seegmiller (1985), AIAA J. 23(2), 163-171, DOI 10.2514/3.8890; NASA Turbulence Modeling Resource, 2D backward-facing step"
    }

    /// The step of the reference: the step height, the expansion ratio and the Reynolds number
    /// within 2 %, a uniform inlet profile, and the five lines the comparison reads. The
    /// turbulence model and steady or transient time do not matter.
    fn matches(&self, r: &ResolvedStudy) -> Result<(), Vec<Mismatch>> {
        let Some((h, step_x)) = step_geometry(r) else {
            return Err(vec![mismatch(
                "shape",
                match r.domain.shape {
                    None => "a box",
                    Some(_) => "another shape",
                },
                "a backward step",
            )]);
        };
        let g = &self.reference.geometry;
        let mut out = Vec::new();
        if !near(h, g.step_height_m) {
            out.push(mismatch(
                "step height",
                format!("{:.2} mm", h * 1000.0),
                format!("{:.2} mm", g.step_height_m * 1000.0),
            ));
        }
        let ratio = r.domain.length_y / (r.domain.length_y - h);
        if !near(ratio, g.expansion_ratio) {
            out.push(mismatch(
                "expansion ratio",
                format!("{ratio:.3}"),
                format!("{:.3}", g.expansion_ratio),
            ));
        }
        match inlet(r) {
            None => out.push(mismatch("velocity inlet", "none", "one velocity inlet")),
            Some(boundary) => {
                let speed = boundary.velocity.unwrap_or(f64::NAN);
                let re_h = speed * h / r.fluid.kinematic_viscosity;
                if !near(re_h, g.re_h) {
                    out.push(mismatch(
                        "Reynolds number on the step height",
                        format!("{re_h:.0}"),
                        format!("{:.0}", g.re_h),
                    ));
                }
                if boundary.profile == Some(InletProfile::Parabolic) {
                    out.push(mismatch("inlet profile", "parabolic", "uniform"));
                }
            }
        }
        out.extend(floor_line_mismatch(
            r,
            h,
            self.tolerances.floor_offset_over_h,
        ));
        for station in STATIONS {
            let label = profile_label(station);
            match r.monitors.lines.iter().find(|l| l.label == label) {
                None => out.push(mismatch(
                    &format!("line {label}"),
                    "missing",
                    "a sampled line of velocity",
                )),
                Some(l) => {
                    let from_step = (l.x0 - step_x) / h;
                    if (from_step - station).abs() > STATION_TOLERANCE
                        || (l.x1 - l.x0).abs() > 1e-9 * h
                    {
                        out.push(mismatch(
                            &format!("line {label} position"),
                            format!("{from_step:.2} step heights from the step"),
                            format!("{station} step heights from the step, vertical"),
                        ));
                    }
                }
            }
        }
        if out.is_empty() { Ok(()) } else { Err(out) }
    }

    fn compare(&self, files: &dyn ResultFiles, r: &ResolvedStudy) -> GateReport {
        self.judge(files, r)
            .unwrap_or_else(|reason| GateReport::failed_named(ID, reason))
    }

    fn quantities(&self, files: &dyn ResultFiles, r: &ResolvedStudy) -> Vec<(String, f64)> {
        let Some((h, step_x)) = step_geometry(r) else {
            return Vec::new();
        };
        line_samples(files, r, FLOOR, 0)
            .ok()
            .and_then(|floor| reattachment_length(&floor, step_x))
            .map(|length| vec![("bench.step.reattachment".to_owned(), length / h)])
            .unwrap_or_default()
    }
}
