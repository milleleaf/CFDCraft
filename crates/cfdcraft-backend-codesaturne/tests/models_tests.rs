mod common;

use cfdcraft_backend::{CompileFailure, CompileOptions};
use cfdcraft_backend_codesaturne::xml::Element;
use cfdcraft_model::{InitMode, Severity, TurbulenceModel, diagnose, has_errors};
use cfdcraft_templates::{MeshPreset, TemplateOptions, TimeChoice, build};
use common::{
    backend, cavity, coarse, compiled, edited, number_of, options, resolved, setup_root,
    with_number_changed,
};

fn inlet(root: &Element) -> &Element {
    root.child("boundary_conditions")
        .unwrap()
        .child("inlet")
        .unwrap()
}

/// The kinetic energy and the specific dissipation rate the solver derives from the intensity (in
/// percent), the hydraulic diameter and the speed written at an inlet, from its published
/// relations: k = 1.5 (I U)^2, eps = 10 Cmu^0.75 k^1.5 / (kappa D), omega = eps / (Cmu k).
fn derived(percent: f64, diameter: f64, speed: f64) -> (f64, f64) {
    let (cmu, kappa) = (0.09_f64, 0.42_f64);
    let k = 1.5 * (percent / 100.0 * speed).powi(2);
    let eps = 10.0 * cmu.powf(0.75) * k.powf(1.5) / (kappa * diameter);
    (k, eps / (cmu * k))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

#[test]
fn the_heated_cavity_is_refused_by_name_because_the_expansion_coefficient_has_no_node() {
    let heated = build("heated-cavity", &TemplateOptions::default()).unwrap();
    let caps = backend().capabilities();
    let found = diagnose(&heated, &caps);
    assert!(
        found
            .iter()
            .any(|d| d.code == "CFD-BACKEND-004" && d.severity == Severity::Error),
        "{found:?}"
    );
    // A writer that is asked anyway refuses by name; nothing is dropped.
    let study = resolved("heated-cavity", &options(MeshPreset::Coarse));
    match backend().compile(&study, &CompileOptions::default()) {
        Err(CompileFailure::NotSupported { setting, .. }) => assert_eq!(setting, "buoyancy"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_sst_study_selects_the_sst_model() {
    let study = coarse("backward-step");
    let tree = compiled(&study);
    let root = setup_root(&tree);
    let turbulence = root.find(&["thermophysical_models", "turbulence"]).unwrap();
    assert_eq!(turbulence.attr("model"), Some("k-omega-SST"));
    let t = &study.physics.as_ref().unwrap().turbulence;
    let speed = study
        .boundary_conditions
        .values()
        .find_map(|b| b.velocity)
        .unwrap();
    assert!((number_of(turbulence, "reference_velocity") - speed).abs() <= 1e-12);

    // The inlet holds constants whose derived values are the study's inflow values.
    let values = cfdcraft_model::inlet_turbulence(
        t.intensity.unwrap(),
        t.viscosity_ratio.unwrap(),
        speed,
        study.fluid.kinematic_viscosity,
    )
    .unwrap();
    let inlet = inlet(&root);
    let norm = number_of(inlet, "norm");
    assert!((norm - speed).abs() <= 1e-12, "{norm}");
    let written = inlet.child("turbulence").unwrap();
    assert_eq!(written.attr("choice"), Some("turbulent_intensity"));
    let percent = number_of(written, "turbulent_intensity");
    let diameter = number_of(written, "hydraulic_diameter");
    assert!(percent > 0.0 && diameter > 0.0);
    let (k, omega) = derived(percent, diameter, norm);
    assert!(close(k, values.k), "{k} {}", values.k);
    assert!(close(omega, values.omega), "{omega} {}", values.omega);

    // No formula-like node appears anywhere.
    let mut words = 0;
    root.walk(&mut |e| {
        words += 1;
        let mut text = vec![e.name.to_lowercase(), e.text.to_lowercase()];
        text.extend(
            e.attributes
                .iter()
                .map(|(k, v)| format!("{k} {v}").to_lowercase()),
        );
        for banned in ["formula", "mei", "compute", "user", "function"] {
            assert!(
                text.iter().all(|t| !t.contains(banned)),
                "{} holds {banned}",
                e.name
            );
        }
    });
    assert!(words > 20);

    // A laminar study writes the laminar choice and no turbulence values at all.
    let laminar = setup_root(&compiled(&cavity(4)));
    let off = laminar
        .find(&["thermophysical_models", "turbulence"])
        .unwrap();
    assert_eq!(off.attr("model"), Some("off"));
    assert!(off.children.is_empty());
}

#[test]
fn a_transient_study_writes_the_time_nodes() {
    let transient = TemplateOptions {
        time: Some(TimeChoice::Transient),
        ..options(MeshPreset::Coarse)
    };
    let study = resolved("lid-cavity", &transient);
    let dt = study.effective_delta_t().unwrap();
    let root = setup_root(&compiled(&study));
    let time = root.find(&["analysis_control", "time_parameters"]).unwrap();
    assert_eq!(time.child("time_passing").unwrap().text, "0");
    let step: f64 = time.child("time_step_ref").unwrap().text.parse().unwrap();
    assert_eq!(step.to_bits(), dt.to_bits());
    // The count is the end time over the step, rounded to a whole number.
    let count: f64 = time.child("iterations").unwrap().text.parse().unwrap();
    assert!((count - (study.solver.end_time / dt).round()).abs() < 0.5);
    assert_eq!(
        root.find(&["numerical_parameters", "velocity_pressure_algo"])
            .and_then(|a| a.attr("choice")),
        Some("simplec")
    );

    // A steady study has no step and the steady coupling.
    let steady = setup_root(&compiled(&cavity(4)));
    let time = steady
        .find(&["analysis_control", "time_parameters"])
        .unwrap();
    assert_eq!(time.child("time_passing").unwrap().text, "-1");
    assert!(time.child("time_step_ref").is_none());
    assert_eq!(
        steady
            .find(&["numerical_parameters", "velocity_pressure_algo"])
            .and_then(|a| a.attr("choice")),
        Some("simple")
    );
}

#[test]
fn the_guard_still_passes_every_written_setup() {
    let transient = TemplateOptions {
        time: Some(TimeChoice::Transient),
        ..options(MeshPreset::Coarse)
    };
    let studies = [
        ("cavity", cavity(4)),
        ("transient cavity", resolved("lid-cavity", &transient)),
        ("sst step", coarse("backward-step")),
    ];
    for (what, study) in &studies {
        let tree = compiled(study);
        assert!(backend().scan(&tree).is_empty(), "{what}");
        let found = backend().read_back(study, &tree);
        assert!(found.is_empty(), "{what}: {found:?}");
    }

    // Each value the read-back checks is a finding when the written case no longer holds it.
    let step = coarse("backward-step");
    let tree = compiled(&step);
    for (what, tag) in [
        ("hydraulic diameter", "hydraulic_diameter"),
        ("turbulence intensity", "turbulent_intensity"),
        ("inlet speed", "norm"),
        ("reference velocity", "reference_velocity"),
        ("a monitoring point", "probe_x"),
        ("the recording frequency", "probe_recording_frequency"),
    ] {
        let changed = with_number_changed(&tree, tag, |v| v * 1.5);
        let found = backend().read_back(&step, &changed);
        assert!(
            found.iter().any(|d| d.severity == Severity::Error),
            "{what} was changed and nothing was found"
        );
    }
    let transient = resolved("lid-cavity", &transient);
    let tree = compiled(&transient);
    for (what, tag) in [
        ("time step", "time_step_ref"),
        ("iteration count", "iterations"),
    ] {
        let changed = with_number_changed(&tree, tag, |v| v + 1.0);
        assert!(
            !backend().read_back(&transient, &changed).is_empty(),
            "{what} was changed and nothing was found"
        );
    }
    let model = edited(&compiled(&step), "DATA/setup.xml", "k-omega-SST", "off");
    assert!(!backend().read_back(&step, &model).is_empty());
}

#[test]
fn the_limits_now_offered() {
    let caps = backend().capabilities();
    assert_eq!(
        caps.turbulence_models,
        [TurbulenceModel::Laminar, TurbulenceModel::KOmegaSst]
    );
    assert!(caps.transient);
    assert!(
        !caps.buoyancy,
        "the pinned release has no node for the coefficient"
    );
    assert!(!caps.potential_init);
    assert!(!caps.parabolic_inlet);

    let code_of = |options: &TemplateOptions, id: &str, code: &str| {
        let study = build(id, options).unwrap();
        diagnose(&study, &caps).iter().any(|d| d.code == code)
    };
    for model in [TurbulenceModel::RealizableKe, TurbulenceModel::RngKEpsilon] {
        let asked = TemplateOptions {
            turbulence: Some(model),
            ..TemplateOptions::default()
        };
        assert!(
            code_of(&asked, "lid-cavity", "CFD-BACKEND-002"),
            "{model:?}"
        );
    }
    let potential = TemplateOptions {
        init: Some(InitMode::Potential),
        ..TemplateOptions::default()
    };
    assert!(code_of(&potential, "lid-cavity", "CFD-BACKEND-005"));
    // What is offered raises no limit.
    let offered = TemplateOptions {
        turbulence: Some(TurbulenceModel::KOmegaSst),
        time: Some(TimeChoice::Transient),
        init: Some(InitMode::Uniform),
        ..TemplateOptions::default()
    };
    let study = build("lid-cavity", &offered).unwrap();
    let found = diagnose(&study, &caps);
    assert!(!has_errors(&found), "{found:?}");
}
