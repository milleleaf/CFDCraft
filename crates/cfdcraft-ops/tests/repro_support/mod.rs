//! Shared pieces for the tests that solve documents without a compute host: a runner that writes
//! a prepared results folder instead of running a case, and small helpers to make documents.
//! Nothing here is specific to one operating system.
#![allow(dead_code)]

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_ops::{CaseRun, CaseRunner, ExitClass, Report, Repro, RunData, save_document};
use cfdcraft_runs::{
    Check, KeyNumbers, Output, Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id,
};
use cfdcraft_store::ContentHash;
use cfdcraft_text::{Line, Text};
use serde_json::Value;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

/// The cavity study, at the schema the files use now.
pub const CAVITY: &str = include_str!("../../../cfdcraft-model/tests/fixtures/cavity_v1.json");

/// A fresh, empty folder under the build folder, named by the process and the tag.
pub fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ops-solve-tests")
        .join(format!("{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A new document called `name` in `dir`, made from the study text.
pub fn new_document(dir: &Path, name: &str, study_json: &str) -> PathBuf {
    let path = dir.join(format!("{name}.cfdcraft"));
    let saved = save_document(study_json, &path, false);
    assert_eq!(
        saved.report.class(),
        ExitClass::Passed,
        "{}",
        saved.report.render_text()
    );
    path
}

/// The study text with one change made to it as JSON.
pub fn edit(study_json: &str, change: impl FnOnce(&mut Value)) -> String {
    let mut study: Value = serde_json::from_str(study_json).unwrap();
    change(&mut study);
    study.to_string()
}

/// The cavity with another speed for the lid.
pub fn with_lid_speed(speed: f64) -> String {
    edit(CAVITY, |study| {
        study["boundary_conditions"]["lid"]["velocity"]["value"] = Value::from(speed);
    })
}

/// The sentence a person reads for a line.
pub fn say(line: &Line) -> String {
    Text::english().render(line)
}

pub fn passed(report: &Report) {
    assert_eq!(
        report.class(),
        ExitClass::Passed,
        "{}",
        report.render_text()
    );
}

/// What a solve reported about the stages.
pub fn data(report: &Report) -> &RunData {
    match &report.repro {
        Some(Repro::Run(data)) => data,
        other => panic!("expected the data of a solve, found {other:?}"),
    }
}

pub fn codes(report: &Report) -> Vec<&str> {
    report.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

/// A runner that writes a folder like the one a good run leaves, and counts how often it is asked.
///
/// The cell count of the folder follows the study it is given, and its centreline samples follow
/// the speed of the lid, so two studies that differ in those give different blobs.
pub struct FixtureRunner {
    /// How many times the case was run.
    pub calls: usize,
    root: PathBuf,
    /// Answer every call with this report instead of running.
    pub fail_with: Option<Report>,
    /// Add a check of this name that does not pass.
    pub failing_check: Option<String>,
    /// Leave this file (relative to the folder) out of the folder.
    pub omit: Option<&'static str>,
    /// Called at the start of every call, before anything is written.
    pub hook: Option<Box<dyn FnMut()>>,
    /// The shift of the centreline samples for each call in turn, the last one repeating. While
    /// the list is empty the shift follows the speed of the lid.
    pub shifts: Vec<f64>,
    /// The study text of the latest call.
    pub last_study: Option<String>,
    /// With `fail_with` set: fail only on this call (counted from 1) instead of on every call.
    pub fail_on: Option<usize>,
}

impl FixtureRunner {
    pub fn new(root: &Path) -> Self {
        Self {
            calls: 0,
            root: root.to_owned(),
            fail_with: None,
            failing_check: None,
            omit: None,
            hook: None,
            shifts: Vec::new(),
            last_study: None,
            fail_on: None,
        }
    }
}

fn number(study: &Value, path: &[&str]) -> Option<f64> {
    path.iter()
        .try_fold(study, |value, key| value.get(*key))?
        .as_f64()
}

impl CaseRunner for FixtureRunner {
    fn run_case(&mut self, study_json: &str) -> Result<CaseRun, Box<Report>> {
        self.calls += 1;
        if let Some(hook) = self.hook.as_mut() {
            hook();
        }
        self.last_study = Some(study_json.to_owned());
        let failing = self
            .fail_with
            .as_ref()
            .filter(|_| self.fail_on.is_none_or(|call| call == self.calls));
        if let Some(report) = failing {
            return Err(Box::new(report.clone()));
        }
        let study: Value = serde_json::from_str(study_json).unwrap();
        let cell = |axis: &str| number(&study, &["mesh", axis]).unwrap_or(1.0) as u64;
        let speed =
            number(&study, &["boundary_conditions", "lid", "velocity", "value"]).unwrap_or(0.1);
        let shift = match self.shifts.as_slice() {
            [] => speed - 0.1,
            shifts => shifts[(self.calls - 1).min(shifts.len() - 1)],
        };
        let spec = FixtureSpec {
            end_time: 5000,
            cells: cell("cells_x") * cell("cells_y") * cell("cells_z"),
            samples: 64,
            // The cavity at its first speed gives the plain ramp.
            shift,
        };
        let dir = self.root.join(format!("call-{}", self.calls));
        write_results(&dir, &spec).unwrap();
        if let Some(omitted) = self.omit {
            fs::remove_file(dir.join(omitted)).unwrap();
        }
        let mut checks = vec![Check {
            name: "evidence".to_owned(),
            passed: true,
            detail: "passed".to_owned(),
        }];
        if let Some(name) = &self.failing_check {
            checks.push(Check {
                name: name.clone(),
                passed: false,
                detail: "outside the allowed range".to_owned(),
            });
        }
        Ok(CaseRun {
            results_dir: dir,
            checks,
            host: Some("fixture-host".to_owned()),
        })
    }
}

/// A document made from the cavity and solved, then changed to the lid speed `second_speed` and
/// solved again, so it holds `run-1` (mesh and result) and `run-2` (a result on the same mesh).
/// The runner's shifts decide how the samples of the two runs compare.
pub fn two_runs(tag: &str, second_speed: f64, shifts: Vec<f64>) -> (PathBuf, FixtureRunner) {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    runner.shifts = shifts;
    passed(&cfdcraft_ops::solve_document(&path, &mut runner).report);
    let changed = save_document(&with_lid_speed(second_speed), &path, true);
    passed(&changed.report);
    passed(&cfdcraft_ops::solve_document(&path, &mut runner).report);
    (path, runner)
}

/// Replace `fragment` by `changed` in the `artifacts.json` of the file, which must hold it once.
pub fn tamper_with_index(path: &Path, fragment: &str, changed: &str) {
    let mut zip = ZipArchive::new(Cursor::new(fs::read(path).unwrap())).unwrap();
    let mut member = |name: &str| {
        let mut bytes = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
        bytes
    };
    let manifest = member("manifest.json");
    let study = member("study.json");
    let index = String::from_utf8(member("artifacts.json")).unwrap();
    let thumbnail = member("thumbnail.png");
    assert_eq!(
        index.matches(fragment).count(),
        1,
        "the fragment is in the index once"
    );
    let damaged = index.replace(fragment, changed);
    assert_ne!(damaged, index);

    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.json", manifest),
        ("study.json", study),
        ("artifacts.json", damaged.into_bytes()),
        ("thumbnail.png", thumbnail),
    ] {
        out.start_file(name, SimpleFileOptions::default()).unwrap();
        out.write_all(&bytes).unwrap();
    }
    fs::write(path, out.finish().unwrap().into_inner()).unwrap();
}

/// Every file and folder under `root` with its size, sorted; empty when `root` is not there.
pub fn tree(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let entry = entry.unwrap();
            let meta = entry.metadata().unwrap();
            let name = entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .display()
                .to_string();
            found.push(format!("{name} {}", meta.len()));
            if meta.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

/// The bytes of a document and the listing of the store beside it.
pub fn snapshot(document: &Path) -> (Vec<u8>, Vec<String>) {
    (
        fs::read(document).unwrap(),
        tree(&cfdcraft_doc::store_dir(document)),
    )
}

/// Append a run that recorded only a mesh to the document, as one made by hand would be.
pub fn add_mesh_only_run(path: &Path) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let study = document.study.clone();
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    let recipes = StageRecipes::from_resolved(&cfdcraft_model::resolve(&study).unwrap(), &tool);
    let keys = recipes.keys();
    let mesh_hash = ContentHash::of(b"a mesh made by hand");
    let record = RunRecord {
        id: next_run_id(&document.artifacts.runs),
        rerun_of: None,
        mesh: Some(Output {
            key: keys.mesh,
            recipe: recipes.mesh.clone(),
            blob: mesh_hash,
            size: 19,
        }),
        result: None,
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash,
            result_key: None,
            settings: study,
            tool,
            host: None,
        },
        checks: Vec::new(),
        key_numbers: KeyNumbers::new(),
        comparison: None,
    };
    document.add_run(record).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
}
