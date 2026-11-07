//! The gate chain accepts the studies that are not one laminar steady box: the backward step, the
//! cylinder with its force and boundary probes, the heated cavity with its heat-flux surfaces, a
//! transient start and a potential-flow start. The read-back check is given the backend's list of
//! what a shaped study must have, and still refuses a case the study does not account for.

mod plan_studies;

use cfdcraft_backend_esi::{CaseTree, Dialect};
use cfdcraft_ops::{ExitClass, Report, prepare, prepare::prepare_with_tamper};
use plan_studies::{
    cylinder_json, cylinder_monitors, heated_json, heated_surfaces, potential_cavity_text,
    step_json, transient_cavity_text,
};
use std::path::Path;

fn edit(tree: &mut CaseTree, name: &str, change: impl Fn(&str) -> String) {
    let file = tree
        .files
        .iter_mut()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    file.bytes = change(std::str::from_utf8(&file.bytes).unwrap()).into_bytes();
}

fn failed(result: Result<cfdcraft_ops::Prepared, Report>) -> Report {
    result.expect_err("the chain should have stopped")
}

#[test]
fn prepare_accepts_the_shaped_studies() {
    let studies = [
        ("step", step_json()),
        ("cylinder", cylinder_json("")),
        (
            "cylinder with a force and boundary probes",
            cylinder_json(&cylinder_monitors("drag", "front", "rear")),
        ),
        ("heated cavity", heated_json("")),
        (
            "heated cavity with heat-flux surfaces",
            heated_json(&heated_surfaces("cold_flux", "hot_flux")),
        ),
        ("transient cavity", transient_cavity_text()),
        ("potential cavity", potential_cavity_text()),
    ];
    for (name, json) in studies {
        let prepared = match prepare(&json, Dialect::EsiV2512) {
            Ok(prepared) => prepared,
            Err(report) => panic!("{name}: {:#?}", report.diagnostics),
        };
        assert!(
            prepared
                .warnings
                .iter()
                .all(|d| !d.code.starts_with("CFD-RT-")),
            "{name}: {:?}",
            prepared.warnings
        );
        assert!(!prepared.hash.is_empty(), "{name}");
        assert!(
            prepared
                .export_tree
                .files
                .iter()
                .any(|f| f.rel_path == Path::new("Allrun")),
            "{name}: the export holds no Allrun"
        );
        assert!(
            prepared.export_tree.files.len() > prepared.tree.files.len(),
            "{name}"
        );
    }
}

#[test]
fn prepare_still_refuses_a_case_the_study_does_not_account_for() {
    // A wall that the mesh dictionary writes as an open patch.
    let report = failed(prepare_with_tamper(
        &step_json(),
        Dialect::EsiV2512,
        |tree| {
            edit(tree, "system/blockMeshDict", |t| {
                t.replacen("type wall;", "type patch;", 1)
            });
        },
    ));
    assert_eq!(report.class(), ExitClass::GuardRefused);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| matches!(d.code.as_str(), "CFD-RT-001" | "CFD-RT-002")),
        "{:?}",
        report.diagnostics
    );

    // A time step the study did not ask for, in a transient case.
    let report = failed(prepare_with_tamper(
        &cylinder_json(""),
        Dialect::EsiV2512,
        |tree| {
            edit(tree, "system/controlDict", |t| {
                t.replacen("deltaT 0.001;", "deltaT 0.002;", 1)
            });
        },
    ));
    assert_eq!(report.class(), ExitClass::GuardRefused);
    assert!(
        report.diagnostics.iter().any(|d| d.code == "CFD-RT-001"),
        "{:?}",
        report.diagnostics
    );

    // The first vertex of the cylinder channel moved off the floor.
    let report = failed(prepare_with_tamper(
        &cylinder_json(""),
        Dialect::EsiV2512,
        |tree| {
            edit(tree, "system/blockMeshDict", |t| {
                t.replacen("(0 0 0)", "(0 0.001 0)", 1)
            });
        },
    ));
    assert_eq!(report.class(), ExitClass::GuardRefused);
    assert!(
        report.diagnostics.iter().any(|d| d.code == "CFD-RT-001"),
        "{:?}",
        report.diagnostics
    );
}
