//! The cylinder in a channel is a transient laminar run with the study's time step, and its
//! monitors are written in the key forms of this line.

mod common;

use cfdcraft_backend::{Backend, CompileOptions};
use cfdcraft_backend_esi::{monitors::short, patches::patch_of_label, scalar};
use cfdcraft_model::diagnose;
use cfdcraft_templates::MeshPreset;
use common::{
    authored, backend, broken, compiled, cylinder, cylinder_with_every_monitor, edited, names,
    options, text,
};

fn row() -> &'static dyn Backend {
    backend()
}

#[test]
fn the_cylinder_is_transient_laminar_with_the_study_time_step() {
    let study = cylinder();
    let tree = compiled(&study);
    let control = text(&tree, "system/controlDict");
    assert!(control.contains("solver incompressibleFluid;"));
    let step = study
        .effective_delta_t()
        .expect("the cylinder is transient");
    assert!(control.contains(&format!("deltaT {};", scalar(step))));
    assert!(control.contains("adjustTimeStep no;"));
    assert!(control.contains("writeControl runTime;"));
    assert!(control.contains(&format!("endTime {};", scalar(study.solver.end_time))));
    assert!(control.contains(&format!(
        "writeInterval {};",
        scalar(study.solver.write_interval)
    )));
    let schemes = text(&tree, "system/fvSchemes");
    assert!(schemes.contains("ddtSchemes { default backward; }"));
    assert!(!schemes.contains("steadyState"));
    assert!(text(&tree, "constant/momentumTransport").contains("simulationType laminar;"));
    let exported = row().export(&study, &tree, "hash", &CompileOptions::default());
    assert!(text(&exported, "Allrun").contains("foamRun > log.foamRun"));
    let plan = row().run_plan(&study, 1);
    assert!(plan.transient);
    assert!(plan.residual_checks.is_empty());
    assert_eq!(plan.final_fields, ["U", "p"]);
}

/// Every library a case loads, in the order of the files.
fn libraries(tree: &cfdcraft_backend::CaseTree) -> Vec<String> {
    let mut found = Vec::new();
    for name in names(tree) {
        for line in text(tree, &name).lines() {
            if let Some(rest) = line.trim().strip_prefix("libs (\"") {
                found.push(rest.trim_end_matches("\");").to_owned());
            }
        }
    }
    found
}

#[test]
fn force_coefficients_use_the_v14_keys() {
    let study = cylinder();
    let tree = compiled(&study);
    let block = common::block_of(&tree, "system/controlDict", "force_1");
    let patch = patch_of_label(&study, "cylinder").unwrap();
    let force = &study.monitor_plan().forces[0];
    for word in [
        "type forceCoeffs;".to_owned(),
        "libs (\"libforces.so\");".to_owned(),
        format!("patches ({patch});"),
        "liftDir (0 1 0);".to_owned(),
        "dragDir (1 0 0);".to_owned(),
        "pitchAxis (0 0 1);".to_owned(),
        format!("magUInf {};", short(force.reference_velocity)),
        format!("lRef {};", short(force.reference_length)),
        "Aref ".to_owned(),
        "rho rhoInf;".to_owned(),
        "rhoInf 1;".to_owned(),
    ] {
        assert!(block.contains(&word), "{word} is missing from\n{block}");
    }
    // The keys of the older line are not written.
    for word in ["e1 ", "e3 ", "origin"] {
        assert!(!block.contains(word), "{word} is in\n{block}");
    }
    // The guard accepts every library entry without a new allowlist entry.
    assert!(row().scan(&tree).is_empty());
    let allowed = [
        "libsampling.so",
        "libfieldFunctionObjects.so",
        "libforces.so",
    ];
    for lib in libraries(&tree) {
        assert!(allowed.contains(&lib.as_str()), "{lib}");
    }
}

#[test]
fn surface_integrals_sets_and_probes_use_the_v14_forms() {
    let study = cylinder_with_every_monitor();
    let tree = compiled(&study);
    let outlet = patch_of_label(&study, "outlet").unwrap();
    let surface = common::block_of(&tree, "system/controlDict", "surface_1");
    assert!(surface.contains("type surfaceFieldValue;"));
    assert!(surface.contains(&format!("patch {outlet};")));
    assert!(surface.contains("operation "));
    assert!(!surface.contains("regionType"));
    assert!(!surface.contains("name "));
    let pressure = common::block_of(&tree, "system/controlDict", "surface_2");
    assert!(pressure.contains("operation areaAverage;"));
    assert!(pressure.contains("fields (p);"));
    let line = common::block_of(&tree, "system/controlDict", "line_1");
    assert!(line.contains("libs (\"libsampling.so\");"));
    assert!(line.contains("sets\n        {\n            line\n            {\n"));
    assert!(line.contains("type lineUniform;"));
    assert!(line.contains("nPoints 21;"));
    assert!(!line.contains("sets\n        (\n"));
    let probe = common::block_of(&tree, "system/controlDict", "probe_1");
    assert!(probe.contains("type patchProbes;"));
    assert!(probe.contains("probeLocations\n        (\n"));
    assert!(probe.contains(&format!(
        "patches ({});",
        patch_of_label(&study, "cylinder").unwrap()
    )));
    assert!(row().scan(&tree).is_empty());
}

#[test]
fn the_cylinder_round_trips_and_matches_its_golden() {
    for (folder, study) in [
        ("cylinder-of14", cylinder()),
        ("cylinder-monitors-of14", cylinder_with_every_monitor()),
    ] {
        let tree = compiled(&study);
        assert_eq!(row().read_back(&study, &tree), vec![]);
        assert!(row().scan(&tree).is_empty());
        let plan = study.monitor_plan();
        // The time step.
        let step = scalar(study.effective_delta_t().unwrap());
        let changed = edited(
            &tree,
            "system/controlDict",
            &format!("deltaT {step};"),
            "deltaT 0.5;",
        );
        assert!(broken(&changed, &study));
        // A probe location.
        let probe = &plan.probes[0];
        let at = format!("({} {} ", scalar(probe.x), scalar(probe.y));
        let moved = format!("({} {} ", scalar(probe.x + 0.01), scalar(probe.y));
        assert!(broken(
            &edited(&tree, "system/controlDict", &at, &moved),
            &study
        ));
        // The reference length.
        let length = format!("lRef {};", short(plan.forces[0].reference_length));
        assert!(broken(
            &edited(&tree, "system/controlDict", &length, "lRef 0.77;"),
            &study
        ));
        // Last, so a first run still shows every other check: the golden folder.
        let misses = common::golden_misses(folder, &tree);
        assert!(misses.is_empty(), "{}", misses.join("\n=====\n"));
    }
}

#[test]
fn transient_is_offered_now() {
    assert!(row().capabilities().transient);
    let study = authored("cylinder", &options(MeshPreset::Coarse));
    let codes: Vec<String> = diagnose(&study, &row().capabilities())
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(!codes.contains(&"CFD-BACKEND-003".to_owned()), "{codes:?}");
    assert!(
        row()
            .compile(&common::cylinder(), &CompileOptions::default())
            .is_ok()
    );
}
