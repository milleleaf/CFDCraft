//! The laminar backward-facing step against the measurements of Armaly and others, Re 100 to 400.

use cfdcraft_gate::{Benchmark, GateReport, Mismatch, ResultFiles};
use cfdcraft_model::{InletProfile, ResolvedStudy, ResolvedTime, TurbulenceModel};

use crate::data::{ArmalyCurve, ArmalyReference, LoadError};
use crate::lines::{
    FLOOR, floor_line_mismatch, inlet, line_samples, mismatch, near, nominal_scalar, step_geometry,
};
use crate::reattach::reattachment_length;

/// The id of the benchmark.
pub const ID: &str = "armaly";
/// The height of the floor line above the floor, in step heights.
const FLOOR_OFFSET: f64 = 0.01;

/// The Reynolds number of the experiment, `U 2h / nu`: the mean inlet speed, twice the height of
/// the inlet channel (the domain height less the step) and the kinematic viscosity. `None` when the
/// study has no step or no inlet speed, or the number is not finite and above zero.
pub fn reynolds_number(r: &ResolvedStudy) -> Option<f64> {
    let (step, _) = step_geometry(r)?;
    let speed = inlet(r)?.velocity?;
    let re = speed * 2.0 * (r.domain.length_y - step) / r.fluid.kinematic_viscosity;
    (re.is_finite() && re > 0.0).then_some(re)
}

/// The comparison of a laminar step run with the measured reattachment length.
#[derive(Debug, Clone)]
pub struct ArmalyBenchmark {
    reference: ArmalyReference,
    curve: ArmalyCurve,
}

impl ArmalyBenchmark {
    /// The benchmark with the shipped reference and curve.
    pub fn new() -> Result<Self, LoadError> {
        Ok(Self {
            reference: ArmalyReference::load()?,
            curve: ArmalyCurve::load()?,
        })
    }

    fn judge(&self, files: &dyn ResultFiles, r: &ResolvedStudy) -> Result<GateReport, String> {
        let (step, step_x) = step_geometry(r).ok_or("the study has no backward step shape")?;
        let re = reynolds_number(r).ok_or("the study has no usable Reynolds number")?;
        let published = self.curve.reference_at(re).ok_or_else(|| {
            format!("the Reynolds number {re:.0} is outside the measured range 100 to 400")
        })?;
        let floor = line_samples(files, r, FLOOR, 0).map_err(|e| e.to_string())?;
        let length = reattachment_length(&floor, step_x).ok_or(
            "the streamwise velocity along the floor line never changes from negative to positive",
        )?;
        let t = &self.reference.tolerance;
        let metric = nominal_scalar(
            "x1/S",
            length / step,
            published,
            t.reading_uncertainty_s,
            t.reading_uncertainty_s + t.relative_allowance * published,
            "step heights",
        );
        Ok(GateReport::from_metrics(ID, vec![metric]))
    }
}

impl Benchmark for ArmalyBenchmark {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Laminar backward-facing step, Armaly et al."
    }

    fn reference(&self) -> &'static str {
        "Armaly, Durst, Pereira and Schonung (1983), J. Fluid Mech. 127, 473-496, DOI 10.1017/S0022112083002839, Fig. 4"
    }

    /// The experiment: Reynolds number 100 to 400, the expansion ratio within 2 %, a parabolic
    /// inlet profile, steady laminar flow without buoyancy, and the floor line.
    fn matches(&self, r: &ResolvedStudy) -> Result<(), Vec<Mismatch>> {
        let Some((step, _)) = step_geometry(r) else {
            return Err(vec![mismatch(
                "shape",
                match r.domain.shape {
                    None => "a box",
                    Some(_) => "another shape",
                },
                "a backward step",
            )]);
        };
        let mut out = Vec::new();
        match reynolds_number(r) {
            Some(re) if self.curve.reference_at(re).is_some() => {}
            found => out.push(mismatch(
                "Reynolds number",
                found.map_or_else(|| "none".to_owned(), |re| format!("{re:.0}")),
                "100 to 400",
            )),
        }
        let g = &self.reference.geometry;
        let ratio = r.domain.length_y / (r.domain.length_y - step);
        if !near(ratio, g.expansion_ratio) {
            out.push(mismatch(
                "expansion ratio",
                format!("{ratio:.3}"),
                format!("{:.2}", g.expansion_ratio),
            ));
        }
        let profile = inlet(r).and_then(|b| b.profile);
        if profile != Some(InletProfile::Parabolic) {
            out.push(mismatch(
                "inlet profile",
                if profile.is_some() { "uniform" } else { "none" },
                "parabolic",
            ));
        }
        if let Some(p) = &r.physics {
            if p.turbulence.model != TurbulenceModel::Laminar {
                out.push(mismatch(
                    "turbulence model",
                    p.turbulence.model.word(),
                    "laminar",
                ));
            }
            if !matches!(p.time, ResolvedTime::Steady) {
                out.push(mismatch("time", "transient", "steady"));
            }
            if p.buoyancy.is_some() {
                out.push(mismatch("buoyancy", "on", "off"));
            }
        }
        out.extend(floor_line_mismatch(r, step, FLOOR_OFFSET));
        if out.is_empty() { Ok(()) } else { Err(out) }
    }

    fn compare(&self, files: &dyn ResultFiles, r: &ResolvedStudy) -> GateReport {
        self.judge(files, r)
            .unwrap_or_else(|reason| GateReport::failed_named(ID, reason))
    }

    fn quantities(&self, files: &dyn ResultFiles, r: &ResolvedStudy) -> Vec<(String, f64)> {
        let Some((step, step_x)) = step_geometry(r) else {
            return Vec::new();
        };
        line_samples(files, r, FLOOR, 0)
            .ok()
            .and_then(|floor| reattachment_length(&floor, step_x))
            .map(|length| vec![("bench.armaly.reattachment".to_owned(), length / step)])
            .unwrap_or_default()
    }
}
