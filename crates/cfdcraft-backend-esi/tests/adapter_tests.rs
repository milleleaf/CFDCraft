//! The backend trait over the OpenCFD writer gives exactly what the direct functions give: the same
//! files byte for byte, the same refusals, the same run plan, the same read-back.

mod plan_studies;
mod shape_studies;

use cfdcraft_backend::{Backend, CaseTree, CompileFailure, CompileOptions, Family, LimitKind};
use cfdcraft_backend_esi::{
    CompileError, Dialect, ResidualLimit, SolverPlan, WRITER_SOURCES,
    case::{CaseOptions, compile_with},
    compile, dialect,
    export::export_tree_with,
    guard,
    rows::backends,
};
use cfdcraft_model::{ResolvedStudy, Severity};
use plan_studies::{TRANSIENT_ONLY, cavity, heated, resolve_text};
use std::path::Path;

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn backend() -> &'static dyn Backend {
    backends()[0]
}

/// The golden studies, the solver-plan studies and the two shaped ones.
fn studies() -> Vec<(&'static str, ResolvedStudy)> {
    vec![
        ("cavity v0", resolve_text(CAVITY_V0)),
        ("cavity v1", resolve_text(CAVITY_V1)),
        ("channel", resolve_text(CHANNEL_V1)),
        ("steady cavity", cavity("{}", "5000", "500", 0)),
        ("transient cavity", cavity(TRANSIENT_ONLY, "8", "0.5", 0)),
        ("heated cavity", heated("", "1000", "100")),
        (
            "cylinder",
            shape_studies::resolved(&shape_studies::cylinder_json(0, "cylinder")),
        ),
        (
            "step",
            shape_studies::resolved(&shape_studies::step_json(true, 0, "floor")),
        ),
    ]
}

fn replace_text(tree: &mut CaseTree, name: &str, from: &str, to: &str) {
    let file = tree
        .files
        .iter_mut()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    let text = String::from_utf8(file.bytes.clone()).unwrap();
    assert!(text.contains(from), "{name} has no {from}");
    file.bytes = text.replace(from, to).into_bytes();
}

#[test]
fn the_trait_compiles_every_golden_study_byte_for_byte() {
    for (name, study) in studies() {
        let direct = compile(&study, Dialect::EsiV2512).unwrap();
        let through = backend()
            .compile(&study, &CompileOptions { np: 1 })
            .unwrap();
        assert_eq!(through, direct, "{name}, one process");
        let direct_two = compile_with(&study, Dialect::EsiV2512, &CaseOptions { np: 2 }).unwrap();
        let through_two = backend()
            .compile(&study, &CompileOptions { np: 2 })
            .unwrap();
        assert_eq!(through_two, direct_two, "{name}, two processes");
        assert_ne!(through_two, through, "{name}: two processes add a file");
    }
}

#[test]
fn a_refused_study_reads_the_same_through_the_trait() {
    let mut study = resolve_text(CAVITY_V0);
    study.mesh.cells_z = 2;
    let old = compile(&study, Dialect::EsiV2512).unwrap_err();
    assert!(matches!(old, CompileError::NotTwoDimensional));
    let new = backend()
        .compile(&study, &CompileOptions::default())
        .unwrap_err();
    assert_eq!(
        new,
        CompileFailure::Invalid {
            reason: old.to_string()
        }
    );
    let none = backend()
        .compile(&resolve_text(CAVITY_V0), &CompileOptions { np: 0 })
        .unwrap_err();
    assert!(matches!(none, CompileFailure::Invalid { .. }));
}

#[test]
fn export_and_scan_match_the_old_functions() {
    for (name, study) in studies() {
        for np in [1, 2] {
            let options = CompileOptions { np };
            let tree = backend().compile(&study, &options).unwrap();
            assert_eq!(
                backend().export(&study, &tree, "abc123", &options),
                export_tree_with(&tree, "abc123", np, &SolverPlan::for_study(&study)),
                "{name}, {np} processes"
            );
            assert!(backend().scan(&tree).is_empty(), "{name}");
        }
    }
    let mut tree = compile(&resolve_text(CAVITY_V0), Dialect::EsiV2512).unwrap();
    replace_text(
        &mut tree,
        "system/controlDict",
        "FoamFile",
        "a 1;\ntype coded;\nb 2;\ntype systemCall;\nFoamFile",
    );
    let old = guard::scan_tree(&tree);
    let new = backend().scan(&tree);
    assert!(!old.is_empty());
    assert_eq!(new.len(), old.len());
    for (n, o) in new.iter().zip(&old) {
        assert_eq!(n.code, o.rule.code());
        assert_eq!(n.file, o.file);
        assert_eq!(n.line as usize, o.line);
        assert_eq!(n.entry, o.entry);
        assert_eq!(n.message, o.message);
    }
    let found: Vec<(&str, u32)> = new.iter().map(|r| (r.code, r.line)).collect();
    assert_eq!(found, [("CFD-GUARD-002", 2), ("CFD-GUARD-003", 4)]);
}

#[test]
fn run_plan_matches_the_solver_plan() {
    for (name, study) in studies() {
        for np in [1, 2] {
            let plan = SolverPlan::for_study(&study);
            let got = backend().run_plan(&study, np);
            let steps: Vec<(String, String)> = got
                .steps
                .iter()
                .map(|s| (s.command.clone(), s.log.clone()))
                .collect();
            assert_eq!(steps, plan.allrun_steps(np), "{name}");
            assert_eq!(got.solver, plan.binary, "{name}");
            assert_eq!(got.solver_log, plan.log_file, "{name}");
            assert_eq!(got.judged_logs, plan.judged_logs(), "{name}");
            assert_eq!(got.final_fields, plan.final_fields, "{name}");
            assert_eq!(got.transient, plan.transient, "{name}");
            assert_eq!(got.buoyant, plan.buoyant, "{name}");
            assert_eq!(got.pressure_field, plan.pressure_field, "{name}");
            assert_eq!(got.potential, plan.potential, "{name}");
            let checks: Vec<(String, LimitKind)> = plan
                .residual_checks
                .iter()
                .map(|(field, limit)| {
                    let kind = match limit {
                        ResidualLimit::Pressure => LimitKind::Pressure,
                        ResidualLimit::Velocity => LimitKind::Velocity,
                    };
                    (field.clone(), kind)
                })
                .collect();
            assert_eq!(got.residual_checks, checks, "{name}");
        }
    }
    let heated = backend().run_plan(&heated("", "1000", "100"), 1);
    assert!(heated.buoyant && !heated.transient);
    let transient = backend().run_plan(&cavity(TRANSIENT_ONLY, "8", "0.5", 0), 1);
    assert!(transient.transient && transient.residual_checks.is_empty());
}

#[test]
fn read_back_finds_a_lost_value_and_passes_a_clean_tree() {
    let study = resolve_text(CAVITY_V0);
    let mut tree = backend()
        .compile(&study, &CompileOptions::default())
        .unwrap();
    assert!(backend().read_back(&study, &tree).is_empty());
    replace_text(
        &mut tree,
        "constant/transportProperties",
        "nu [0 2 -1 0 0 0 0] ",
        "nu [0 2 -1 0 0 0 0] 9",
    );
    let found = backend().read_back(&study, &tree);
    assert!(!found.is_empty());
    assert!(found.iter().any(|d| d.severity == Severity::Error));
}

#[test]
fn the_row_states_its_identity() {
    let rows = backends();
    let ids: Vec<&str> = rows.iter().map(|r| r.id()).collect();
    assert_eq!(ids, ["esi-v2512", "esi-v2606"]);
    let row = rows[0];
    assert_eq!(row.id(), "esi-v2512");
    assert_eq!(row.family(), Family::Esi);
    assert_eq!(row.image(), Some(dialect::IMAGE));
    assert!(!row.preview());
    assert_eq!(row.capabilities(), Dialect::EsiV2512.capabilities());
    assert_eq!(row.writer_sources().len(), 12);
    assert_eq!(row.writer_sources(), &WRITER_SOURCES[..]);
}
