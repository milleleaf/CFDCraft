//! The benchmarks a study can be checked against. Each one is a [`Benchmark`]: it says whether a
//! study is that benchmark, compares the files of a finished run with the published values and
//! names the numbers it records. This module holds the list, the choice of a benchmark by what
//! the study is, and the file access the comparisons read through.

use crate::{
    documents::{finding, say},
    ghia,
};
use cfdcraft_bench_cylinder::{CylinderBenchmark, Tolerances as CylinderTolerances};
use cfdcraft_bench_dvd::DvdBenchmark;
use cfdcraft_bench_ghia::{GhiaTable, linear_sample};
use cfdcraft_bench_step::{ArmalyBenchmark, StepBenchmark, Tolerances as StepTolerances};
use cfdcraft_capture::{entry_names, read_entry};
use cfdcraft_gate::{Benchmark, GateReport, Mismatch, ResultFiles};
use cfdcraft_model::{
    Diagnostic, InitMode, ResolvedStudy, ResolvedTime, Severity, TurbulenceModel,
};
use cfdcraft_text::Line;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// The largest file a comparison reads.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// The largest tolerance file that is read.
const MAX_OVERRIDE_BYTES: u64 = 64 * 1024;

/// What a report says about the tolerances of a benchmark whose settings come from a file.
pub(crate) const OVERRIDE_NOTE: &str = "the tolerances come from the file given with the check";

/// The files of a results folder on disk. It follows no link, reads only regular files up to
/// [`MAX_FILE_BYTES`] and refuses an absolute path or a `..` part, so a folder that came from a
/// compute host cannot lead a comparison outside itself.
#[derive(Debug, Clone)]
pub struct DirFiles {
    pub root: PathBuf,
}

impl DirFiles {
    /// The path below the root, or `None` when `rel` is not a plain relative path or any part of
    /// it is a link or missing.
    fn resolve(&self, rel: &str) -> Option<PathBuf> {
        let mut path = self.root.clone();
        for part in Path::new(rel).components() {
            match part {
                Component::Normal(name) => path.push(name),
                Component::CurDir => continue,
                _ => return None,
            }
            let meta = std::fs::symlink_metadata(&path).ok()?;
            if meta.file_type().is_symlink() {
                return None;
            }
        }
        Some(path)
    }
}

impl ResultFiles for DirFiles {
    fn read(&self, rel: &str) -> Option<String> {
        let path = self.resolve(rel)?;
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        let Some(path) = self.resolve(rel_dir) else {
            return Vec::new();
        };
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            return Vec::new();
        }
        let mut names: Vec<String> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file() || t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

/// The path a name stands for when it is plain: its normal parts joined by `/`, `""` for the
/// root itself. `None` for an absolute name, a drive prefix or a `..` part.
fn plain_name(rel: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in Path::new(rel).components() {
        match part {
            Component::Normal(name) => parts.push(name.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// The files of a stored result: the bytes of the archive a finished run was captured into. It
/// reads entries by name from memory and touches no disk, and refuses an absolute path or a `..`
/// part like [`DirFiles`]. An archive that cannot be read has no files.
#[derive(Debug, Clone)]
pub struct BundleFiles {
    bytes: Vec<u8>,
}

impl BundleFiles {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl ResultFiles for BundleFiles {
    fn read(&self, rel: &str) -> Option<String> {
        let name = plain_name(rel).filter(|name| !name.is_empty())?;
        let bytes = read_entry(&self.bytes, &name).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        let Some(dir) = plain_name(rel_dir) else {
            return Vec::new();
        };
        let prefix = if dir.is_empty() {
            dir
        } else {
            format!("{dir}/")
        };
        let mut names: Vec<String> = entry_names(&self.bytes)
            .unwrap_or_default()
            .iter()
            .filter_map(|name| name.strip_prefix(prefix.as_str()))
            .filter_map(|rest| rest.split('/').next())
            .filter(|first| !matches!(*first, "" | "." | ".."))
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

fn mismatch(what: &str, found: impl Into<String>, expected: &str) -> Mismatch {
    Mismatch {
        what: what.to_owned(),
        found: found.into(),
        expected: expected.to_owned(),
    }
}

/// A number for a sentence: six decimals at most, no trailing zeros.
fn num(x: f64) -> String {
    let text = format!("{x:.6}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

fn turbulence_name(model: TurbulenceModel) -> &'static str {
    match model {
        TurbulenceModel::Laminar => "laminar",
        TurbulenceModel::KOmegaSst => "k-omega SST",
        TurbulenceModel::RealizableKe => "realizable k-epsilon",
        TurbulenceModel::RngKEpsilon => "RNG k-epsilon",
    }
}

/// The lid-driven cavity at Reynolds number 100 against the tabulated centreline velocities of
/// Ghia, Ghia and Shin.
pub struct GhiaBenchmark {
    pub table: GhiaTable,
}

impl GhiaBenchmark {
    /// With the shipped tolerances, or the caller's tolerance file when one is given.
    pub fn new(tolerance_override: Option<&Path>) -> Result<Self, String> {
        Ok(Self {
            table: ghia::load_table(tolerance_override)?,
        })
    }
}

impl Benchmark for GhiaBenchmark {
    fn id(&self) -> &'static str {
        "ghia"
    }

    fn title(&self) -> &'static str {
        "Lid-driven cavity, Reynolds number 100"
    }

    fn reference(&self) -> &'static str {
        "Ghia, Ghia and Shin (1982)"
    }

    fn matches(&self, r: &ResolvedStudy) -> Result<(), Vec<Mismatch>> {
        let mut found = Vec::new();
        let lid = r.lid_velocity();
        if lid.is_none() {
            found.push(mismatch(
                "lid",
                "no single moving wall on the top face",
                "exactly one moving wall, on the top face",
            ));
        }
        let (length_x, length_y) = (r.domain.length_x, r.domain.length_y);
        if r.domain.shape.is_some() {
            found.push(mismatch("shape", "a shaped channel", "a square box"));
        } else if (length_x / length_y - 1.0).abs() > 1e-10 {
            found.push(mismatch(
                "shape",
                format!("{} m by {} m", num(length_x), num(length_y)),
                "a square",
            ));
        }
        if let Some(lid) = lid {
            let re = lid * length_y / r.fluid.kinematic_viscosity;
            if (re - 100.0).abs() > 1e-8 {
                found.push(mismatch("reynolds number", num(re), "100"));
            }
        }
        if let Some(physics) = &r.physics {
            if matches!(physics.time, ResolvedTime::Transient { .. }) {
                found.push(mismatch("time", "transient", "steady"));
            }
            if physics.turbulence.model != TurbulenceModel::Laminar {
                found.push(mismatch(
                    "turbulence model",
                    turbulence_name(physics.turbulence.model),
                    "laminar",
                ));
            }
            if physics.buoyancy.is_some() {
                found.push(mismatch("buoyancy", "on", "off"));
            }
            if physics.init != InitMode::Uniform {
                found.push(mismatch("initialisation", "potential flow", "uniform"));
            }
        }
        if found.is_empty() { Ok(()) } else { Err(found) }
    }

    fn compare(&self, files: &dyn ResultFiles, r: &ResolvedStudy) -> GateReport {
        ghia::compare_files(files, r, &self.table)
    }

    fn quantities(&self, files: &dyn ResultFiles, r: &ResolvedStudy) -> Vec<(String, f64)> {
        match ghia::centrelines(files, r) {
            Ok((vertical, _)) => linear_sample(&vertical, 0.5)
                .map(|u| vec![("bench.ghia.u_centre".to_owned(), u)])
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }
}

/// The benchmark ids, in the order a study is tried against them.
pub fn ids() -> &'static [&'static str] {
    &["ghia", "step", "armaly", "cylinder", "dvd"]
}

/// The text of a file given by a person, at most `limit` bytes. An error names the file as it
/// was typed.
fn read_bounded(path: &Path, limit: u64) -> Result<String, String> {
    let shown = path.display();
    let file = std::fs::File::open(path).map_err(|e| format!("{shown} cannot be read: {e}"))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{shown} cannot be read: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{shown} is larger than {} KiB", limit / 1024));
    }
    String::from_utf8(bytes).map_err(|_| format!("{shown} is not text"))
}

/// The benchmark with this id. A tolerance file replaces the shipped tolerances where the
/// benchmark has them: it is bounded, it needs a justification, and every report names it. The
/// laminar step has no tolerance setting a file can change and refuses one.
pub fn build(id: &str, tolerance_override: Option<&Path>) -> Result<Box<dyn Benchmark>, String> {
    let text = || -> Result<Option<String>, String> {
        tolerance_override
            .map(|path| read_bounded(path, MAX_OVERRIDE_BYTES))
            .transpose()
    };
    match id {
        "ghia" => Ok(Box::new(GhiaBenchmark::new(tolerance_override)?)),
        "step" => {
            let benchmark = StepBenchmark::new().map_err(|e| e.to_string())?;
            match text()? {
                None => Ok(Box::new(benchmark)),
                Some(text) => {
                    let shipped = StepTolerances::default_embedded().map_err(|e| e.to_string())?;
                    let changed =
                        StepTolerances::from_toml(&text, shipped).map_err(|e| e.to_string())?;
                    Ok(Box::new(benchmark.with_tolerances(changed, OVERRIDE_NOTE)))
                }
            }
        }
        "armaly" => {
            if tolerance_override.is_some() {
                return Err("the armaly benchmark has no tolerance that a file can change".into());
            }
            Ok(Box::new(ArmalyBenchmark::new().map_err(|e| e.to_string())?))
        }
        "cylinder" => {
            let tolerances = match text()? {
                None => CylinderTolerances::default_embedded(),
                Some(text) => CylinderTolerances::from_toml(&text),
            }
            .map_err(|e| e.to_string())?;
            Ok(Box::new(
                CylinderBenchmark::new(tolerances).map_err(|e| e.to_string())?,
            ))
        }
        "dvd" => {
            let benchmark = DvdBenchmark::new().map_err(|e| e.to_string())?;
            match text()? {
                None => Ok(Box::new(benchmark)),
                Some(text) => Ok(Box::new(
                    benchmark.with_override(&text).map_err(|e| e.to_string())?,
                )),
            }
        }
        other => Err(format!("{other} is not a known benchmark")),
    }
}

/// Every benchmark a study was tried against, with what differs.
pub type Rejected = Vec<(String, Vec<Mismatch>)>;

/// The id of the first benchmark the study is, or every id with what differs.
pub fn select_id(r: &ResolvedStudy) -> Result<&'static str, Rejected> {
    let mut rejected = Rejected::new();
    for &id in ids() {
        match build(id, None) {
            Ok(benchmark) => match benchmark.matches(r) {
                Ok(()) => return Ok(id),
                Err(found) => rejected.push((id.to_owned(), found)),
            },
            Err(reason) => rejected.push((
                id.to_owned(),
                vec![mismatch("reference data", reason, "readable")],
            )),
        }
    }
    Err(rejected)
}

/// The first benchmark the study is, or every id with what differs.
pub fn select(
    r: &ResolvedStudy,
    tolerance_override: Option<&Path>,
) -> Result<Box<dyn Benchmark>, Rejected> {
    let id = select_id(r)?;
    build(id, tolerance_override).map_err(|reason| {
        vec![(
            id.to_owned(),
            vec![mismatch("reference data", reason, "readable")],
        )]
    })
}

/// The numbers the benchmark that the study is records for a run, by name. Nothing when the
/// study is no benchmark or the files lack what the benchmark reads.
pub fn quantities_for(r: &ResolvedStudy, files: &dyn ResultFiles) -> Vec<(String, f64)> {
    select(r, None)
        .map(|benchmark| benchmark.quantities(files, r))
        .unwrap_or_default()
}

fn differences(mismatches: &[Mismatch]) -> String {
    mismatches
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// The sentence that says a study is not a benchmark and lists every difference.
pub fn mismatch_sentence(id: &str, mismatches: &[Mismatch]) -> String {
    say(&Line::BenchmarkNotMatching {
        benchmark: id.to_owned(),
        differences: differences(mismatches),
    })
}

/// The finding for a study that is checked against a benchmark it is not.
pub fn not_a_benchmark(id: &str, mismatches: &[Mismatch]) -> Diagnostic {
    finding(
        Severity::Error,
        "reference benchmark",
        (
            "CFD-BENCH-001",
            Line::BenchmarkNotMatching {
                benchmark: id.to_owned(),
                differences: differences(mismatches),
            },
        ),
    )
}
