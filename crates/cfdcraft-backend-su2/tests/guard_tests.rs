//! The safety scan holds every key and every value to an allowlist and every file name to three.

mod common;

use cfdcraft_backend::{Backend, CaseFile, CaseTree};
use cfdcraft_backend_su2::guard::{ALLOWED_FILES, ALLOWED_KEYS, scan_tree};
use cfdcraft_model::{BoundaryKind, ResolvedStudy};
use common::{FIVE, backend, cavity4, coarse, compiled, edited, keys, study, text};
use std::collections::BTreeSet;

/// The allowlist, pinned. Changing it is a reviewed change of this list.
const PINNED_KEYS: [&str; 62] = [
    "SOLVER",
    "KIND_TURB_MODEL",
    "MATH_PROBLEM",
    "RESTART_SOL",
    "KIND_INCOMP_SYSTEM",
    "INC_NONDIM",
    "INC_DENSITY_MODEL",
    "INC_DENSITY_INIT",
    "INC_VELOCITY_INIT",
    "INC_TEMPERATURE_INIT",
    "INC_ENERGY_EQUATION",
    "VISCOSITY_MODEL",
    "MU_CONSTANT",
    "THERMAL_EXPANSION_COEFF",
    "PRANDTL_LAM",
    "PRANDTL_TURB",
    "FREESTREAM_TURBULENCEINTENSITY",
    "FREESTREAM_TURB2LAMVISCRATIO",
    "INC_INLET_TYPE",
    "MARKER_INLET",
    "INC_OUTLET_TYPE",
    "MARKER_OUTLET",
    "SPECIFIED_INLET_PROFILE",
    "INLET_FILENAME",
    "SURFACE_MOVEMENT",
    "MARKER_MOVING",
    "SURFACE_TRANSLATION_RATE",
    "MARKER_HEATFLUX",
    "MARKER_ISOTHERMAL",
    "MARKER_EULER",
    "MARKER_SYM",
    "NUM_METHOD_GRAD",
    "CFL_NUMBER",
    "CFL_ADAPT",
    "CONV_NUM_METHOD_FLOW",
    "MUSCL_FLOW",
    "SLOPE_LIMITER_FLOW",
    "TIME_DISCRE_FLOW",
    "LINEAR_SOLVER",
    "LINEAR_SOLVER_PREC",
    "LINEAR_SOLVER_ERROR",
    "LINEAR_SOLVER_ITER",
    "ITER",
    "CONV_FIELD",
    "CONV_RESIDUAL_MINVAL",
    "CONV_STARTITER",
    "TIME_DOMAIN",
    "TIME_MARCHING",
    "TIME_STEP",
    "MAX_TIME",
    "TIME_ITER",
    "INNER_ITER",
    "OUTPUT_WRT_FREQ",
    "MESH_FILENAME",
    "MESH_FORMAT",
    "CONV_FILENAME",
    "TABULAR_FORMAT",
    "VOLUME_FILENAME",
    "OUTPUT_FILES",
    "OUTPUT_PRECISION",
    "HISTORY_OUTPUT",
    "SCREEN_OUTPUT",
];

/// The five benchmark studies and the variants that reach the keys they leave out: slip walls.
fn variants() -> Vec<ResolvedStudy> {
    let mut all: Vec<ResolvedStudy> = FIVE.iter().map(|(_, id)| coarse(id)).collect();
    let mut slip = coarse("lid-cavity");
    slip.boundary_conditions.get_mut("walls").unwrap().kind = BoundaryKind::SlipWall;
    all.push(slip);
    all
}

/// The backend behind its trait, so the trait is in scope for every call.
fn scanner() -> &'static dyn Backend {
    backend()
}

fn with_config(tree: &CaseTree, config: &str) -> CaseTree {
    let mut out = tree.clone();
    out.files[0].bytes = config.as_bytes().to_vec();
    out
}

fn codes(tree: &CaseTree) -> Vec<(&'static str, String)> {
    scan_tree(tree)
        .into_iter()
        .map(|r| (r.code, r.entry))
        .collect()
}

#[test]
fn the_allowlist_is_the_pinned_list() {
    assert_eq!(ALLOWED_KEYS, PINNED_KEYS.as_slice());
    assert_eq!(ALLOWED_FILES, ["config.cfg", "mesh.su2", "inlet.dat"]);
}

#[test]
fn every_key_the_builder_writes_is_allowed_and_every_allowed_key_is_written() {
    let mut written = BTreeSet::new();
    for variant in variants() {
        written.extend(keys(&compiled(&variant)));
    }
    let allowed: BTreeSet<String> = ALLOWED_KEYS.iter().map(|k| (*k).to_owned()).collect();
    assert_eq!(written, allowed);
}

#[test]
fn the_guard_refuses_unknown_keys_and_paths() {
    let tree = compiled(&cavity4());
    assert!(scanner().scan(&tree).is_empty(), "{:?}", codes(&tree));
    let config = text(&tree, "config.cfg");
    // A key that is not listed, wherever it stands.
    for key in [
        "SYSTEM_CALL",
        "CONFIG_LIST",
        "MESH_FILENAME_EXTRA",
        "solver",
    ] {
        let bad = with_config(&tree, &format!("{config}{key}= x\n"));
        assert_eq!(codes(&bad), [("CFD-GUARD-010", key.to_owned())], "{key}");
        let refusal = &scanner().scan(&bad)[0];
        assert_eq!(refusal.file, "config.cfg");
        assert_eq!(refusal.line as usize, config.lines().count() + 1);
    }
    // A value that holds a path or a forbidden character.
    for value in [
        "/etc/passwd",
        "../mesh.su2",
        "a/b",
        "`id`",
        "$(id)",
        "$HOME",
        "\"quoted\"",
        "tab\there",
        "one;two",
        "x\\",
        "a|b",
        "C:\\win",
    ] {
        let bad = with_config(&tree, &format!("{config}MESH_FILENAME= {value}\n"));
        let found = codes(&bad);
        assert_eq!(found.len(), 1, "{value}: {found:?}");
        assert_eq!(found[0].0, "CFD-GUARD-011", "{value}");
    }
    // A value split over two lines by a continuation: the first line carries the backslash and the
    // second line is not an entry at all.
    let split = with_config(
        &tree,
        &format!("{config}INLET_FILENAME= inlet.dat\\\n/etc/passwd\n"),
    );
    let found = codes(&split);
    assert!(
        found.iter().any(|(c, _)| *c == "CFD-GUARD-011"),
        "{found:?}"
    );
    assert!(
        found.iter().any(|(c, _)| *c == "CFD-GUARD-010"),
        "{found:?}"
    );
    // A comment and a line with no key are refused too.
    for line in ["% a comment", "SOLVER", "= value", "   "] {
        let bad = with_config(&tree, &format!("{config}{line}\n"));
        assert!(!codes(&bad).is_empty(), "{line:?}");
    }
    // Carriage returns end no line: they are control characters in the value.
    let crlf = with_config(&tree, &config.replace('\n', "\r\n"));
    assert!(codes(&crlf).iter().all(|(c, _)| *c == "CFD-GUARD-011"));
    assert!(!codes(&crlf).is_empty());
}

#[test]
fn the_mesh_and_the_profile_are_scanned_as_well() {
    let tree = compiled(&cavity4());
    let bad = edited(
        &tree,
        "mesh.su2",
        "NDIME= 2",
        "NDIME= 2\nINCLUDE= /etc/passwd",
    );
    let found = codes(&bad);
    assert_eq!(found[0].0, "CFD-GUARD-010", "{found:?}");
    assert!(found.iter().any(|(c, _)| *c == "CFD-GUARD-011"));
    let bad = edited(
        &tree,
        "mesh.su2",
        "MARKER_TAG= movingWall",
        "MARKER_TAG= ../lid",
    );
    assert_eq!(codes(&bad)[0].0, "CFD-GUARD-011");
    let bad = edited(&tree, "mesh.su2", "3 ", "3 x");
    assert_eq!(codes(&bad)[0].0, "CFD-GUARD-010");
    let step = compiled(&coarse("backward-step-laminar"));
    assert!(scan_tree(&step).is_empty());
    let bad = edited(&step, "inlet.dat", "NROW=", "NROWS=");
    assert_eq!(codes(&bad)[0].0, "CFD-GUARD-010");
    let bad = edited(&step, "inlet.dat", "NMARK= 1", "NMARK= 1\n1.0 $(id) 3");
    assert_eq!(codes(&bad)[0].0, "CFD-GUARD-010");
    let bad = edited(&step, "inlet.dat", "# COORD-X", "# COORD-Q");
    assert_eq!(codes(&bad)[0].0, "CFD-GUARD-010");
}

#[test]
fn a_case_holds_exactly_the_three_named_files() {
    let tree = compiled(&cavity4());
    for name in [
        "run.sh",
        "Allrun",
        "user.py",
        "sub/config.cfg",
        "../config.cfg",
        "inlet.dat.bak",
    ] {
        let mut bad = tree.clone();
        bad.files.push(CaseFile {
            rel_path: name.into(),
            bytes: b"x".to_vec(),
        });
        let found = scanner().scan(&bad);
        assert_eq!(found.len(), 1, "{name}");
        assert_eq!(found[0].code, "CFD-GUARD-010", "{name}");
        assert_eq!(found[0].file, name);
    }
    // The three names themselves pass when the text is clean.
    let mut ok = tree.clone();
    ok.files.push(CaseFile {
        rel_path: "inlet.dat".into(),
        bytes: b"NMARK= 1\n".to_vec(),
    });
    assert!(scanner().scan(&ok).is_empty());
}

#[test]
fn every_benchmark_case_passes_the_scan() {
    for (name, id) in FIVE {
        let tree = compiled(&coarse(id));
        assert!(
            scanner().scan(&tree).is_empty(),
            "{name}: {:?}",
            codes(&tree)
        );
    }
    // The heated cavity with its wall temperatures and a transient cylinder as well.
    let heated = study(
        "heated-cavity",
        &common::options(cfdcraft_templates::MeshPreset::Coarse),
    );
    assert!(scanner().scan(&compiled(&heated)).is_empty());
}
