mod support;

use std::fmt::Write as _;

use cfdcraft_assess::{
    Assessment, Files, ForceSpec, HeatSpec, Spec, SurfaceQuantityKind, SurfaceSpec, assess,
};
use cfdcraft_monitors::FieldKind;
use cfdcraft_settle::{Settings, VerdictState};
use support::{Shape, probe_spec, put, velocity_probe};

/// A log of one iteration holding one solve line per `(field, initial residual)`.
fn log_of(rows: &[(&str, f64)]) -> String {
    let mut text = String::from("Time = 1\n\n");
    for (field, r) in rows {
        let _ = writeln!(
            text,
            "DICPCG:  Solving for {field}, Initial residual = {r:.12e}, Final residual = {:.12e}, No Iterations 2",
            r * 1e-3
        );
    }
    text.push('\n');
    text
}

/// A spec with one settled velocity probe, so the verdict has something to judge and names its
/// reasons, and the residual limits asked for.
fn judged(limits: (f64, f64), log: String) -> (Spec, Files) {
    let spec = Spec {
        probes: vec![probe_spec("probe_1", FieldKind::Velocity)],
        residual_limits: Some(limits),
        ..Spec::default()
    };
    let mut files = Files {
        log,
        ..Files::default()
    };
    put(
        &mut files,
        "probe_1",
        velocity_probe(1500, Shape::Settles, 0.2),
    );
    (spec, files)
}

fn run(spec: &Spec, files: &Files) -> Assessment {
    assess(spec, files, &Settings::default())
}

#[test]
fn an_old_spec_deserialises_and_assesses_as_before() {
    let old = r#"{
        "probes": [],
        "lines": [],
        "surfaces": [{"fo": "surface_1", "label": "inlet flux", "quantity": "flux"}],
        "patches": [],
        "solves_energy": false,
        "residual_limits": [0.5, 0.25],
        "transient": false
    }"#;
    let spec: Spec = serde_json::from_str(old).expect("a spec written before the new members");
    assert_eq!(spec.log_file, "log.simpleFoam");
    assert_eq!(spec.pressure_field, "p");
    assert!(!spec.temperature && !spec.periodic);
    assert!(spec.forces.is_empty() && spec.heat.is_none());
    assert_eq!(spec.surfaces[0].component, None);
    assert_eq!(spec.residual_limits, Some((0.5, 0.25)));
    assert_eq!(
        spec,
        Spec {
            surfaces: vec![SurfaceSpec {
                fo: "surface_1".to_owned(),
                label: "inlet flux".to_owned(),
                quantity: SurfaceQuantityKind::Flux,
                component: None,
            }],
            residual_limits: Some((0.5, 0.25)),
            ..Spec::default()
        }
    );
    // The pressure residual is judged by name `p` and a velocity residual by its `U` prefix, as before.
    let (spec, files) = judged((1e-6, 1e-6), support::log(1500, true));
    let a = run(&spec, &files);
    assert!(a.verdict.residuals_ok);
    assert_eq!(a.verdict.state, VerdictState::Converged);
    let (spec, files) = judged((1e-12, 1e-12), support::log(1500, true));
    assert!(!run(&spec, &files).verdict.residuals_ok);
}

#[test]
fn the_pressure_field_is_named_by_the_spec() {
    let log = log_of(&[("Ux", 1e-9), ("p_rgh", 1e-3)]);
    let (mut spec, files) = judged((1e-6, 1e-6), log);
    spec.pressure_field = "p_rgh".to_owned();
    let a = run(&spec, &files);
    assert!(!a.verdict.residuals_ok);
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
    assert_eq!(a.verdict.reasons, ["residuals_above_limit"]);

    // With the default pressure field the p_rgh row is not held to the pressure limit.
    spec.pressure_field = "p".to_owned();
    let a = run(&spec, &files);
    assert!(a.verdict.residuals_ok);
    assert!(a.residuals.iter().any(|r| r.field == "p_rgh"));
}

#[test]
fn temperature_is_gated_by_the_velocity_limit_when_solved() {
    let log = log_of(&[("Ux", 1e-9), ("p_rgh", 1e-9), ("T", 1e-3)]);
    let (mut spec, files) = judged((1e-6, 1e-6), log);
    spec.pressure_field = "p_rgh".to_owned();
    spec.temperature = true;
    assert!(!run(&spec, &files).verdict.residuals_ok);

    spec.temperature = false;
    assert!(run(&spec, &files).verdict.residuals_ok);
}

#[test]
fn turbulence_fields_are_reported_and_not_gated() {
    let log = log_of(&[
        ("Ux", 1e-9),
        ("p", 1e-9),
        ("k", 1e3),
        ("omega", 1e9),
        ("epsilon", 1e3),
    ]);
    let (spec, files) = judged((1e-6, 1e-6), log);
    let a = run(&spec, &files);
    for field in ["k", "omega", "epsilon"] {
        assert!(a.residuals.iter().any(|r| r.field == field), "{field}");
    }
    assert!(a.verdict.residuals_ok);
}

#[test]
fn no_limits_is_no_check_and_limits_with_no_row_fail() {
    let log = log_of(&[("Ux", 1e9), ("p_rgh", 1e9), ("T", 1e9)]);
    let (mut spec, files) = judged((1e-6, 1e-6), log);
    spec.pressure_field = "p_rgh".to_owned();
    spec.temperature = true;
    spec.residual_limits = None;
    assert!(run(&spec, &files).verdict.residuals_ok);

    spec.residual_limits = Some((1e-6, 1e-6));
    let empty = Files {
        log: String::new(),
        ..files
    };
    assert!(!run(&spec, &empty).verdict.residuals_ok);
}

#[test]
fn the_spec_round_trips_through_json() {
    let spec = Spec {
        probes: vec![probe_spec("probe_1", FieldKind::Pressure)],
        surfaces: vec![SurfaceSpec {
            fo: "surface_1".to_owned(),
            label: "hot wall".to_owned(),
            quantity: SurfaceQuantityKind::HeatFlux,
            component: Some(1),
        }],
        solves_energy: true,
        residual_limits: Some((0.5, 0.25)),
        transient: true,
        log_file: "log.buoyantFoam".to_owned(),
        pressure_field: "p_rgh".to_owned(),
        temperature: true,
        periodic: true,
        forces: vec![ForceSpec {
            fo: "force_1".to_owned(),
            label: "cylinder".to_owned(),
        }],
        heat: Some(HeatSpec {
            hot: "surface_1".to_owned(),
            cold: "surface_2".to_owned(),
        }),
        ..Spec::default()
    };
    let text = serde_json::to_string(&spec).expect("a spec serialises");
    let back: Spec = serde_json::from_str(&text).expect("and parses");
    assert_eq!(back, spec);
}
