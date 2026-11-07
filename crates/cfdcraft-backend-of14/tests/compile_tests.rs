//! The cavity and the laminar step compile to Foundation 14 case files, the scan holds, and the
//! settings the row offers are accepted by the model and the writer.

mod common;

use cfdcraft_backend::{Backend, CompileFailure, CompileOptions, Family};
use cfdcraft_backend_esi::{
    WRITER_SOURCES as ESI_SOURCES, patches::layout_of, patches::patch_plan,
};
use cfdcraft_backend_of14::backends;
use cfdcraft_model::{BoundaryKind, InitMode, diagnose, resolve};
use cfdcraft_templates::{MeshPreset, TemplateOptions, TimeChoice};
use common::{authored, backend, cavity, coarse, compiled, names, options, step, text};

/// The backend behind its trait, so the trait is in scope for every call.
fn row() -> &'static dyn Backend {
    backend()
}

#[test]
fn the_cavity_compiles_to_the_v14_file_set() {
    let tree = compiled(&cavity());
    assert_eq!(
        names(&tree),
        [
            "0/U",
            "0/p",
            "constant/momentumTransport",
            "constant/physicalProperties",
            "system/blockMeshDict",
            "system/controlDict",
            "system/fvSchemes",
            "system/fvSolution",
        ]
    );
    let control = text(&tree, "system/controlDict");
    assert!(control.contains("solver incompressibleFluid;"));
    assert!(!control.contains("application"));
    let schemes = text(&tree, "system/fvSchemes");
    assert!(schemes.contains("ddtSchemes { default steadyState; }"));
    let solution = text(&tree, "system/fvSolution");
    assert!(solution.contains("PIMPLE"));
    assert!(solution.contains("nOuterCorrectors 1;"));
    assert!(solution.contains("consistent yes;"));
    assert!(solution.contains("pRefCell 0;"));
    assert!(solution.contains("pRefValue 0;"));
    let physical = text(&tree, "constant/physicalProperties");
    assert!(physical.contains("viscosityModel constant;"));
    assert!(physical.contains("nu "));
    assert!(text(&tree, "constant/momentumTransport").contains("simulationType laminar;"));
}

#[test]
fn the_cavity_tree_is_deterministic() {
    let study = cavity();
    assert_eq!(compiled(&study), compiled(&study));
    let two = CompileOptions { np: 2 };
    let parallel = row().compile(&study, &two).unwrap();
    assert!(names(&parallel).contains(&"system/decomposeParDict".to_owned()));
    assert!(text(&parallel, "system/decomposeParDict").contains("numberOfSubdomains 2;"));
    let exported = row().export(&study, &parallel, "hash", &two);
    assert!(text(&exported, "Allrun").contains("mpirun -np 2 foamRun -parallel > log.foamRun"));
    let serial = row().export(
        &study,
        &compiled(&study),
        "hash",
        &CompileOptions::default(),
    );
    assert!(!text(&serial, "Allrun").contains("mpirun"));
    assert!(text(&serial, "README.md").contains("`log.foamRun`"));
}

#[test]
fn the_scan_passes_and_a_tamper_is_refused() {
    let tree = compiled(&cavity());
    assert!(row().scan(&tree).is_empty());
    let tampered = common::rewritten(&tree, "0/U", |t| {
        format!(
            "{t}\nextra\n{{\n    #codeStream\n    {{\n        code #{{ int a; #}};\n    }}\n}}\n"
        )
    });
    let refused = row().scan(&tampered);
    assert!(!refused.is_empty());
    assert!(refused.iter().all(|r| r.code.starts_with("CFD-GUARD-")));
}

#[test]
fn the_run_plan_names_the_v14_steps() {
    let study = cavity();
    let plan = row().run_plan(&study, 1);
    let steps: Vec<(&str, &str)> = plan
        .steps
        .iter()
        .map(|s| (s.command.as_str(), s.log.as_str()))
        .collect();
    assert_eq!(
        steps,
        [
            ("blockMesh", "log.blockMesh"),
            ("checkMesh", "log.checkMesh"),
            ("foamRun", "log.foamRun"),
        ]
    );
    assert_eq!(plan.solver, "foamRun");
    assert_eq!(plan.pressure_field, "p");
    assert!(!plan.potential);
    assert!(
        plan.steps
            .iter()
            .all(|s| !s.command.contains("potentialFoam"))
    );
    let parallel = row().run_plan(&study, 2);
    let commands: Vec<&str> = parallel.steps.iter().map(|s| s.command.as_str()).collect();
    assert_eq!(
        commands,
        [
            "blockMesh",
            "checkMesh",
            "decomposePar -force",
            "mpirun -np 2 foamRun -parallel",
            "reconstructPar -latestTime",
        ]
    );
}

#[test]
fn the_row_states_its_identity() {
    let row = row();
    assert_eq!(row.id(), "of14-20261008");
    assert_eq!(row.family(), Family::Of14);
    assert_eq!(row.image(), None);
    assert!(!row.preview());
    assert!(!row.writer_sources().is_empty());
    assert_ne!(row.writer_sources(), &ESI_SOURCES[..]);
    assert_eq!(backends().len(), 1);
}

/// The codes `diagnose` gives a study against the capabilities of this backend.
fn codes(study: &cfdcraft_model::Study) -> Vec<String> {
    diagnose(study, &row().capabilities())
        .into_iter()
        .map(|d| d.code)
        .collect()
}

#[test]
fn settings_are_accepted_and_a_boundary_kind_the_row_lacks_is_still_refused() {
    let sst = authored("backward-step", &options(MeshPreset::Coarse));
    let transient = authored(
        "lid-cavity",
        &TemplateOptions {
            time: Some(TimeChoice::Transient),
            ..options(MeshPreset::Coarse)
        },
    );
    let heated = authored("heated-cavity", &options(MeshPreset::Coarse));
    let potential = authored(
        "backward-step-laminar",
        &TemplateOptions {
            init: Some(InitMode::Potential),
            ..options(MeshPreset::Coarse)
        },
    );
    for study in [&sst, &transient, &heated, &potential] {
        let found = codes(study);
        for code in [
            "CFD-BACKEND-002",
            "CFD-BACKEND-003",
            "CFD-BACKEND-004",
            "CFD-BACKEND-005",
        ] {
            assert!(!found.contains(&code.to_owned()), "{code} in {found:?}");
        }
    }
    // The model still checks every boundary kind against the row's own list: a list without the
    // moving wall refuses the cavity's lid.
    let mut lacking = row().capabilities();
    lacking
        .boundary_kinds
        .retain(|kind| *kind != BoundaryKind::MovingWall);
    let cavity = authored("lid-cavity", &options(MeshPreset::Coarse));
    let found: Vec<String> = diagnose(&cavity, &lacking)
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(found.contains(&"CFD-MODEL-001".to_owned()), "{found:?}");
}

#[test]
fn a_transient_study_compiles_and_a_heat_flux_without_buoyancy_is_refused_by_name() {
    let transient = common::study(
        "lid-cavity",
        &TemplateOptions {
            time: Some(TimeChoice::Transient),
            ..options(MeshPreset::Coarse)
        },
    );
    assert!(
        row()
            .compile(&transient, &CompileOptions::default())
            .is_ok()
    );
    // The temperature field exists only with buoyancy, so a heat-flux surface without it has
    // nothing to integrate.
    // The model itself refuses such a study (a heat-flux surface needs a thermal condition, which
    // needs buoyancy), so the resolved study is edited after resolving to reach the row's own check.
    let mut dry = resolve(&authored("heated-cavity", &options(MeshPreset::Coarse))).unwrap();
    dry.physics.as_mut().unwrap().buoyancy = None;
    let refused = row().compile(&dry, &CompileOptions::default());
    assert!(matches!(refused, Err(CompileFailure::NotSupported { .. })));
}

#[test]
fn the_laminar_step_compiles_with_a_parabolic_inlet() {
    let study = step();
    let tree = compiled(&study);
    let layout = layout_of(&study).unwrap();
    let mesh = text(&tree, "system/blockMeshDict");
    assert_eq!(mesh.matches("hex (").count(), layout.blocks.len());
    let velocity = text(&tree, "0/U");
    let inlet = velocity
        .lines()
        .find(|l| l.trim_start().starts_with("inlet"))
        .expect("the velocity file has an inlet");
    for word in ["fixedProfile", "profile", "direction", "origin"] {
        assert!(inlet.contains(word), "{inlet}");
    }
    // The patch table is the one the older writer uses: same names, same boundary list.
    let older =
        cfdcraft_backend_esi::compile(&study, cfdcraft_backend_esi::Dialect::EsiV2512).unwrap();
    let boundary = |t: &str| t[t.find("boundary").unwrap()..].to_owned();
    assert_eq!(
        boundary(&mesh),
        boundary(&text(&older, "system/blockMeshDict"))
    );
    for patch in patch_plan(&study).unwrap() {
        assert!(mesh.contains(patch.name), "{}", patch.name);
    }
}

#[test]
fn a_graded_box_and_the_cylinder_channel_compile() {
    let options = TemplateOptions {
        time: Some(TimeChoice::Transient),
        ..options(MeshPreset::Coarse)
    };
    assert!(!codes(&authored("cylinder", &options)).contains(&"CFD-BACKEND-003".to_owned()));
    assert!(
        row()
            .compile(
                &common::study("cylinder", &options),
                &CompileOptions::default()
            )
            .is_ok()
    );
    assert!(
        row()
            .compile(&common::graded_box(), &CompileOptions::default())
            .is_ok()
    );
    // The coarse mesh preset of the cavity is plain and writes.
    assert!(
        row()
            .compile(&coarse("lid-cavity"), &CompileOptions::default())
            .is_ok()
    );
}
