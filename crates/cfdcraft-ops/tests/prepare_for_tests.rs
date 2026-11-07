//! The gate chain run through a registry row gives what the dialect-taking functions give.

use cfdcraft_backend::{CaseTree, CompileOptions};
use cfdcraft_backend_esi::{Dialect, case::CaseOptions};
use cfdcraft_ops::{
    Prepared, Report, backends::by_id, prepare, prepare::prepare_for_with_tamper,
    prepare::prepare_with_tamper, prepare_for, prepare_with_options,
};
use std::path::Path;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn esi() -> cfdcraft_ops::backends::Row {
    by_id("esi-v2512").expect("the registry holds the default backend")
}

fn same_case(left: &Prepared, right: &Prepared) {
    assert_eq!(left.hash, right.hash);
    assert_eq!(left.tree, right.tree);
    assert_eq!(left.export_tree, right.export_tree);
    assert_eq!(left.warnings, right.warnings);
}

fn failed(result: Result<Prepared, Report>) -> Report {
    result.expect_err("the chain should have stopped")
}

fn edit(tree: &mut CaseTree, name: &str, change: impl Fn(&str) -> String) {
    let file = tree
        .files
        .iter_mut()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    file.bytes = change(std::str::from_utf8(&file.bytes).unwrap()).into_bytes();
}

#[test]
fn prepare_for_gives_the_same_case_as_prepare() {
    let old = prepare(CAVITY, Dialect::EsiV2512).unwrap();
    let new = prepare_for(CAVITY, esi().backend, &CompileOptions { np: 1 }).unwrap();
    same_case(&old, &new);
    assert!(!new.tree.files.is_empty());

    let old_two = prepare_with_options(CAVITY, Dialect::EsiV2512, &CaseOptions { np: 2 }).unwrap();
    let new_two = prepare_for(CAVITY, esi().backend, &CompileOptions { np: 2 }).unwrap();
    same_case(&old_two, &new_two);
    assert_ne!(new_two.export_tree, new.export_tree);
}

#[test]
fn a_failing_chain_gives_the_same_report() {
    let outlet = "    \"outlet\": {\n      \"faces\": [\"XMax\"],\n      \"kind\": \"PressureOutlet\"\n    },\n";
    let broken = CHANNEL.replace(outlet, "");
    assert_ne!(broken, CHANNEL, "the fixture changed shape");
    let old = failed(prepare(&broken, Dialect::EsiV2512));
    let new = failed(prepare_for(
        &broken,
        esi().backend,
        &CompileOptions::default(),
    ));
    assert!(!old.diagnostics.is_empty());
    assert_eq!(old.diagnostics, new.diagnostics);
    assert_eq!(old.exit_class, new.exit_class);

    let tamper = |tree: &mut CaseTree| {
        edit(tree, "system/controlDict", |t| {
            t.replace(
                "application simpleFoam;",
                "application simpleFoam;\nx #codeStream;",
            )
        });
    };
    let old = failed(prepare_with_tamper(CAVITY, Dialect::EsiV2512, tamper));
    let new = failed(prepare_for_with_tamper(
        CAVITY,
        esi().backend,
        &CompileOptions::default(),
        tamper,
    ));
    assert_eq!(old.diagnostics[0].code, "CFD-GUARD-001");
    assert_eq!(old.diagnostics, new.diagnostics);
    assert_eq!(old.study_hash, new.study_hash);
    assert_eq!(old.exit_class, new.exit_class);
}

#[test]
fn a_zeroed_viscosity_is_refused_by_the_read_back_through_the_registry_row() {
    let report = failed(prepare_for_with_tamper(
        CAVITY,
        esi().backend,
        &CompileOptions::default(),
        |tree| {
            edit(tree, "constant/transportProperties", |t| {
                t.replace("0.0001;", "0;")
            });
        },
    ));
    assert!(report.diagnostics.iter().any(|d| d.code == "CFD-RT-001"));
}
