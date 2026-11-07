//! The configuration, the mesh and the run script of each benchmark study, and what the backend
//! says about itself and about the settings it does not offer.

mod common;

use cfdcraft_backend::{Backend, CompileFailure, CompileOptions, Family};
use cfdcraft_backend_su2::{backends, config::num, guard};
use cfdcraft_model::{
    InitMode, RawBackend, RawOverride, Severity, Study, TurbulenceModel, diagnose,
};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build};
use common::{backend, coarse, compiled, has_key, options, parse_mesh, study, text, value};

/// The backend behind its trait, so the trait is in scope for every call.
fn row() -> &'static dyn Backend {
    backend()
}

#[test]
fn the_cavity_compiles_to_a_config_a_mesh_and_a_script() {
    let study = coarse("lid-cavity");
    let tree = compiled(&study);
    let names: Vec<&str> = tree
        .files
        .iter()
        .map(|f| f.rel_path.to_str().unwrap())
        .collect();
    assert_eq!(names, ["config.cfg", "mesh.su2"]);
    assert_eq!(value(&tree, "SOLVER"), "INC_NAVIER_STOKES");
    assert_eq!(value(&tree, "SURFACE_MOVEMENT"), "MOVING_WALL");
    assert_eq!(value(&tree, "MARKER_MOVING"), "movingWall");
    assert_eq!(value(&tree, "SURFACE_TRANSLATION_RATE"), "0.1 0 0");
    assert_eq!(
        value(&tree, "MARKER_HEATFLUX"),
        "( movingWall, 0, fixedWalls, 0 )"
    );
    // The density is one, so the dynamic viscosity is the kinematic viscosity of the study.
    assert_eq!(value(&tree, "INC_DENSITY_INIT"), "1");
    assert_eq!(
        value(&tree, "MU_CONSTANT"),
        num(study.fluid.kinematic_viscosity)
    );
    assert_eq!(
        value(&tree, "ITER"),
        format!("{}", study.solver.end_time as u64)
    );
    assert_eq!(value(&tree, "CONV_FILENAME"), "history");
    assert_eq!(value(&tree, "TABULAR_FORMAT"), "CSV");
    assert_eq!(value(&tree, "MESH_FILENAME"), "mesh.su2");
    assert!(!has_key(&tree, "TIME_DOMAIN"));

    let exported = row().export(&study, &tree, "0123abcd", &CompileOptions::default());
    let names: Vec<&str> = exported
        .files
        .iter()
        .map(|f| f.rel_path.to_str().unwrap())
        .collect();
    assert_eq!(names, ["config.cfg", "mesh.su2", "Allrun", "README.md"]);
    assert_eq!(
        text(&exported, "Allrun"),
        "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\nSU2_CFD -t 1 config.cfg > log.SU2 2>&1 || exit 1\n"
    );
    let readme = text(&exported, "README.md");
    assert!(readme.contains("Study hash: `0123abcd`"));
    assert!(readme.contains("Exit Success (SU2_CFD)"));
    // One line per paragraph, and no solver name in lowercase outside backticks.
    assert!(!readme.contains("\n\n\n"));
    for line in readme.lines().filter(|l| !l.starts_with("```")) {
        let without_code: String = line.split('`').step_by(2).collect();
        assert!(!without_code.contains("cfdcraft"), "{line}");
    }
    // More threads change the script and the README, and nothing in the case.
    let two = row().export(&study, &tree, "0123abcd", &CompileOptions { np: 2 });
    assert!(text(&two, "Allrun").contains("SU2_CFD -t 2 config.cfg"));
    assert!(text(&two, "README.md").contains("--cpus 2"));
    assert!(!readme.contains("--cpus"));
}

#[test]
fn the_row_states_its_identity_and_limits() {
    let row = row();
    assert_eq!(row.id(), "su2-8.6.0");
    assert_eq!(row.family(), Family::Su2);
    assert_eq!(row.image(), None);
    assert!(!row.preview());
    assert_eq!(backends().len(), 1);
    let caps = row.capabilities();
    assert_eq!(
        caps.turbulence_models,
        vec![TurbulenceModel::Laminar, TurbulenceModel::KOmegaSst]
    );
    assert!(caps.transient && caps.buoyancy && caps.parabolic_inlet);
    assert!(!caps.potential_init && !caps.accepts_raw && !caps.uniform_square_cells);
    assert!(caps.requires_single_cell_z);

    let findings = |study: &Study| {
        diagnose(study, &caps)
            .into_iter()
            .filter(|d| d.code.starts_with("CFD-BACKEND"))
            .map(|d| (d.code, d.severity))
            .collect::<Vec<_>>()
    };
    for model in [TurbulenceModel::RealizableKe, TurbulenceModel::RngKEpsilon] {
        let step = build(
            "backward-step",
            &TemplateOptions {
                turbulence: Some(model),
                ..TemplateOptions::default()
            },
        )
        .unwrap();
        assert_eq!(
            findings(&step),
            [("CFD-BACKEND-002".to_owned(), Severity::Error)]
        );
    }
    let potential = build(
        "lid-cavity",
        &TemplateOptions {
            init: Some(InitMode::Potential),
            ..TemplateOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        findings(&potential),
        [("CFD-BACKEND-005".to_owned(), Severity::Error)]
    );
    let mut raw = build("lid-cavity", &TemplateOptions::default()).unwrap();
    raw.raw_overrides.insert(
        "relax-u".into(),
        RawOverride {
            backend: RawBackend::Esi,
            file: "system/fvSolution".into(),
            entry: "relaxationFactors/equations/U".into(),
            value: "0.5".into(),
        },
    );
    assert_eq!(
        findings(&raw),
        [("CFD-BACKEND-006".to_owned(), Severity::Error)]
    );
    // The supported physics raises none of them.
    for id in ["lid-cavity", "backward-step", "cylinder", "heated-cavity"] {
        let ok = build(id, &TemplateOptions::default()).unwrap();
        assert!(findings(&ok).is_empty(), "{id}");
    }
}

#[test]
fn a_study_with_more_than_one_cell_through_z_is_refused_never_compiled() {
    let mut deep = build("lid-cavity", &TemplateOptions::default()).unwrap();
    deep.mesh.cells_z = 2;
    let found = diagnose(&deep, &row().capabilities());
    assert!(found.iter().any(|d| d.severity == Severity::Error));
    let mut resolved = coarse("lid-cavity");
    resolved.mesh.cells_z = 2;
    assert!(matches!(
        row().compile(&resolved, &CompileOptions::default()),
        Err(CompileFailure::Invalid { .. })
    ));
}

#[test]
fn a_setting_the_backend_does_not_offer_is_named_and_never_compiled() {
    let step = study(
        "backward-step",
        &TemplateOptions {
            turbulence: Some(TurbulenceModel::RealizableKe),
            ..options(MeshPreset::Coarse)
        },
    );
    assert!(matches!(
        row().compile(&step, &CompileOptions::default()),
        Err(CompileFailure::NotSupported { .. })
    ));
    let potential = study(
        "lid-cavity",
        &TemplateOptions {
            init: Some(InitMode::Potential),
            ..options(MeshPreset::Coarse)
        },
    );
    assert!(matches!(
        row().compile(&potential, &CompileOptions::default()),
        Err(CompileFailure::NotSupported { .. })
    ));
}

#[test]
fn the_laminar_step_has_a_parabolic_inlet_profile() {
    let study = coarse("backward-step-laminar");
    let tree = compiled(&study);
    assert_eq!(value(&tree, "SPECIFIED_INLET_PROFILE"), "YES");
    assert_eq!(value(&tree, "INLET_FILENAME"), "inlet.dat");
    assert!(guard::ALLOWED_FILES.contains(&"inlet.dat"));
    assert!(row().scan(&tree).is_empty());
    let profile = text(&tree, "inlet.dat");
    let lines: Vec<&str> = profile.lines().collect();
    assert_eq!(lines[0], "NMARK= 1");
    assert_eq!(lines[1], "MARKER_TAG= inlet");
    let rows: usize = lines[2].strip_prefix("NROW= ").unwrap().parse().unwrap();
    assert_eq!(lines[3], "NCOL= 6");
    assert!(lines[4].starts_with("# COORD-X COORD-Y"));
    assert_eq!(lines.len(), 5 + rows);
    // One row per node of the inlet marker, as the mesh holds it.
    let mesh = parse_mesh(&text(&tree, "mesh.su2"));
    let (_, edges) = mesh.markers.iter().find(|(t, _)| t == "inlet").unwrap();
    let mut nodes: Vec<usize> = edges.iter().flatten().copied().collect();
    nodes.sort_unstable();
    nodes.dedup();
    assert_eq!(rows, nodes.len());
    // The mean speed is the inlet speed and the peak is one and a half times it.
    let mean = study.boundary_conditions["inlet"].velocity.unwrap().abs();
    let speeds: Vec<f64> = lines[5..]
        .iter()
        .map(|line| {
            let words: Vec<f64> = line.split(' ').map(|w| w.parse().unwrap()).collect();
            assert_eq!(words.len(), 6, "{line}");
            assert!(words.iter().all(|w| w.is_finite()), "{line}");
            assert_eq!((words[4], words[5]), (1.0, 0.0), "{line}");
            words[3]
        })
        .collect();
    let peak = speeds.iter().copied().fold(0.0, f64::max);
    assert!(
        peak <= 1.5 * mean * (1.0 + 1e-9) && peak > 1.4 * mean,
        "{peak}"
    );
    // The walls hold no flow.
    assert!(speeds.iter().copied().fold(f64::MAX, f64::min) <= 1e-9 * mean);
    // A study with no parabolic inlet writes no profile and no profile keys.
    let flat = compiled(&coarse("lid-cavity"));
    assert!(!has_key(&flat, "SPECIFIED_INLET_PROFILE"));
    assert!(!flat.files.iter().any(|f| f.rel_path.ends_with("inlet.dat")));
}

#[test]
fn the_sst_step_uses_the_rans_solver() {
    let sst = compiled(&coarse("backward-step"));
    assert_eq!(value(&sst, "SOLVER"), "INC_RANS");
    assert_eq!(value(&sst, "KIND_TURB_MODEL"), "SST");
    // The inflow values come from the intensity and the viscosity ratio of the study, at the speed
    // the solver starts from.
    assert_eq!(value(&sst, "INC_VELOCITY_INIT"), "( 44.2, 0, 0 )");
    assert!(has_key(&sst, "FREESTREAM_TURBULENCEINTENSITY"));
    assert!(has_key(&sst, "FREESTREAM_TURB2LAMVISCRATIO"));
    assert_eq!(value(&sst, "MARKER_SYM"), "( startup )");
    assert!(row().scan(&sst).is_empty());
    for id in [
        "lid-cavity",
        "backward-step-laminar",
        "cylinder",
        "heated-cavity",
    ] {
        let laminar = compiled(&coarse(id));
        assert_eq!(value(&laminar, "SOLVER"), "INC_NAVIER_STOKES", "{id}");
        assert_eq!(value(&laminar, "KIND_TURB_MODEL"), "NONE", "{id}");
        assert!(!has_key(&laminar, "FREESTREAM_TURBULENCEINTENSITY"), "{id}");
    }
}

#[test]
fn the_cylinder_is_transient_with_dual_time_stepping() {
    let study = coarse("cylinder");
    let tree = compiled(&study);
    assert_eq!(value(&tree, "TIME_DOMAIN"), "YES");
    assert_eq!(
        value(&tree, "TIME_MARCHING"),
        "DUAL_TIME_STEPPING-2ND_ORDER"
    );
    let step = study.effective_delta_t().unwrap();
    assert_eq!(value(&tree, "TIME_STEP"), num(step));
    assert_eq!(value(&tree, "MAX_TIME"), num(study.solver.end_time));
    assert_eq!(
        value(&tree, "TIME_ITER"),
        format!("{}", (study.solver.end_time / step).round() as u64)
    );
    assert_eq!(value(&tree, "INNER_ITER"), "50");
    assert!(!has_key(&tree, "ITER"));
    // The cylinder is a no-slip wall with its own marker, and its nodes lie on the circle.
    assert_eq!(
        value(&tree, "MARKER_HEATFLUX"),
        "( fixedWalls, 0, obstacle, 0 )"
    );
    let mesh = parse_mesh(&text(&tree, "mesh.su2"));
    let (_, edges) = mesh.markers.iter().find(|(t, _)| t == "obstacle").unwrap();
    assert!(!edges.is_empty());
    for node in edges.iter().flatten() {
        let [x, y] = mesh.nodes[*node];
        let radius = ((x - 0.2).powi(2) + (y - 0.2).powi(2)).sqrt();
        assert!((radius - 0.05).abs() < 1e-9, "{radius}");
    }
    for i in 0..mesh.cells.len() {
        assert!(mesh.area(i) > 0.0);
    }
}

#[test]
fn the_heated_cavity_uses_the_boussinesq_model() {
    let study = coarse("heated-cavity");
    let tree = compiled(&study);
    assert_eq!(value(&tree, "INC_DENSITY_MODEL"), "BOUSSINESQ");
    assert_eq!(value(&tree, "INC_ENERGY_EQUATION"), "YES");
    assert_eq!(value(&tree, "INC_TEMPERATURE_INIT"), "300");
    assert_eq!(value(&tree, "PRANDTL_LAM"), "0.71");
    assert_eq!(
        value(&tree, "MARKER_ISOTHERMAL"),
        "( isothermal1, 299.5, isothermal2, 300.5 )"
    );
    assert_eq!(value(&tree, "MARKER_HEATFLUX"), "( fixedWalls, 0 )");
    // The solver refuses a body force together with the Boussinesq model and fixes gravity at the
    // standard value, so the study's gravity is carried by the expansion coefficient: the source
    // is proportional to their product.
    assert!(!has_key(&tree, "BODY_FORCE"));
    assert!(!has_key(&tree, "BODY_FORCE_VECTOR"));
    let buoyancy = study.physics.as_ref().unwrap().buoyancy.unwrap();
    let coefficient: f64 = value(&tree, "THERMAL_EXPANSION_COEFF").parse().unwrap();
    let product = coefficient * cfdcraft_backend_su2::markers::STANDARD_GRAVITY;
    let wanted = buoyancy.gravity * buoyancy.beta;
    assert!(
        (product - wanted).abs() <= 1e-12 * wanted,
        "{product} {wanted}"
    );
    assert!(row().scan(&tree).is_empty());
    // Without buoyancy the energy equation is off and nothing thermal is written.
    let plain = compiled(&coarse("lid-cavity"));
    assert_eq!(value(&plain, "INC_DENSITY_MODEL"), "CONSTANT");
    assert_eq!(value(&plain, "INC_ENERGY_EQUATION"), "NO");
    assert!(!has_key(&plain, "THERMAL_EXPANSION_COEFF"));
}

#[test]
fn the_run_plan_names_the_su2_step() {
    let plan = row().run_plan(&coarse("lid-cavity"), 4);
    assert_eq!(plan.solver, "SU2_CFD");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].command, "SU2_CFD -t 4 config.cfg");
    assert_eq!(plan.steps[0].log, "log.SU2");
    assert_eq!(plan.solver_log, "log.SU2");
    assert_eq!(plan.judged_logs, ["log.SU2"]);
    assert_eq!(plan.final_fields, ["history.csv", "flow.csv"]);
    // The mesh the run stands on is the file the writer makes.
    assert_eq!(row().mesh_files(&coarse("lid-cavity")), ["mesh.su2"]);
    assert!(!plan.transient && !plan.buoyant && !plan.potential);
    assert!(plan.pressure_field.is_empty() && plan.residual_checks.is_empty());
    // The time mode and the buoyancy follow the study.
    assert!(row().run_plan(&coarse("cylinder"), 1).transient);
    assert!(row().run_plan(&coarse("heated-cavity"), 1).buoyant);
    // At least one thread, whatever is asked.
    assert_eq!(
        row().run_plan(&coarse("lid-cavity"), 0).steps[0].command,
        "SU2_CFD -t 1 config.cfg"
    );
    // The files that decide what is written are named, each once, and the scan and the reader are
    // not among them.
    let names: Vec<&str> = row().writer_sources().iter().map(|(n, _)| *n).collect();
    for wanted in [
        "config.rs",
        "mesh.rs",
        "inlet.rs",
        "markers.rs",
        "export.rs",
        "plan.rs",
    ] {
        assert!(names.contains(&wanted), "{wanted}");
    }
    assert!(!names.contains(&"guard.rs") && !names.contains(&"reader.rs"));
    assert!(
        row()
            .writer_sources()
            .iter()
            .all(|(_, text)| !text.is_empty())
    );
}
