//! The backend trait is object safe and a small backend can implement every method of it.

use cfdcraft_backend::{
    Backend, CaseFile, CaseTree, CompileFailure, CompileOptions, Family, LimitKind, Refusal,
    RunPlan, RunStep,
};
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy, Study, resolve};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

const SOURCES: [(&str, &str); 1] = [("writer.rs", "fn write() {}")];

/// A backend that writes one file and refuses one word.
struct Fake;

impl Backend for Fake {
    fn id(&self) -> &'static str {
        "fake-1"
    }
    fn family(&self) -> Family {
        Family::Lbm
    }
    fn image(&self) -> Option<&'static str> {
        None
    }
    fn preview(&self) -> bool {
        true
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::unrestricted()
    }
    fn compile(
        &self,
        _study: &ResolvedStudy,
        options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure> {
        if options.np == 0 {
            return Err(CompileFailure::Invalid {
                reason: "the process count must be at least one".into(),
            });
        }
        Ok(CaseTree {
            files: vec![CaseFile {
                rel_path: "input.txt".into(),
                bytes: b"hello".to_vec(),
            }],
        })
    }
    fn scan(&self, tree: &CaseTree) -> Vec<Refusal> {
        tree.files
            .iter()
            .filter(|f| f.bytes.windows(4).any(|w| w == b"boom"))
            .map(|f| Refusal {
                code: "CFD-TEST-001",
                file: f.rel_path.display().to_string(),
                line: 1,
                entry: "boom".into(),
                message: "the word is refused".into(),
            })
            .collect()
    }
    fn read_back(&self, _study: &ResolvedStudy, _tree: &CaseTree) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn export(
        &self,
        _study: &ResolvedStudy,
        tree: &CaseTree,
        hash: &str,
        _options: &CompileOptions,
    ) -> CaseTree {
        let mut files = tree.files.clone();
        files.push(CaseFile {
            rel_path: "README.md".into(),
            bytes: hash.as_bytes().to_vec(),
        });
        CaseTree { files }
    }
    fn run_plan(&self, _study: &ResolvedStudy, _np: u32) -> RunPlan {
        RunPlan {
            solver: "fake".into(),
            steps: vec![RunStep {
                command: "fake input.txt".into(),
                log: "log.fake".into(),
            }],
            solver_log: "log.fake".into(),
            judged_logs: vec!["log.fake".into()],
            final_fields: vec!["out.txt".into()],
            transient: false,
            buoyant: false,
            energy_field: "T".to_owned(),
            pressure_field: String::new(),
            residual_checks: vec![("rho".into(), LimitKind::Pressure)],
            potential: false,
        }
    }
    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &SOURCES
    }
}

/// Everything a caller does with a backend, through the trait object only.
fn drive(backend: &dyn Backend, study: &ResolvedStudy) -> (CaseTree, RunPlan) {
    let options = CompileOptions::default();
    let tree = backend.compile(study, &options).unwrap();
    assert!(backend.scan(&tree).is_empty());
    assert!(backend.read_back(study, &tree).is_empty());
    let exported = backend.export(study, &tree, "abc", &options);
    assert_eq!(exported.files.len(), tree.files.len() + 1);
    (tree, backend.run_plan(study, 1))
}

#[test]
fn the_trait_is_object_safe() {
    let study = resolve(&Study::from_json(CAVITY).unwrap()).unwrap();
    let rows: [&dyn Backend; 1] = [&Fake];
    let (tree, plan) = drive(rows[0], &study);
    assert_eq!(tree.files[0].rel_path, std::path::Path::new("input.txt"));
    assert_eq!(plan.steps[0].log, plan.solver_log);
    assert_eq!(rows[0].id(), "fake-1");
    assert_eq!(rows[0].family(), Family::Lbm);
    assert_eq!(rows[0].image(), None);
    assert!(rows[0].preview());
    assert_eq!(rows[0].capabilities(), Capabilities::unrestricted());
    assert_eq!(rows[0].writer_sources().len(), 1);
    let bad = CaseTree {
        files: vec![CaseFile {
            rel_path: "x".into(),
            bytes: b"a boom b".to_vec(),
        }],
    };
    assert_eq!(rows[0].scan(&bad).len(), 1);
    let refused = rows[0]
        .compile(&study, &CompileOptions { np: 0 })
        .unwrap_err();
    assert!(matches!(refused, CompileFailure::Invalid { .. }));
}

#[test]
fn a_family_has_one_frozen_name() {
    let names: Vec<&str> = [
        Family::Esi,
        Family::Of14,
        Family::Su2,
        Family::CodeSaturne,
        Family::Lbm,
    ]
    .iter()
    .map(|f| f.name())
    .collect();
    assert_eq!(names, ["esi", "of14", "su2", "codesaturne", "lbm"]);
}

#[test]
fn one_process_is_the_default() {
    assert_eq!(CompileOptions::default().np, 1);
}

#[test]
fn a_failure_reads_as_a_sentence() {
    let failure = CompileFailure::Raw {
        label: "fv".into(),
        reason: "no such block".into(),
    };
    assert_eq!(failure.to_string(), "raw override fv: no such block");
    let unsupported = CompileFailure::NotSupported {
        setting: "buoyancy".into(),
        reason: "the solver has none".into(),
    };
    assert!(unsupported.to_string().contains("buoyancy"));
}
