//! The five benchmark studies compile to the files of their golden folders, round trip and pass
//! the scan.
//!
//! A golden folder holds the configuration, the inlet profile where there is one, and a summary of
//! the mesh (its counts, its extent, its area and its markers): the mesh text of a real benchmark
//! is far too long to review as a file, and the full text of a small mesh is pinned separately.
//! A difference prints the whole actual text so it can be pasted into the golden file as it is.

mod common;

use cfdcraft_backend::{Backend, CaseTree};
use cfdcraft_model::ResolvedStudy;
use common::{FIVE, backend, cavity4, coarse, compiled, parse_mesh, text};
use std::path::Path;

/// The backend behind its trait, so the trait is in scope for every call.
fn row() -> &'static dyn Backend {
    backend()
}

fn golden(folder: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(folder)
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A difference is collected, not raised, so one run prints every differing file.
fn assert_same(misses: &mut Vec<String>, folder: &str, name: &str, actual: &str) {
    if actual != golden(folder, name) {
        misses.push(format!(
            "{folder}/{name} differs from its golden file; the actual text is:\n{actual}"
        ));
    }
}

/// The facts of the mesh a reviewer can check against the shape: counts, extent, area, markers.
fn summary(tree: &CaseTree) -> String {
    let mesh = parse_mesh(&text(tree, "mesh.su2"));
    let area: f64 = (0..mesh.cells.len()).map(|i| mesh.area(i)).sum();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for node in &mesh.nodes {
        lo = [lo[0].min(node[0]), lo[1].min(node[1])];
        hi = [hi[0].max(node[0]), hi[1].max(node[1])];
    }
    let mut out = format!(
        "nodes {}\ncells {}\nx {:.6} {:.6}\ny {:.6} {:.6}\narea {:.9}\n",
        mesh.nodes.len(),
        mesh.cells.len(),
        lo[0],
        hi[0],
        lo[1],
        hi[1],
        area
    );
    for (tag, edges) in &mesh.markers {
        out.push_str(&format!("marker {tag} {}\n", edges.len()));
    }
    out
}

fn has_file(tree: &CaseTree, name: &str) -> bool {
    tree.files.iter().any(|f| f.rel_path.to_str() == Some(name))
}

fn parabolic(study: &ResolvedStudy) -> bool {
    study
        .boundary_conditions
        .values()
        .any(|b| b.profile == Some(cfdcraft_model::InletProfile::Parabolic))
}

#[test]
fn all_five_benchmarks_round_trip_and_match_their_goldens() {
    let mut misses = Vec::new();
    for (folder, id) in FIVE {
        let study = coarse(id);
        let tree = compiled(&study);
        // Structural facts first, so a golden that is still a placeholder is not the only check.
        let names: Vec<&str> = tree
            .files
            .iter()
            .map(|f| f.rel_path.to_str().unwrap())
            .collect();
        let expected: &[&str] = if parabolic(&study) {
            &["config.cfg", "mesh.su2", "inlet.dat"]
        } else {
            &["config.cfg", "mesh.su2"]
        };
        assert_eq!(names, expected, "{folder}");
        assert!(row().scan(&tree).is_empty(), "{folder}");
        assert!(
            row().read_back(&study, &tree).is_empty(),
            "{folder}: {:?}",
            row().read_back(&study, &tree)
        );
        let config = text(&tree, "config.cfg");
        assert!(config.starts_with("SOLVER= INC_"), "{folder}");
        assert!(config.ends_with("OUTPUT_PRECISION= 10\n"), "{folder}");
        assert_same(&mut misses, folder, "config.cfg", &config);
        if has_file(&tree, "inlet.dat") {
            assert_same(&mut misses, folder, "inlet.dat", &text(&tree, "inlet.dat"));
        }
        assert_same(&mut misses, folder, "mesh.summary", &summary(&tree));
    }
    assert!(misses.is_empty(), "{}", misses.join("\n=====\n"));
}

#[test]
fn the_small_cavity_mesh_matches_its_golden() {
    let tree = compiled(&cavity4());
    let mut misses = Vec::new();
    assert_same(
        &mut misses,
        "cavity4-su2",
        "mesh.su2",
        &text(&tree, "mesh.su2"),
    );
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

#[test]
fn the_golden_folders_hold_nothing_else() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut folders: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    folders.sort();
    let mut wanted: Vec<String> = FIVE.iter().map(|(f, _)| (*f).to_owned()).collect();
    wanted.push("cavity4-su2".into());
    wanted.sort();
    assert_eq!(folders, wanted);
    for (folder, id) in FIVE {
        let mut files: Vec<String> = std::fs::read_dir(root.join(folder))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        let mut wanted = vec!["config.cfg".to_owned(), "mesh.summary".to_owned()];
        if parabolic(&coarse(id)) {
            wanted.insert(1, "inlet.dat".to_owned());
        }
        assert_eq!(files, wanted, "{folder}");
    }
}
