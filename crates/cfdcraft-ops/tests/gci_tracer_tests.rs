//! The thinnest whole path of mesh independence on the cavity: a level sweep on a document, three
//! meshes of 32, 64 and 128 cells a side, runs that recorded one centreline velocity each, and the
//! report the published worked example gives. No solver runs: the records are written as a solve
//! would leave them.

use std::path::Path;

use cfdcraft_model::{ResolvedStudy, resolve};
use cfdcraft_ops::Repro;
use cfdcraft_ops::gci_ops::{GciRequest, gci_create, gci_report};
use cfdcraft_ops::scenario_ops::effective_of;
use cfdcraft_runs::{Output, Provenance, RunRecord, StageRecipes, ToolIdentity, next_run_id};
use cfdcraft_store::ContentHash;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::{CAVITY, edit, new_document, passed, scratch};

const SWEEP: &str = "gci-cavity";
/// The centreline velocity of the Ghia comparison that the level sweep is judged on.
const QUANTITY: &str = "centreline.vertical.ux.0031";
const POINTS: [&str; 3] = ["gci-cavity-001", "gci-cavity-002", "gci-cavity-003"];
/// Coarse to fine; the three values of the published worked example.
const VALUES: [f64; 3] = [0.96178, 0.96854, 0.97050];

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn resolved_of(document: &cfdcraft_doc::Document, point: &str) -> ResolvedStudy {
    let (study, _) = effective_of("gci", document, Some(point)).unwrap();
    resolve(&study).unwrap()
}

/// Append a run for the variant `scenario` that recorded a result holding `value` for `QUANTITY`.
fn record(path: &Path, scenario: &str, value: f64) {
    let mut document = cfdcraft_doc::open(path).unwrap().document;
    let (study, tag) = effective_of("gci", &document, Some(scenario)).unwrap();
    let recipes = StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool());
    let keys = recipes.keys();
    let id = next_run_id(&document.artifacts.runs);
    let bytes = format!("output of {id}").into_bytes();
    let record = RunRecord {
        id,
        rerun_of: None,
        scenario: tag,
        mesh: None,
        result: Some(Output {
            key: keys.result,
            recipe: recipes.result.clone(),
            blob: ContentHash::of(&bytes),
            size: bytes.len() as u64,
        }),
        provenance: Provenance {
            geometry_hash: recipes.mesh.geometry.hash(),
            mesh_key: keys.mesh,
            mesh_hash: ContentHash::of(b"a mesh made earlier"),
            result_key: Some(keys.result),
            settings: study,
            tool: tool(),
            host: None,
        },
        checks: Vec::new(),
        key_numbers: [(QUANTITY.to_owned(), value)].into_iter().collect(),
        comparison: None,
        continued_from: None,
        outcome: cfdcraft_runs::Outcome::Complete,
        checkpoint: None,
        verdict: None,
        np: None,
    };
    document.add_run(record).unwrap();
    cfdcraft_doc::save(&document, path).unwrap();
}

#[test]
fn the_tracer_path_end_to_end() {
    // The cavity of the first validated run, with its 128 cells a side brought down to 32.
    let study = edit(CAVITY, |study| {
        study["mesh"]["cells_x"] = json!(32);
        study["mesh"]["cells_y"] = json!(32);
    });
    let dir = scratch("gci-tracer");
    let path = new_document(&dir, "cavity", &study);

    passed(&gci_create(&path, &GciRequest::new(SWEEP)).report);

    // Three effective studies: 1 024, 4 096 and 16 384 cells, three different meshes, and nothing
    // else about the flow changed.
    let document = cfdcraft_doc::open(&path).unwrap().document;
    let resolved: Vec<ResolvedStudy> = POINTS.iter().map(|p| resolved_of(&document, p)).collect();
    let counts: Vec<Option<u64>> = resolved.iter().map(ResolvedStudy::cell_count).collect();
    assert_eq!(counts, [Some(1_024), Some(4_096), Some(16_384)]);
    let meshes: Vec<_> = resolved
        .iter()
        .map(|r| StageRecipes::from_resolved(r, &tool()).keys().mesh)
        .collect();
    assert_ne!(meshes[0], meshes[1]);
    assert_ne!(meshes[1], meshes[2]);
    assert_ne!(meshes[0], meshes[2]);
    for pair in resolved.windows(2) {
        assert_eq!(pair[0].fluid, pair[1].fluid);
        assert_eq!(pair[0].boundary_conditions, pair[1].boundary_conditions);
        assert_eq!(pair[0].solver, pair[1].solver);
        assert_eq!(pair[0].domain, pair[1].domain);
    }

    // One recorded key number for each point, then the report.
    for (point, value) in POINTS.iter().zip(VALUES) {
        record(&path, point, value);
    }
    let out = gci_report(&path, SWEEP, QUANTITY);
    passed(&out.report);
    let Some(Repro::Gci(data)) = &out.report.repro else {
        panic!("expected the data of a mesh independence report");
    };
    let report = data.reports.last().expect("one analysis");
    let order = report.order.expect("an order of convergence");
    assert!(
        ((order - 1.78617) / 1.78617).abs() <= 1e-5,
        "the observed order is {order}"
    );
}
