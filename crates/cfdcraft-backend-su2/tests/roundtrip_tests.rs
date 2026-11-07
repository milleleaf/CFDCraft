//! The written case is read back by code that shares nothing with the writer, and a value that is
//! lost or changed is named.

mod common;

use cfdcraft_backend::{Backend, CaseFile, CaseTree};
use cfdcraft_model::{Diagnostic, ResolvedStudy, Severity};
use common::{FIVE, backend, cavity4, coarse, compiled, edited, text};

/// The backend behind its trait, so the trait is in scope for every call.
fn reader() -> &'static dyn Backend {
    backend()
}

fn read(study: &ResolvedStudy, tree: &CaseTree) -> Vec<Diagnostic> {
    reader().read_back(study, tree)
}

fn settings(found: &[Diagnostic]) -> Vec<&str> {
    found.iter().map(|d| d.setting.as_str()).collect()
}

/// One edit of the case text, and the setting that must be named for it.
fn assert_named(
    study: &ResolvedStudy,
    tree: &CaseTree,
    name: &str,
    from: &str,
    to: &str,
    setting: &str,
) {
    let found = read(study, &edited(tree, name, from, to));
    assert!(
        settings(&found).contains(&setting),
        "{from} -> {to}: {found:?}"
    );
    for d in &found {
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, "CFD-RT-001");
        assert!(!d.message.is_empty());
    }
}

#[test]
fn the_value_round_trip_reads_every_fact() {
    let study = cavity4();
    let tree = compiled(&study);
    assert!(read(&study, &tree).is_empty(), "{:?}", read(&study, &tree));
    // The lid speed, the viscosity, the cell count, the iteration count and a marker name, one at
    // a time.
    assert_named(
        &study,
        &tree,
        "config.cfg",
        "SURFACE_TRANSLATION_RATE= 0.1 0 0",
        "SURFACE_TRANSLATION_RATE= 0.2 0 0",
        "moving wall speed",
    );
    assert_named(
        &study,
        &tree,
        "config.cfg",
        "MU_CONSTANT= 0.0001",
        "MU_CONSTANT= 0.0002",
        "viscosity",
    );
    assert_named(
        &study,
        &tree,
        "mesh.su2",
        "NELEM= 16",
        "NELEM= 17",
        "cell count",
    );
    assert_named(
        &study,
        &tree,
        "config.cfg",
        "ITER= 5000",
        "ITER= 4000",
        "iteration count",
    );
    assert_named(
        &study,
        &tree,
        "mesh.su2",
        "MARKER_TAG= movingWall",
        "MARKER_TAG= lid",
        "boundary marker names",
    );
}

#[test]
fn a_lost_key_and_a_changed_kind_are_named() {
    let study = cavity4();
    let tree = compiled(&study);
    // A key that is gone is not found.
    let config = text(&tree, "config.cfg");
    let line = config.lines().find(|l| l.starts_with("ITER= ")).unwrap();
    let found = read(
        &study,
        &edited(&tree, "config.cfg", &format!("{line}\n"), ""),
    );
    let lost = found
        .iter()
        .find(|d| d.setting == "iteration count")
        .unwrap();
    assert!(lost.message.contains("was not found"), "{}", lost.message);
    // A wall that is no longer listed as a wall, and one that moves nowhere.
    assert_named(
        &study,
        &tree,
        "config.cfg",
        "MARKER_HEATFLUX= ( movingWall, 0, fixedWalls, 0 )",
        "MARKER_HEATFLUX= ( movingWall, 0 )",
        "boundary kind",
    );
    assert_named(
        &study,
        &tree,
        "config.cfg",
        "MARKER_MOVING= movingWall\n",
        "",
        "boundary kind",
    );
    assert_named(
        &study,
        &tree,
        "config.cfg",
        "SOLVER= INC_NAVIER_STOKES",
        "SOLVER= INC_EULER",
        "solver kind",
    );
    assert_named(
        &study,
        &tree,
        "config.cfg",
        "INC_DENSITY_INIT= 1\n",
        "INC_DENSITY_INIT= 2\n",
        "density",
    );
}

#[test]
fn a_broken_mesh_is_named_as_such() {
    let study = cavity4();
    let tree = compiled(&study);
    // A cell that names a node that does not exist.
    assert_named(
        &study,
        &tree,
        "mesh.su2",
        "NPOIN= 25",
        "NPOIN= 24",
        "mesh structure",
    );
    // A marker that declares more edges than it lists.
    assert_named(
        &study,
        &tree,
        "mesh.su2",
        "MARKER_ELEMS= 4",
        "MARKER_ELEMS= 5",
        "mesh structure",
    );
    // A file the reader cannot understand is one error of its own kind.
    let garbled = edited(&tree, "mesh.su2", "NELEM= 16", "NELEM= 16\nnot a line");
    let found = read(&study, &garbled);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, "CFD-RT-002");
    assert_eq!(found[0].severity, Severity::Error);
    let missing = CaseTree {
        files: vec![tree.files[0].clone()],
    };
    assert_eq!(read(&study, &missing)[0].code, "CFD-RT-002");
    let empty = CaseTree { files: Vec::new() };
    assert_eq!(read(&study, &empty)[0].code, "CFD-RT-002");
}

#[test]
fn the_time_buoyancy_and_profile_facts_are_read_back() {
    // Transient: the step, the end time, the number of steps.
    let cylinder = coarse("cylinder");
    let tree = compiled(&cylinder);
    assert!(
        read(&cylinder, &tree).is_empty(),
        "{:?}",
        read(&cylinder, &tree)
    );
    assert_named(
        &cylinder,
        &tree,
        "config.cfg",
        "TIME_STEP= 0.001",
        "TIME_STEP= 0.002",
        "time step",
    );
    assert_named(
        &cylinder,
        &tree,
        "config.cfg",
        "TIME_ITER= 8000",
        "TIME_ITER= 4000",
        "number of time steps",
    );
    assert_named(
        &cylinder,
        &tree,
        "config.cfg",
        "TIME_MARCHING= DUAL_TIME_STEPPING-2ND_ORDER",
        "TIME_MARCHING= TIME_STEPPING",
        "time marching",
    );
    // The profile: a changed speed, a missing file.
    let profile = text(&tree, "inlet.dat");
    let row = profile.lines().nth(5).unwrap().to_owned();
    let words: Vec<&str> = row.split(' ').collect();
    let changed = row.replacen(words[3], "9", 1);
    assert_named(
        &cylinder,
        &tree,
        "inlet.dat",
        &row,
        &changed,
        "inlet profile",
    );
    let without = CaseTree {
        files: tree
            .files
            .iter()
            .filter(|f| f.rel_path.to_str() != Some("inlet.dat"))
            .cloned()
            .collect::<Vec<CaseFile>>(),
    };
    assert!(settings(&read(&cylinder, &without)).contains(&"inlet profile"));
    // Buoyancy: the product of gravity and expansion, the temperature, the energy equation.
    let heated = coarse("heated-cavity");
    let tree = compiled(&heated);
    assert!(
        read(&heated, &tree).is_empty(),
        "{:?}",
        read(&heated, &tree)
    );
    assert_named(
        &heated,
        &tree,
        "config.cfg",
        "INC_TEMPERATURE_INIT= 300",
        "INC_TEMPERATURE_INIT= 310",
        "buoyancy reference temperature",
    );
    assert_named(
        &heated,
        &tree,
        "config.cfg",
        "MARKER_ISOTHERMAL= ( isothermal1, 299.5, isothermal2, 300.5 )",
        "MARKER_ISOTHERMAL= ( isothermal1, 299.5, isothermal2, 301.5 )",
        "wall temperature",
    );
    assert_named(
        &heated,
        &tree,
        "config.cfg",
        "INC_ENERGY_EQUATION= YES",
        "INC_ENERGY_EQUATION= NO",
        "energy equation",
    );
    let config = text(&tree, "config.cfg");
    let coefficient = config
        .lines()
        .find(|l| l.starts_with("THERMAL_EXPANSION_COEFF= "))
        .unwrap();
    assert_named(
        &heated,
        &tree,
        "config.cfg",
        coefficient,
        "THERMAL_EXPANSION_COEFF= 0.003",
        "buoyancy (gravity times expansion coefficient)",
    );
    // Turbulence: the inflow intensity.
    let step = coarse("backward-step");
    let tree = compiled(&step);
    assert!(read(&step, &tree).is_empty(), "{:?}", read(&step, &tree));
    assert_named(
        &step,
        &tree,
        "config.cfg",
        "FREESTREAM_TURB2LAMVISCRATIO= 0.0003846",
        "FREESTREAM_TURB2LAMVISCRATIO= 3",
        "turbulence viscosity ratio",
    );
}

#[test]
fn every_benchmark_round_trips() {
    for (name, id) in FIVE {
        let study = coarse(id);
        let tree = compiled(&study);
        assert!(
            read(&study, &tree).is_empty(),
            "{name}: {:?}",
            read(&study, &tree)
        );
    }
}
