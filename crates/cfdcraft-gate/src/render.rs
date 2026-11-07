//! The fixed-format text of a report: the computed value, the published range, the tolerance
//! that was used and the verdict, side by side.
//!
//! Number formats are part of the format and do not change: four decimals for a scalar value, its
//! range and its tolerance and for stations; six decimals for the values and deviations of a
//! profile.

use crate::report::{GateMetric, GateReport, MetricKind};

fn verdict(metric: &GateMetric) -> &'static str {
    match (metric.gating, metric.passed) {
        (false, _) => "info",
        (true, true) => "PASS",
        (true, false) => "FAIL",
    }
}

fn scalar_line(metric: &GateMetric) -> String {
    let computed = metric
        .rows
        .first()
        .map_or_else(|| "-".to_owned(), |r| format!("{:.4}", r.computed));
    let range = metric.reference_range.map_or_else(
        || "-".to_owned(),
        |[low, high]| format!("{low:.4} to {high:.4}"),
    );
    format!(
        "{}  {}  {}  {:.4}  {}  {}\n",
        metric.name,
        computed,
        range,
        metric.tolerance,
        metric.unit.as_deref().unwrap_or("-"),
        verdict(metric)
    )
}

fn profile_block(metric: &GateMetric) -> String {
    // The worst station is the last one among equal deviations.
    let worst = metric
        .rows
        .iter()
        .max_by(|a, b| a.abs_dev.total_cmp(&b.abs_dev))
        .map_or_else(|| "-".to_owned(), |r| format!("{:.4}", r.station));
    let mut out = format!(
        "{}  max deviation {:.6}  tolerance {:.6}  worst station {}  {}\n",
        metric.name,
        metric.max_abs_dev,
        metric.tolerance,
        worst,
        verdict(metric)
    );
    out.push_str(&format!(
        "    {:>8}  {:>10}  {:>10}  {:>10}\n",
        "station", "computed", "reference", "deviation"
    ));
    for row in &metric.rows {
        out.push_str(&format!(
            "    {:>8.4}  {:>10.6}  {:>10.6}  {:>10.6}\n",
            row.station, row.computed, row.reference, row.abs_dev
        ));
    }
    out
}

/// The report as text: a verdict line, one block per metric, then the notes. A report that failed
/// without a comparison prints its reason and nothing else.
pub fn render_report(report: &GateReport) -> String {
    let word = if report.passed { "PASS" } else { "FAILED" };
    let mut out = format!("{} verdict: {word}\n", report.name);
    if let Some(reason) = &report.reason {
        out.push_str(&format!("  {reason}\n"));
        return out;
    }
    for metric in &report.metrics {
        out.push_str(&match metric.kind {
            MetricKind::Scalar => scalar_line(metric),
            MetricKind::Profile => profile_block(metric),
        });
    }
    for note in &report.notes {
        out.push_str(note);
        out.push('\n');
    }
    out
}
