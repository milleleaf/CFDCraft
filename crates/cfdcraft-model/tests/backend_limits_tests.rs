use cfdcraft_model::diagnostics::{ALL_KINDS, ALL_TURBULENCE_MODELS, CODES, Severity};
use cfdcraft_model::{
    Capabilities, Diagnostic, RawBackend, Study, TurbulenceModel, diagnose, is_known_code,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("fixtures/channel_v1.json");

fn value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn study(v: &Value) -> Study {
    Study::from_json(&v.to_string()).unwrap()
}

fn seconds(v: f64) -> Value {
    json!({"value": v, "unit": "s"})
}

fn kelvin(v: f64) -> Value {
    json!({"value": v, "unit": "K"})
}

/// The channel study with its solver following the physics, so other physics is allowed.
fn channel() -> Value {
    let mut v = value(CHANNEL);
    v["solver"]["solver"] = "FromPhysics".into();
    v
}

/// The cavity run as a transient study of 10 s in steps of 1 ms.
fn transient() -> Value {
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["solver"]["end_time"] = seconds(10.0);
    v["solver"]["write_interval"] = seconds(1.0);
    v["physics"] = json!({"time": {"mode": "transient", "delta_t": seconds(0.001)}});
    v
}

/// A closed cavity heated from the left and cooled from the right, with buoyancy on.
fn heated() -> Value {
    let mut v = value(CAVITY);
    v["solver"]["solver"] = "FromPhysics".into();
    v["boundary_conditions"] = json!({
        "hot": {"faces": ["XMin"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": kelvin(300.5)}},
        "cold": {"faces": ["XMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": kelvin(299.5)}},
        "adiabatic": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "adiabatic"}}
    });
    v["physics"] = json!({"buoyancy": {
        "gravity": 9.81,
        "beta": 0.003,
        "reference_temperature": kelvin(300.0),
        "prandtl": 0.71,
        "turbulent_prandtl": 0.85
    }});
    v
}

/// What the ESI writer states, spelled out because the model crate cannot see the backend.
fn esi_like() -> Capabilities {
    Capabilities {
        boundary_kinds: ALL_KINDS.to_vec(),
        requires_single_cell_z: true,
        same_kind_boundaries_must_agree: true,
        raw_backend: Some(RawBackend::Esi),
        ..Capabilities::unrestricted()
    }
}

fn backend_findings(study: &Study, capabilities: &Capabilities) -> Vec<Diagnostic> {
    diagnose(study, capabilities)
        .into_iter()
        .filter(|d| d.code.starts_with("CFD-BACKEND"))
        .collect()
}

fn only(found: &[Diagnostic], code: &str, pointer: &str) {
    assert_eq!(found.len(), 1, "{found:?}");
    let d = &found[0];
    assert_eq!((d.code.as_str(), d.pointer.as_str()), (code, pointer));
    assert_eq!(d.severity, Severity::Error);
    assert!(!d.message.is_empty());
}

#[test]
fn a_transient_study_is_refused_by_a_steady_only_backend() {
    let v = transient();
    let steady_only = Capabilities {
        transient: false,
        ..Capabilities::unrestricted()
    };
    only(
        &backend_findings(&study(&v), &steady_only),
        "CFD-BACKEND-003",
        "/physics/time",
    );
    assert!(backend_findings(&study(&v), &Capabilities::unrestricted()).is_empty());
    assert!(backend_findings(&study(&v), &esi_like()).is_empty());
    // A steady study is fine on the steady-only backend.
    assert!(backend_findings(&study(&value(CAVITY)), &steady_only).is_empty());
}

#[test]
fn the_old_capability_literals_still_mean_the_same() {
    for v in [value(CAVITY), heated(), transient(), channel()] {
        assert!(backend_findings(&study(&v), &esi_like()).is_empty());
        assert!(backend_findings(&study(&v), &Capabilities::unrestricted()).is_empty());
    }
}

#[test]
fn each_limit_has_its_own_code() {
    // A turbulence model the backend does not list.
    let mut v = channel();
    v["physics"] = json!({"turbulence": {
        "model": "k_omega_sst", "intensity": 0.05, "viscosity_ratio": 10.0
    }});
    let laminar_only = Capabilities {
        turbulence_models: vec![TurbulenceModel::Laminar],
        ..Capabilities::unrestricted()
    };
    only(
        &backend_findings(&study(&v), &laminar_only),
        "CFD-BACKEND-002",
        "/physics/turbulence/model",
    );
    // Buoyancy.
    let no_buoyancy = Capabilities {
        buoyancy: false,
        ..Capabilities::unrestricted()
    };
    only(
        &backend_findings(&study(&heated()), &no_buoyancy),
        "CFD-BACKEND-004",
        "/physics/buoyancy",
    );
    // Potential-flow start.
    let mut v = channel();
    v["physics"] = json!({"init": "potential"});
    let no_potential = Capabilities {
        potential_init: false,
        ..Capabilities::unrestricted()
    };
    only(
        &backend_findings(&study(&v), &no_potential),
        "CFD-BACKEND-005",
        "/physics/init",
    );
    // Raw overrides name the override label.
    let mut v = value(CAVITY);
    v["raw_overrides"] = json!({"relax-u": {
        "backend": "esi", "file": "system/fvSolution",
        "entry": "relaxationFactors/equations/U", "value": "0.5",
    }});
    let no_raw = Capabilities {
        accepts_raw: false,
        ..Capabilities::unrestricted()
    };
    let found = backend_findings(&study(&v), &no_raw);
    only(&found, "CFD-BACKEND-006", "/raw_overrides/relax-u");
    assert!(found[0].message.contains("relax-u"), "{}", found[0].message);
    // A parabolic inlet.
    let mut v = channel();
    v["boundary_conditions"]["inlet"]["profile"] = "parabolic".into();
    let flat_only = Capabilities {
        parabolic_inlet: false,
        ..Capabilities::unrestricted()
    };
    only(
        &backend_findings(&study(&v), &flat_only),
        "CFD-BACKEND-012",
        "/boundary_conditions/inlet/profile",
    );
    // Equal square cells: graded cells, and cells that are not square.
    let square = Capabilities {
        uniform_square_cells: true,
        ..Capabilities::unrestricted()
    };
    let mut v = value(CAVITY);
    v["mesh"]["grading"] = json!({"y": 1.2});
    only(
        &backend_findings(&study(&v), &square),
        "CFD-BACKEND-007",
        "/mesh/grading",
    );
    let found = backend_findings(&study(&channel()), &square);
    only(&found, "CFD-BACKEND-007", "/mesh");
    assert!(
        found[0].message.contains("0.010000") && found[0].message.contains("0.005000"),
        "{}",
        found[0].message
    );
    assert!(backend_findings(&study(&value(CAVITY)), &square).is_empty());
    // A shaped study's counts follow its block layout: no cell-shape finding, grading still counts.
    let length = |metres: f64| json!({"value": metres, "unit": "m"});
    let mut v = channel();
    v["domain"]["shape"] = json!({
        "kind": "cylinder_channel",
        "diameter": length(0.1),
        "centre_x": length(0.2),
        "centre_y": length(0.2),
    });
    assert!(backend_findings(&study(&v), &square).is_empty());
    v["mesh"]["grading"] = json!({"y": 1.2});
    only(
        &backend_findings(&study(&v), &square),
        "CFD-BACKEND-007",
        "/mesh/grading",
    );
    // Every limit is quiet when it is permissive.
    let mut v = heated();
    v["physics"]["init"] = "potential".into();
    assert!(backend_findings(&study(&v), &Capabilities::unrestricted()).is_empty());
}

#[test]
fn the_catalogue_holds_every_code_of_the_phase() {
    let new_codes: Vec<&str> = [
        "CFD-BACKEND-001",
        "CFD-BACKEND-002",
        "CFD-BACKEND-003",
        "CFD-BACKEND-004",
        "CFD-BACKEND-005",
        "CFD-BACKEND-006",
        "CFD-BACKEND-007",
        "CFD-BACKEND-008",
        "CFD-BACKEND-009",
        "CFD-BACKEND-010",
        "CFD-BACKEND-011",
        "CFD-BACKEND-012",
        "CFD-IMPORT-001",
        "CFD-IMPORT-002",
        "CFD-IMPORT-003",
        "CFD-IMPORT-004",
        "CFD-IMPORT-005",
        "CFD-IMPORT-006",
        "CFD-IMPORT-007",
        "CFD-PARITY-001",
        "CFD-PARITY-002",
        "CFD-PARITY-003",
        "CFD-PARITY-004",
        "CFD-PARITY-005",
        "CFD-PARITY-006",
        "CFD-GUARD-010",
        "CFD-GUARD-011",
        "CFD-GUARD-012",
        "CFD-GUARD-013",
        "CFD-GUARD-014",
        "CFD-GUARD-015",
    ]
    .to_vec();
    assert_eq!(new_codes.len(), 31);
    // They follow the existing entries, in this order.
    let tail: Vec<&str> = CODES[CODES.len() - 31..].iter().map(|(c, _)| *c).collect();
    assert_eq!(tail, new_codes);
    for (code, title) in &CODES[CODES.len() - 31..] {
        assert!(!title.is_empty() && is_known_code(code), "{code}");
    }
    assert_eq!(ALL_TURBULENCE_MODELS.len(), 4);
}

#[test]
fn a_study_that_asks_for_nothing_special_triggers_no_backend_code() {
    let cavity = study(&value(CAVITY));
    for capabilities in [Capabilities::unrestricted(), esi_like()] {
        let found = diagnose(&cavity, &capabilities);
        assert!(
            found.iter().all(|d| !d.code.starts_with("CFD-BACKEND")),
            "{found:?}"
        );
    }
}
