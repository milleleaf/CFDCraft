//! The differentially heated square cavity as a benchmark: which studies it applies to, and the
//! verdict for a finished run against Table V.

use cfdcraft_gate::{Benchmark, GateMetric, GateReport, Mismatch, ResultFiles};
use cfdcraft_model::ResolvedStudy;

use crate::data::{LoadError, Row, Table, Tolerances, judge_position, judge_relative};
use crate::metrics::{Setup, measure, measure_with};

const ID: &str = "dvd";

/// The benchmark with its published numbers and the allowances in force. `overridden` holds the
/// justification of a user's allowance file when one is in use, so the report can name it.
#[derive(Debug, Clone, PartialEq)]
pub struct DvdBenchmark {
    table: Table,
    tolerances: Tolerances,
    overridden: Option<String>,
}

impl DvdBenchmark {
    /// The benchmark with the shipped table and allowances.
    pub fn new() -> Result<Self, LoadError> {
        let table = Table::load()?;
        let tolerances = table.tolerances.clone();
        Ok(Self {
            table,
            tolerances,
            overridden: None,
        })
    }

    /// The benchmark with the allowances of a user file: bounded, justified, and named in every
    /// report.
    pub fn with_override(mut self, text: &str) -> Result<Self, LoadError> {
        let changed = Tolerances::from_toml(text, &self.table.tolerances)?;
        self.overridden = Some(changed.justification.clone());
        self.tolerances = changed;
        Ok(self)
    }

    /// The setup of the study and the column of Table V for its Rayleigh number.
    fn prepare(&self, study: &ResolvedStudy) -> Result<(Setup, &Row), Vec<Mismatch>> {
        let setup = Setup::from_study(study)?;
        match self.table.row_for_ra(setup.rayleigh) {
            Some(row) => Ok((setup, row)),
            None => Err(vec![Mismatch {
                what: "Rayleigh number".to_owned(),
                found: format!("{:.4e}", setup.rayleigh),
                expected: "1e3, 1e4, 1e5 or 1e6, within 0.5 %".to_owned(),
            }]),
        }
    }
}

fn with_unit(mut metric: GateMetric, unit: &str) -> GateMetric {
    metric.unit = Some(unit.to_owned());
    metric
}

impl Benchmark for DvdBenchmark {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Differentially heated square cavity"
    }

    fn reference(&self) -> &'static str {
        "de Vahl Davis (1983), Table V"
    }

    fn matches(&self, study: &ResolvedStudy) -> Result<(), Vec<Mismatch>> {
        self.prepare(study).map(|_| ())
    }

    fn compare(&self, files: &dyn ResultFiles, study: &ResolvedStudy) -> GateReport {
        let (setup, row) = match self.prepare(study) {
            Ok(found) => found,
            Err(list) => {
                let text: Vec<String> = list.iter().map(ToString::to_string).collect();
                return GateReport::failed_named(
                    ID,
                    format!(
                        "CFD-BENCH-001 the study is not the heated cavity: {}",
                        text.join("; ")
                    ),
                );
            }
        };
        let measured = match measure_with(files, &setup) {
            Ok(measured) => measured,
            Err(refusal) => return GateReport::failed_named(ID, refusal.to_string()),
        };
        let t = &self.tolerances;
        let nu = t.nu_allowance(row);
        let velocity = t.velocity_allowance(row);
        let metrics = vec![
            judge_relative("Nu", measured.nu_hot, row.nu_avg, nu),
            with_unit(
                judge_relative("u_max", measured.u_max, row.u_max, velocity),
                "kappa/L",
            ),
            with_unit(
                judge_position("u_max z", measured.u_max_y, row.u_max_z, t.position_abs),
                "L",
            ),
            with_unit(
                judge_relative("w_max", measured.w_max, row.w_max, velocity),
                "kappa/L",
            ),
            with_unit(
                judge_position("w_max x", measured.w_max_x, row.w_max_x, t.position_abs),
                "L",
            ),
        ];
        let mut report = GateReport::from_metrics(ID, metrics);
        report.notes.push(format!(
            "Rayleigh number {:.0}, Prandtl number {}, Table V of {}",
            setup.rayleigh, setup.prandtl, self.table.source.doi
        ));
        report.notes.push(format!(
            "energy imbalance between the hot and cold wall flows: {:.4}",
            measured.energy_imbalance
        ));
        report.notes.push(format!(
            "reference accuracy: {}",
            self.table.source.accuracy
        ));
        if let Some(why) = &self.overridden {
            report.note_override(why);
        }
        report
    }

    fn quantities(&self, files: &dyn ResultFiles, study: &ResolvedStudy) -> Vec<(String, f64)> {
        match measure(files, study) {
            Ok(m) => vec![
                ("bench.dvd.nu_hot".to_owned(), m.nu_hot),
                ("bench.dvd.u_max".to_owned(), m.u_max),
                ("bench.dvd.w_max".to_owned(), m.w_max),
                ("bench.dvd.u_max_y".to_owned(), m.u_max_y),
                ("bench.dvd.w_max_x".to_owned(), m.w_max_x),
                ("bench.dvd.energy_imbalance".to_owned(), m.energy_imbalance),
            ],
            Err(_) => Vec::new(),
        }
    }
}
