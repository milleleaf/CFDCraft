use std::{fmt::Write as _, path::Path};

use thiserror::Error;

use crate::{InterpError, Profile, linear_sample};

const CSV: &str = include_str!("../data/ghia_re100.csv");
const TOLERANCE: &str = include_str!("../data/ghia_re100.toml");

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("cannot read benchmark data {path}: {source}")]
    DataMissing {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid benchmark data {path}: {reason}")]
    DataCorrupt { path: String, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    pub u_max_abs: f64,
    pub v_max_abs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub station: f64,
    pub computed: f64,
    pub reference: f64,
    pub abs_dev: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub max_abs_dev: f64,
    pub tolerance: f64,
    pub passed: bool,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone)]
pub struct GhiaTable {
    pub u: Profile,
    pub v: Profile,
    pub tolerances: Tolerances,
}

fn corrupt(path: &str, reason: impl Into<String>) -> LoadError {
    LoadError::DataCorrupt {
        path: path.into(),
        reason: reason.into(),
    }
}

fn read(path: &Path) -> Result<String, LoadError> {
    std::fs::read_to_string(path).map_err(|source| LoadError::DataMissing {
        path: path.display().to_string(),
        source,
    })
}

impl Tolerances {
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        Self::parse(&read(path)?, &path.display().to_string())
    }

    fn parse(text: &str, path: &str) -> Result<Self, LoadError> {
        let doc: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| corrupt(path, e.to_string()))?;
        let table = doc
            .get("tolerance")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| corrupt(path, "missing [tolerance] table"))?;
        if table.get("units").and_then(toml::Value::as_str) != Some("lid_velocities") {
            return Err(corrupt(path, "tolerance units must be lid_velocities"));
        }
        if !table
            .get("justification")
            .and_then(toml::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Err(corrupt(path, "tolerance justification is required"));
        }
        let number = |name: &str| {
            let value = table
                .get(name)
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
                .ok_or_else(|| corrupt(path, format!("missing numeric {name}")))?;
            if !value.is_finite() || value <= 0.0 || value > 0.05 {
                return Err(corrupt(
                    path,
                    format!("{name} must be finite and in (0, 0.05]"),
                ));
            }
            Ok(value)
        };
        Ok(Self {
            u_max_abs: number("u_max_abs")?,
            v_max_abs: number("v_max_abs")?,
        })
    }
}

impl GhiaTable {
    /// Load the shipped reference and tolerance data embedded at build time.
    pub fn load() -> Result<Self, LoadError> {
        Self::from_text(CSV, TOLERANCE)
    }

    /// Load an explicit CSV and tolerance TOML, including test/CLI overrides.
    pub fn load_from(
        csv_path: impl AsRef<Path>,
        tolerance_path: impl AsRef<Path>,
    ) -> Result<Self, LoadError> {
        let csv_path = csv_path.as_ref();
        let tolerance_path = tolerance_path.as_ref();
        Self::parse(
            &read(csv_path)?,
            &csv_path.display().to_string(),
            &read(tolerance_path)?,
            &tolerance_path.display().to_string(),
        )
    }

    pub fn from_text(csv: &str, tolerance: &str) -> Result<Self, LoadError> {
        Self::parse(csv, "ghia_re100.csv", tolerance, "ghia_re100.toml")
    }

    /// Apply a runtime tolerance file while retaining the shipped reference.
    pub fn with_tolerance_file(mut self, path: impl AsRef<Path>) -> Result<Self, LoadError> {
        self.tolerances = Tolerances::load_from(path)?;
        Ok(self)
    }

    fn parse(
        csv: &str,
        csv_path: &str,
        tolerance: &str,
        tolerance_path: &str,
    ) -> Result<Self, LoadError> {
        if csv.starts_with('\u{feff}') {
            return Err(corrupt(csv_path, "UTF-8 BOM is not accepted"));
        }
        let mut rows = csv
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'));
        if rows.next() != Some("component,coordinate,velocity") {
            return Err(corrupt(
                csv_path,
                "expected component,coordinate,velocity CSV header",
            ));
        }
        let mut u = Vec::new();
        let mut v = Vec::new();
        for line in rows {
            let columns: Vec<_> = line.split(',').collect();
            if columns.len() != 3 {
                return Err(corrupt(csv_path, format!("expected three columns: {line}")));
            }
            let coordinate: f64 = columns[1]
                .parse()
                .map_err(|_| corrupt(csv_path, format!("invalid coordinate: {line}")))?;
            let velocity: f64 = columns[2]
                .parse()
                .map_err(|_| corrupt(csv_path, format!("invalid velocity: {line}")))?;
            if !coordinate.is_finite()
                || !velocity.is_finite()
                || !(0.0..=1.0).contains(&coordinate)
            {
                return Err(corrupt(
                    csv_path,
                    format!("non-finite or out-of-domain station: {line}"),
                ));
            }
            match columns[0] {
                "u" => u.push((coordinate, velocity)),
                "v" => v.push((coordinate, velocity)),
                _ => return Err(corrupt(csv_path, format!("unknown component: {line}"))),
            }
        }
        let profile = |mut points: Vec<(f64, f64)>| -> Result<Profile, LoadError> {
            if points.len() != 17 {
                return Err(corrupt(
                    csv_path,
                    format!("expected 17 stations per component, found {}", points.len()),
                ));
            }
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            let p = Profile {
                coords: points.iter().map(|x| x.0).collect(),
                values: points.iter().map(|x| x.1).collect(),
            };
            p.validate().map_err(|e| corrupt(csv_path, e.to_string()))?;
            if p.coords[0] != 0.0 || p.coords[16] != 1.0 {
                return Err(corrupt(
                    csv_path,
                    "reference must include both walls at 0 and 1",
                ));
            }
            Ok(p)
        };
        Ok(Self {
            u: profile(u)?,
            v: profile(v)?,
            tolerances: Tolerances::parse(tolerance, tolerance_path)?,
        })
    }

    pub fn compare_u(&self, computed: &Profile) -> Result<Comparison, InterpError> {
        compare(computed, &self.u, self.tolerances.u_max_abs)
    }

    pub fn compare_v(&self, computed: &Profile) -> Result<Comparison, InterpError> {
        compare(computed, &self.v, self.tolerances.v_max_abs)
    }
}

fn compare(
    computed: &Profile,
    reference: &Profile,
    tolerance: f64,
) -> Result<Comparison, InterpError> {
    computed.validate()?;
    if computed.coords.first() != Some(&0.0) || computed.coords.last() != Some(&1.0) {
        return Err(InterpError::MissingWalls);
    }
    let mut rows = Vec::with_capacity(reference.coords.len());
    let mut max_abs_dev: f64 = 0.0;
    for (&station, &value) in reference.coords.iter().zip(&reference.values) {
        let prediction = linear_sample(computed, station)?;
        let abs_dev = (prediction - value).abs();
        if !abs_dev.is_finite() {
            return Err(InterpError::NonFinite);
        }
        max_abs_dev = max_abs_dev.max(abs_dev);
        rows.push(Row {
            station,
            computed: prediction,
            reference: value,
            abs_dev,
        });
    }
    Ok(Comparison {
        max_abs_dev,
        tolerance,
        passed: max_abs_dev <= tolerance,
        rows,
    })
}

pub fn render_table(u: &Comparison, v: &Comparison) -> String {
    let mut out = String::new();
    for (component, comparison) in [("u", u), ("v", v)] {
        writeln!(
            out,
            "Ghia {component}: station | computed | reference | |dev|"
        )
        .expect("String write");
        for row in &comparison.rows {
            writeln!(
                out,
                "{:.4} | {:.7} | {:.7} | {:.7}",
                row.station, row.computed, row.reference, row.abs_dev
            )
            .expect("String write");
        }
        writeln!(
            out,
            "Ghia {component}: {} (max |dev| {:.7} {} {})",
            if comparison.passed { "PASS" } else { "FAIL" },
            comparison.max_abs_dev,
            if comparison.passed { "<=" } else { ">" },
            comparison.tolerance
        )
        .expect("String write");
    }
    out
}
