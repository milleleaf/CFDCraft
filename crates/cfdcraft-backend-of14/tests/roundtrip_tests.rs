//! The value round trip: every fact of the study is read back from the written text by a reader
//! that shares no code with the writer, and each fact is lost when its text changes.

mod common;

use cfdcraft_backend::Backend;
use cfdcraft_backend_esi::scalar;
use cfdcraft_model::{ResolvedStudy, diagnostics::Severity};
use common::{backend, cavity, compiled, edited, lost, rewritten, step, text};

fn row() -> &'static dyn Backend {
    backend()
}

/// The line of a file that starts with `key`, as the whole line.
fn line_of(tree: &cfdcraft_backend::CaseTree, name: &str, key: &str) -> String {
    text(tree, name)
        .lines()
        .find(|l| l.trim_start().starts_with(key))
        .unwrap_or_else(|| panic!("{name} has no line starting with {key}"))
        .to_owned()
}

/// The group of three cell counts in front of `simpleGrading`, and its first number.
fn first_block_cells(mesh: &str) -> (String, u64) {
    let grading = mesh.find(" simpleGrading").unwrap();
    let open = mesh[..grading].rfind('(').unwrap();
    let group = mesh[open..grading].to_owned();
    let first = group[1..].split(' ').next().unwrap().parse().unwrap();
    (group, first)
}

#[test]
fn the_value_round_trip_reads_every_fact() {
    let study = cavity();
    let tree = compiled(&study);
    assert!(
        row().read_back(&study, &tree).is_empty(),
        "{:?}",
        row().read_back(&study, &tree)
    );

    // The viscosity digit.
    let nu = format!("nu {} [m^2/s];", scalar(study.fluid.kinematic_viscosity));
    let changed = edited(&tree, "constant/physicalProperties", &nu, "nu 0.5 [m^2/s];");
    assert!(lost(&changed, &study, "/fluid/kinematic_viscosity"));

    // The speed of the lid.
    let lid = line_of(&tree, "0/U", "movingWall");
    let slower = lid.replacen("uniform (", "uniform (9", 1);
    let changed = edited(&tree, "0/U", &lid, &slower);
    assert!(lost(&changed, &study, "/velocity"));

    // The cell count.
    let mesh = text(&tree, "system/blockMeshDict");
    let (group, cells) = first_block_cells(&mesh);
    let more = group.replacen(&cells.to_string(), &(cells + 1).to_string(), 1);
    let changed = edited(&tree, "system/blockMeshDict", &group, &more);
    assert!(lost(&changed, &study, "/mesh/cells_x"));

    // The end time.
    let end = line_of(&tree, "system/controlDict", "endTime");
    let changed = edited(&tree, "system/controlDict", &end, "endTime 7;");
    assert!(lost(&changed, &study, "/solver/end_time"));
}

#[test]
fn a_lost_entry_and_a_wrong_solver_are_found() {
    let study = cavity();
    let tree = compiled(&study);
    // The write interval removed.
    let interval = line_of(&tree, "system/controlDict", "writeInterval");
    let changed = rewritten(&tree, "system/controlDict", |t| t.replace(&interval, ""));
    assert!(lost(&changed, &study, "/solver/write_interval"));
    // A time-marching scheme where the study is steady.
    let changed = edited(&tree, "system/fvSchemes", "steadyState", "Euler");
    assert!(!row().read_back(&study, &changed).is_empty());
    // A pressure reference removed: the domain would read as open.
    let reference = rewritten(&tree, "system/fvSolution", |t| {
        t.replace("    pRefCell 0;\n    pRefValue 0;\n", "")
    });
    assert!(lost(&reference, &study, "/domain/open"));
    // A file that cannot be read is an error, not a silent pass.
    let broken = rewritten(&tree, "constant/physicalProperties", |t| {
        t.replace("viscosityModel constant;", "viscosityModel constant")
    });
    let found = row().read_back(&study, &broken);
    assert!(found.iter().any(|d| d.code == "CFD-RT-002"), "{found:?}");
    assert!(found.iter().all(|d| d.severity == Severity::Error));
}

#[test]
fn the_step_facts_round_trip() {
    let study: ResolvedStudy = step();
    let tree = compiled(&study);
    assert!(
        row().read_back(&study, &tree).is_empty(),
        "{:?}",
        row().read_back(&study, &tree)
    );
    // The inlet profile coefficient.
    let inlet = line_of(&tree, "0/U", "inlet");
    let marker = "polynomial (((";
    let at = inlet.find(marker).unwrap() + marker.len();
    let changed_inlet = format!("{}9{}", &inlet[..at], &inlet[at..]);
    let changed = edited(&tree, "0/U", &inlet, &changed_inlet);
    assert!(
        !row().read_back(&study, &changed).is_empty(),
        "a changed profile must be found"
    );
    // A block cell count.
    let mesh = text(&tree, "system/blockMeshDict");
    let (group, cells) = first_block_cells(&mesh);
    let more = group.replacen(&cells.to_string(), &(cells + 1).to_string(), 1);
    let changed = edited(&tree, "system/blockMeshDict", &group, &more);
    assert!(
        !row().read_back(&study, &changed).is_empty(),
        "a changed block must be found"
    );
}

#[test]
fn every_monitor_of_the_step_is_read_back() {
    let study = step();
    let tree = compiled(&study);
    let control = text(&tree, "system/controlDict");
    assert!(control.contains("line_1"));
    // The sampled line moved by one digit.
    let start = line_of(&tree, "system/controlDict", "start (0.026");
    let changed = edited(
        &tree,
        "system/controlDict",
        &start,
        &start.replacen("0.026", "0.027", 1),
    );
    assert!(!row().read_back(&study, &changed).is_empty());
}
