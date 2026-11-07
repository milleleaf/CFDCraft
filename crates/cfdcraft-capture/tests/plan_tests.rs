use std::path::{Path, PathBuf};

use cfdcraft_capture::fixture::{FixtureSpec, write_results, write_results_for};
use cfdcraft_capture::{
    CaptureError, CapturePlan, checkpoint_bundle, checkpoint_bundle_with, key_numbers,
    key_numbers_with, monitor_names, result_bundle, result_bundle_with,
};

fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("capture-plan-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn spec(end_time: u64) -> FixtureSpec {
    FixtureSpec {
        end_time,
        cells: 64,
        samples: 4,
        shift: 0.0,
    }
}

fn buoyant() -> CapturePlan {
    let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect();
    CapturePlan {
        log: "log.buoyantBoussinesqPimpleFoam".to_owned(),
        fields: names(&["U", "p_rgh", "T"]),
        centreline: false,
        residual_fields: names(&["Ux", "Uy", "p_rgh", "T"]),
        ..CapturePlan::cavity()
    }
}

fn buoyant_results(test: &str, end_time: u64) -> PathBuf {
    let dir = scratch(test);
    write_results_for(&dir, &spec(end_time), &buoyant()).unwrap();
    dir
}

#[test]
fn the_cavity_plan_is_the_old_behaviour() {
    let dir = scratch("cavity");
    write_results(&dir, &spec(5000)).unwrap();
    std::fs::write(dir.join("5000/phi"), "flux\n").unwrap();
    let plan = CapturePlan::cavity();

    let old = result_bundle(&dir, 5000.0).unwrap();
    let new = result_bundle_with(&dir, 5000.0, &plan).unwrap();
    assert_eq!(old.hash().unwrap(), new.hash().unwrap());
    assert_eq!(old.to_bytes().unwrap(), new.to_bytes().unwrap());

    let old = key_numbers(&dir, 5000.0).unwrap();
    let new = key_numbers_with(&dir, 5000.0, &plan).unwrap();
    assert_eq!(old, new);
    assert!(old.contains_key("residual.Uy"));
    assert!(old.contains_key("centreline.vertical.ux.0000"));
}

#[test]
fn a_buoyant_transient_plan_is_captured() {
    let dir = buoyant_results("buoyant", 10);
    let plan = buoyant();
    let bundle = result_bundle_with(&dir, 10.0, &plan).unwrap();
    assert_eq!(
        bundle.names(),
        [
            "10/T",
            "10/U",
            "10/p_rgh",
            "log.blockMesh",
            "log.buoyantBoussinesqPimpleFoam",
            "log.checkMesh",
        ]
    );

    let numbers = key_numbers_with(&dir, 10.0, &plan).unwrap();
    assert!((numbers["solver.iterations"] - 10.0).abs() < 1e-9);
    for name in ["Ux", "Uy", "p_rgh", "T"] {
        assert!(numbers.contains_key(&format!("residual.{name}")), "{name}");
    }
    assert!((numbers["residual.p_rgh"] - 3e-7).abs() < 1e-12);
    assert!(!numbers.keys().any(|k| k.starts_with("centreline.")));
    assert_eq!(numbers.len(), 5);
}

#[test]
fn a_missing_plan_field_is_refused_by_name() {
    let dir = buoyant_results("missing-field", 10);
    std::fs::remove_file(dir.join("10/T")).unwrap();
    let error = result_bundle_with(&dir, 10.0, &buoyant()).unwrap_err();
    assert!(matches!(error, CaptureError::Missing(_)), "{error:?}");
    assert!(error.to_string().contains("10/T"), "{error}");

    let dir = buoyant_results("missing-residual", 10);
    let mut plan = buoyant();
    plan.residual_fields.push("k".to_owned());
    let error = key_numbers_with(&dir, 10.0, &plan).unwrap_err();
    let text = error.to_string();
    assert!(
        text.contains("log.buoyantBoussinesqPimpleFoam") && text.contains("for k"),
        "{text}"
    );
}

#[test]
fn the_potential_flow_log_is_kept_when_present() {
    let dir = buoyant_results("potential", 10);
    let plan = buoyant();
    assert!(
        !result_bundle_with(&dir, 10.0, &plan)
            .unwrap()
            .names()
            .contains(&"log.potentialFoam")
    );
    std::fs::write(dir.join("log.potentialFoam"), "End\n").unwrap();
    assert!(
        result_bundle_with(&dir, 10.0, &plan)
            .unwrap()
            .names()
            .contains(&"log.potentialFoam")
    );
}

#[cfg(unix)]
#[test]
fn links_and_oversized_files_are_still_refused() {
    let dir = buoyant_results("link", 10);
    std::fs::rename(dir.join("10/T"), dir.join("T.real")).unwrap();
    std::os::unix::fs::symlink(dir.join("T.real"), dir.join("10/T")).unwrap();
    let error = result_bundle_with(&dir, 10.0, &buoyant()).unwrap_err();
    assert!(matches!(error, CaptureError::NotAFile(_)), "{error:?}");
    assert!(error.to_string().contains("10/T"), "{error}");

    let dir = buoyant_results("oversized", 10);
    let plan = buoyant();
    std::fs::write(dir.join(&plan.log), vec![b'x'; 4096]).unwrap();
    let error = cfdcraft_capture::result_bundle_limited_with(&dir, 10.0, 1024, &plan).unwrap_err();
    assert!(matches!(error, CaptureError::TooLarge(_)), "{error:?}");
    assert!(error.to_string().contains(&plan.log), "{error}");
}

#[test]
fn force_folders_are_monitor_files() {
    let dir = buoyant_results("force", 10);
    for (folder, file) in [
        ("force_1/0", "coefficient.dat"),
        ("force_1/0.5", "coefficient.dat"),
        ("other_1/0", "coefficient.dat"),
    ] {
        let at = dir.join("postProcessing").join(folder);
        std::fs::create_dir_all(&at).unwrap();
        std::fs::write(at.join(file), "# Time Cd Cl\n0 1 2\n").unwrap();
    }
    let expected = [
        "postProcessing/force_1/0.5/coefficient.dat",
        "postProcessing/force_1/0/coefficient.dat",
    ];
    assert_eq!(monitor_names(&dir).unwrap(), expected);
    let bundle = result_bundle_with(&dir, 10.0, &buoyant()).unwrap();
    for name in expected {
        assert!(bundle.names().contains(&name), "{name}");
    }
    assert!(!bundle.names().iter().any(|n| n.contains("other_1")));
}

#[test]
fn a_checkpoint_takes_the_plan() {
    let dir = buoyant_results("checkpoint", 5);
    let at = dir.join("postProcessing/probe_1/0");
    std::fs::create_dir_all(&at).unwrap();
    std::fs::write(at.join("p"), "0 1\n").unwrap();
    let bundle = checkpoint_bundle_with(&dir, "5", &buoyant()).unwrap();
    assert_eq!(
        bundle.names(),
        [
            "5/T",
            "5/U",
            "5/p_rgh",
            "log.blockMesh",
            "log.buoyantBoussinesqPimpleFoam",
            "log.checkMesh",
            "postProcessing/probe_1/0/p",
        ]
    );

    let dir = scratch("checkpoint-cavity");
    write_results(&dir, &spec(5)).unwrap();
    let names = checkpoint_bundle(&dir, "5").unwrap();
    assert_eq!(
        names.names(),
        [
            "5/U",
            "5/p",
            "log.blockMesh",
            "log.checkMesh",
            "log.simpleFoam"
        ]
    );
    let same = checkpoint_bundle_with(&dir, "5", &CapturePlan::cavity()).unwrap();
    assert_eq!(names.hash().unwrap(), same.hash().unwrap());
}

#[cfg(unix)]
#[test]
fn a_force_file_that_is_a_link_is_refused() {
    let dir = buoyant_results("force-link", 10);
    let at = dir.join("postProcessing/force_1/0");
    std::fs::create_dir_all(&at).unwrap();
    std::fs::write(dir.join("real.dat"), "x\n").unwrap();
    std::os::unix::fs::symlink(dir.join("real.dat"), at.join("coefficient.dat")).unwrap();
    let error = monitor_names(&dir).unwrap_err();
    assert!(matches!(error, CaptureError::NotAFile(_)), "{error:?}");
    assert!(error.to_string().contains("force_1/0/coefficient.dat"));
}

#[test]
fn the_monitor_file_limit_still_applies() {
    let dir = buoyant_results("limit", 10);
    let mut count = 0;
    for object in ["probe_1", "line_1", "surface_1", "balance_1", "force_1"] {
        for folder in 0..3 {
            let at = dir
                .join("postProcessing")
                .join(object)
                .join(folder.to_string());
            std::fs::create_dir_all(&at).unwrap();
            std::fs::write(at.join("f.dat"), "1\n").unwrap();
            count += 1;
        }
    }
    assert_eq!(
        cfdcraft_capture::monitor_names_limited(&dir, count)
            .unwrap()
            .len(),
        count
    );
    let error = cfdcraft_capture::monitor_names_limited(&dir, count - 1).unwrap_err();
    assert!(matches!(error, CaptureError::Invalid(_)), "{error:?}");
    assert!(error.to_string().contains("monitor files"), "{error}");
}
