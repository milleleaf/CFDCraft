//! The claim list is empty, and the backend states what it is and what it can run.

use cfdcraft_backend::{Backend, Family};
use cfdcraft_backend_lbm::{
    ID, backends,
    claims::{CLAIMS, Claim, claimed, claimed_in},
};
use cfdcraft_model::{Capabilities, Study, TurbulenceModel, diagnose};
use serde_json::{Value, json};

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn backend() -> &'static dyn Backend {
    backends()[0]
}

/// The codes of the backend findings for the cavity fixture after `edit`.
fn backend_codes(edit: impl Fn(&mut Value)) -> Vec<String> {
    let mut value: Value = serde_json::from_str(CAVITY_V1).unwrap();
    edit(&mut value);
    let study = Study::from_json(&value.to_string()).unwrap();
    diagnose(&study, &backend().capabilities())
        .into_iter()
        .filter(|d| d.code.starts_with("CFD-BACKEND"))
        .map(|d| d.code)
        .collect()
}

#[test]
fn the_claim_list_is_empty_and_gates_every_case() {
    assert_eq!(CLAIMS.len(), 0);
    for (benchmark, case) in [
        ("cavity", "any"),
        ("cavity", "ghia"),
        ("cylinder", "medium"),
    ] {
        assert!(!claimed(benchmark, case));
    }
    let fixture = [
        Claim {
            benchmark: "cavity",
            case: "ghia",
            evidence: "",
        },
        Claim {
            benchmark: "cavity",
            case: "fine",
            evidence: "  ",
        },
        Claim {
            benchmark: "cavity",
            case: "coarse",
            evidence: "a recorded gate pass",
        },
    ];
    assert!(!claimed_in(&fixture, "cavity", "ghia"));
    assert!(!claimed_in(&fixture, "cavity", "fine"));
    assert!(claimed_in(&fixture, "cavity", "coarse"));
    assert!(!claimed_in(&fixture, "cylinder", "coarse"));
}

#[test]
fn the_row_states_preview_identity_and_limits() {
    let row = backend();
    assert_eq!(row.id(), "fieldcraft-lbm-0.7.5");
    assert_eq!(row.id(), ID);
    assert_eq!(row.family(), Family::Lbm);
    assert_eq!(row.image(), None);
    assert!(row.preview());
    assert_eq!(backends().len(), 1);
    assert!(!row.writer_sources().is_empty());

    let limits = row.capabilities();
    assert_eq!(limits.turbulence_models, [TurbulenceModel::Laminar]);
    assert!(limits.transient && limits.buoyancy && limits.parabolic_inlet);
    assert!(!limits.potential_init);
    assert!(limits.uniform_square_cells);
    assert!(!limits.accepts_raw);
    assert!(limits.requires_single_cell_z);

    // The plain cavity asks for nothing the backend lacks.
    assert!(backend_codes(|_| {}).is_empty());
    // A turbulence model, a raw override, graded cells and cells that are not square are named.
    let sst = backend_codes(|v| {
        v["solver"]["solver"] = "FromPhysics".into();
        v["physics"] = json!({"turbulence": {
            "model": "k_omega_sst", "intensity": 0.05, "viscosity_ratio": 10.0
        }});
    });
    assert_eq!(sst, ["CFD-BACKEND-002"]);
    let raw = backend_codes(|v| {
        v["raw_overrides"] = json!({"relax-u": {
            "backend": "esi", "file": "system/fvSolution",
            "entry": "relaxationFactors/equations/U", "value": "0.5",
        }});
    });
    assert_eq!(raw, ["CFD-BACKEND-006"]);
    assert_eq!(
        backend_codes(|v| v["mesh"]["grading"] = json!({"y": 1.2})),
        ["CFD-BACKEND-007"]
    );
    assert_eq!(
        backend_codes(|v| v["mesh"]["cells_y"] = json!(64)),
        ["CFD-BACKEND-007"]
    );
    assert_ne!(limits, Capabilities::unrestricted());
}
