//! The report types shared by every benchmark comparison.

use crate::render::render_report;
use cfdcraft_bench_ghia::{Comparison, Row, render_table};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The name of the cavity comparison, whose text is the reference crate's own table.
pub const GHIA: &str = "ghia";

fn yes() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateRow {
    pub station: f64,
    pub computed: f64,
    pub reference: f64,
    pub abs_dev: f64,
}

/// A profile is compared station by station; a scalar is one number against a published range.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    #[default]
    Profile,
    Scalar,
}

impl MetricKind {
    fn is_profile(&self) -> bool {
        *self == Self::Profile
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateMetric {
    /// `u` for the vertical centreline, `v` for the horizontal one, or the name of a quantity.
    pub name: String,
    /// The largest deviation. For a scalar it is the distance outside the published range.
    pub max_abs_dev: f64,
    pub tolerance: f64,
    pub passed: bool,
    /// A profile has one row per station. A scalar has one row: station 0, the computed value,
    /// the middle of the published range and the distance from that middle.
    pub rows: Vec<GateRow>,
    #[serde(default, skip_serializing_if = "MetricKind::is_profile")]
    pub kind: MetricKind,
    /// The published low and high value of a scalar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_range: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// A metric that is not gating is shown but does not decide the verdict.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub gating: bool,
}

/// Whether the run a comparison was made on had converged, by its own convergence assessment. It
/// is a flag beside the verdict and never changes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GateConvergence {
    /// True only when the assessment recorded for the run says it converged. A run with no
    /// assessment, or one the assessment could not judge, is not converged here: `reason` says
    /// `unknown`.
    pub converged: bool,
    /// `converged`; the stable words of the criteria the assessment found unmet (for example
    /// `not_settled:floor, residuals_above_limit`); or `unknown`, with the cause after a colon.
    pub reason: String,
    /// The sentence a person reads when the run is not converged or its state is unknown; empty
    /// for a converged run.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GateReport {
    /// The benchmark, for example `ghia`.
    pub name: String,
    pub passed: bool,
    pub metrics: Vec<GateMetric>,
    /// Why the gate failed without a comparison: missing or corrupt sampled output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Lines shown after the table, such as a tolerance override that was used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Whether the run the comparison was made on had converged; present only for a recorded run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convergence: Option<GateConvergence>,
}

impl GateMetric {
    pub fn from_comparison(name: &str, c: &Comparison) -> Self {
        Self {
            name: name.to_owned(),
            max_abs_dev: c.max_abs_dev,
            tolerance: c.tolerance,
            passed: c.passed,
            rows: c
                .rows
                .iter()
                .map(|r| GateRow {
                    station: r.station,
                    computed: r.computed,
                    reference: r.reference,
                    abs_dev: r.abs_dev,
                })
                .collect(),
            kind: MetricKind::Profile,
            reference_range: None,
            unit: None,
            gating: true,
        }
    }

    /// One number against a published range. It passes when it lies inside the range widened by
    /// `tolerance` on both sides. A value that is not finite never passes; callers refuse such a
    /// value before they get here.
    pub fn scalar(
        name: &str,
        computed: f64,
        range: [f64; 2],
        tolerance: f64,
        unit: Option<&str>,
    ) -> Self {
        let finite = computed.is_finite();
        let outside = if finite {
            (range[0] - computed).max(computed - range[1]).max(0.0)
        } else {
            f64::MAX
        };
        let middle = (range[0] + range[1]) / 2.0;
        Self {
            name: name.to_owned(),
            max_abs_dev: outside,
            tolerance,
            passed: finite && outside <= tolerance,
            rows: vec![GateRow {
                station: 0.0,
                computed,
                reference: middle,
                abs_dev: (computed - middle).abs(),
            }],
            kind: MetricKind::Scalar,
            reference_range: Some(range),
            unit: unit.map(str::to_owned),
            gating: true,
        }
    }

    /// The reference crate's own value, for rendering.
    pub fn to_comparison(&self) -> Comparison {
        Comparison {
            max_abs_dev: self.max_abs_dev,
            tolerance: self.tolerance,
            passed: self.passed,
            rows: self
                .rows
                .iter()
                .map(|r| Row {
                    station: r.station,
                    computed: r.computed,
                    reference: r.reference,
                    abs_dev: r.abs_dev,
                })
                .collect(),
        }
    }
}

impl GateReport {
    /// A failed gate that never got as far as a comparison.
    pub fn failed_named(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: false,
            metrics: Vec::new(),
            reason: Some(reason.into()),
            notes: Vec::new(),
            convergence: None,
        }
    }

    /// A report whose verdict is that every gating metric passed. A report with no gating
    /// metric does not pass: nothing was checked.
    pub fn from_metrics(name: impl Into<String>, metrics: Vec<GateMetric>) -> Self {
        let mut gating = metrics.iter().filter(|m| m.gating).peekable();
        let passed = gating.peek().is_some() && gating.all(|m| m.passed);
        Self {
            name: name.into(),
            passed,
            metrics,
            reason: None,
            notes: Vec::new(),
            convergence: None,
        }
    }

    /// Name a tolerance override that was used, so the report shows it beside the numbers.
    pub fn note_override(&mut self, text: &str) {
        self.notes.push(format!("tolerance override used: {text}"));
    }

    /// The reference crate's table for the cavity, the shared table for everything else.
    pub fn render_text(&self) -> String {
        if self.name != GHIA {
            return render_report(self);
        }
        if let Some(reason) = &self.reason {
            return format!("Ghia verdict: FAILED\n  {reason}\n");
        }
        let find = |name: &str| self.metrics.iter().find(|m| m.name == name);
        match (find("u"), find("v")) {
            (Some(u), Some(v)) => render_table(&u.to_comparison(), &v.to_comparison()),
            _ => "Ghia verdict: FAILED\n  the comparison is incomplete\n".to_owned(),
        }
    }
}
