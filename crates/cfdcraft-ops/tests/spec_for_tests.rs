//! What a run of a study monitors and how it is judged follows the study: the solver's log, the
//! pressure field, the periodic path of a transient run, the force coefficients and the pair of
//! wall heat flows of a run that solves energy. No test needs a compute host or a container
//! engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_assess::{ForceSpec, HeatSpec, Spec, SurfaceQuantityKind};
use cfdcraft_capture::Bundle;
use cfdcraft_ops::convergence::{files_from_bundle, files_from_dir, spec_for};

#[path = "plan_studies/mod.rs"]
mod plan_studies;
use plan_studies::*;

fn scratch(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("spec-for-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cylinder_spec() -> Spec {
    spec_for(&resolved(&cylinder_json(&cylinder_monitors(
        "cylinder_forces",
        "front",
        "rear",
    ))))
}

fn heated_spec(monitors: &str) -> Spec {
    spec_for(&resolved(&heated_json(monitors)))
}

#[test]
fn the_cavity_spec_is_unchanged() {
    let spec = spec_for(&resolved(CAVITY));
    assert_eq!(spec.log_file, "log.simpleFoam");
    assert_eq!(spec.pressure_field, "p");
    assert!(!spec.temperature);
    assert!(!spec.periodic);
    assert!(spec.forces.is_empty());
    assert_eq!(spec.heat, None);
    assert_eq!(spec.residual_limits, Some((1e-6, 1e-6)));
    assert!(!spec.transient);
    assert!(!spec.solves_energy);
    assert_eq!(spec.probes.len(), 5);
    assert!(spec.surfaces.is_empty());
}

#[test]
fn a_transient_study_has_no_residual_limit_and_is_periodic() {
    let spec = cylinder_spec();
    assert_eq!(spec.log_file, "log.pimpleFoam");
    assert!(spec.transient);
    assert!(spec.periodic);
    assert_eq!(spec.residual_limits, None);
    assert_eq!(spec.pressure_field, "p");
    assert_eq!(
        spec.forces,
        [ForceSpec {
            fo: "force_1".to_owned(),
            label: "cylinder_forces".to_owned()
        }]
    );
    let probes: Vec<(&str, &str)> = spec
        .probes
        .iter()
        .map(|p| (p.fo.as_str(), p.label.as_str()))
        .collect();
    assert_eq!(probes, [("probe_1", "front"), ("probe_2", "rear")]);
    assert!(
        spec.probes
            .iter()
            .all(|p| p.field == cfdcraft_monitors::FieldKind::Pressure)
    );
}

#[test]
fn a_buoyant_study_names_its_energy_pair() {
    let spec = heated_spec(&heated_surfaces("cold_flux", "hot_flux"));
    assert_eq!(spec.log_file, "log.buoyantBoussinesqSimpleFoam");
    assert_eq!(spec.pressure_field, "p_rgh");
    assert!(spec.temperature);
    assert!(spec.solves_energy);
    assert!(!spec.transient);
    assert_eq!(spec.residual_limits, Some((1e-6, 1e-6)));
    let surfaces: Vec<_> = spec
        .surfaces
        .iter()
        .map(|s| (s.fo.as_str(), s.label.as_str(), s.quantity, s.component))
        .collect();
    assert_eq!(
        surfaces,
        [
            (
                "surface_1",
                "cold_flux",
                SurfaceQuantityKind::HeatFlux,
                Some(0)
            ),
            (
                "surface_2",
                "hot_flux",
                SurfaceQuantityKind::HeatFlux,
                Some(0)
            ),
        ]
    );
    // The hot wall is the one at 300.5 K, the second surface after the labels are sorted.
    assert_eq!(
        spec.heat,
        Some(HeatSpec {
            hot: "surface_2".to_owned(),
            cold: "surface_1".to_owned()
        })
    );
}

#[test]
fn the_hot_wall_is_found_by_its_temperature_not_its_label() {
    // The labels sort the other way round: the hot wall is now the first surface.
    let spec = heated_spec(&heated_surfaces("b_flux", "aa_hot"));
    let first = &spec.surfaces[0];
    let second = &spec.surfaces[1];
    assert_eq!(
        (first.label.as_str(), second.label.as_str()),
        ("aa_hot", "b_flux")
    );
    assert_eq!(
        spec.heat,
        Some(HeatSpec {
            hot: first.fo.clone(),
            cold: second.fo.clone()
        })
    );
}

#[test]
fn a_buoyant_study_with_no_wall_monitors_has_no_pair() {
    let none = heated_spec("");
    assert!(none.solves_energy);
    assert_eq!(none.heat, None);
    assert!(none.surfaces.is_empty());

    let one = heated_spec(&hot_surface_only("hot_flux"));
    assert_eq!(one.surfaces.len(), 1);
    assert_eq!(one.heat, None);
}

/// The text of a force coefficient file in the shape the solver writes it.
const COEFFICIENTS: &str = "# Force coefficients\n# liftDir : (0 1 0)\n# dragDir : (1 0 0)\n# Time Cm Cd Cl Cl(f) Cl(r)\n0.5 0 3.2 0.9 0.45 0.45\n";

#[test]
fn monitor_files_include_coefficients() {
    let spec = cylinder_spec();
    let dir = scratch("coefficients");
    let folder = dir.join("postProcessing/force_1/0");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("coefficient.dat"), COEFFICIENTS).unwrap();
    let files = files_from_dir(&dir, &spec);
    assert_eq!(
        files.monitor_files["force_1"],
        [("0".to_owned(), COEFFICIENTS.to_owned())]
    );

    // The same file is read from a stored result.
    let bundle = Bundle::from_entries(vec![(
        "postProcessing/force_1/0/coefficient.dat".to_owned(),
        COEFFICIENTS.as_bytes().to_vec(),
    )])
    .unwrap();
    let files = files_from_bundle(&bundle.to_bytes().unwrap(), &spec);
    assert_eq!(
        files.monitor_files["force_1"],
        [("0".to_owned(), COEFFICIENTS.to_owned())]
    );
}

#[test]
fn monitor_files_include_heat_flux_surfaces() {
    let spec = heated_spec(&heated_surfaces("cold_flux", "hot_flux"));
    let dir = scratch("heat");
    for surface in ["surface_1", "surface_2"] {
        let folder = dir.join(format!("postProcessing/{surface}/0"));
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("surfaceFieldValue.dat"),
            format!("# {surface}\n"),
        )
        .unwrap();
    }
    let files = files_from_dir(&dir, &spec);
    assert!(files.monitor_files.contains_key("surface_1"));
    assert!(files.monitor_files.contains_key("surface_2"));
}

#[cfg(unix)]
#[test]
fn a_linked_coefficient_file_is_left_out() {
    let spec = cylinder_spec();
    let dir = scratch("linked");
    let folder = dir.join("postProcessing/force_1/0");
    fs::create_dir_all(&folder).unwrap();
    fs::write(dir.join("elsewhere.dat"), COEFFICIENTS).unwrap();
    std::os::unix::fs::symlink(dir.join("elsewhere.dat"), folder.join("coefficient.dat")).unwrap();
    let files = files_from_dir(&dir, &spec);
    assert!(!files.monitor_files.contains_key("force_1"));
}

#[test]
fn the_solver_log_comes_from_the_spec() {
    let dir = scratch("log");
    fs::write(dir.join("log.pimpleFoam"), "transient\n").unwrap();
    fs::write(dir.join("log.simpleFoam"), "steady\n").unwrap();

    let transient = cylinder_spec();
    assert_eq!(files_from_dir(&dir, &transient).log, "transient\n");
    // A spec with the default reads the steady solver's log.
    assert_eq!(files_from_dir(&dir, &Spec::default()).log, "steady\n");

    let bundle = Bundle::from_entries(vec![
        ("log.pimpleFoam".to_owned(), b"transient\n".to_vec()),
        ("log.simpleFoam".to_owned(), b"steady\n".to_vec()),
    ])
    .unwrap();
    let bytes = bundle.to_bytes().unwrap();
    assert_eq!(files_from_bundle(&bytes, &transient).log, "transient\n");
    assert_eq!(files_from_bundle(&bytes, &Spec::default()).log, "steady\n");

    // A name that is not one plain file name reads nothing.
    fs::create_dir_all(dir.join("inner")).unwrap();
    fs::write(dir.join("inner/log.simpleFoam"), "inner\n").unwrap();
    let parent = Spec {
        log_file: "../log.simpleFoam".to_owned(),
        ..Spec::default()
    };
    assert_eq!(files_from_dir(&dir.join("inner"), &parent).log, "");
    let nested = Spec {
        log_file: "inner/log.simpleFoam".to_owned(),
        ..Spec::default()
    };
    assert_eq!(files_from_dir(&dir, &nested).log, "");
}
