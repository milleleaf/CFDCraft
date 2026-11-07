use std::path::{Path, PathBuf};

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_capture::{CaptureError, key_numbers, mesh_bundle, result_bundle};

fn results(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("capture-refuse-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let spec = FixtureSpec {
        end_time: 5000,
        cells: 64,
        samples: 4,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    dir
}

const VERTICAL: &str = "postProcessing/centreline/5000/vertical_U.xy";
const HORIZONTAL: &str = "postProcessing/centreline/5000/horizontal_U.xy";

#[test]
fn a_missing_required_file_is_refused_by_name() {
    let files = [
        "5000/U",
        "5000/p",
        "log.blockMesh",
        "log.checkMesh",
        "log.simpleFoam",
        VERTICAL,
        HORIZONTAL,
    ];
    for (index, file) in files.into_iter().enumerate() {
        let dir = results(&format!("missing-{index}"));
        std::fs::remove_file(dir.join(file)).unwrap();
        let error = result_bundle(&dir, 5000.0).unwrap_err();
        assert!(
            matches!(error, CaptureError::Missing(_)),
            "{file}: {error:?}"
        );
        assert!(error.to_string().contains(file), "{file}: {error}");
    }
    for (index, file) in ["log.simpleFoam", VERTICAL, HORIZONTAL]
        .into_iter()
        .enumerate()
    {
        let dir = results(&format!("missing-numbers-{index}"));
        std::fs::remove_file(dir.join(file)).unwrap();
        let error = key_numbers(&dir, 5000.0).unwrap_err();
        assert!(
            matches!(error, CaptureError::Missing(_)),
            "{file}: {error:?}"
        );
        assert!(error.to_string().contains(file), "{file}: {error}");
    }
}

#[test]
fn a_log_without_a_residual_is_refused() {
    let dir = results("no-residual");
    std::fs::write(
        dir.join("log.simpleFoam"),
        "Time = 4999\n\
         PCG: Solving for p, Initial residual = 7e-03, Final residual = 1e-11, No Iterations 1\n\
         Time = 5000\n\
         smoothSolver: Solving for Ux, Initial residual = 3e-07, Final residual = 1e-11, No Iterations 1\n\
         smoothSolver: Solving for Uy, Initial residual = 2e-07, Final residual = 1e-11, No Iterations 1\n\
         End\n",
    )
    .unwrap();
    let error = key_numbers(&dir, 5000.0).unwrap_err();
    assert!(matches!(error, CaptureError::Invalid(_)), "{error:?}");
    assert!(error.to_string().contains("log.simpleFoam"), "{error}");
}

#[test]
fn a_sample_that_is_not_finite_is_refused() {
    for (index, bad) in ["NaN", "inf"].into_iter().enumerate() {
        let dir = results(&format!("not-finite-{index}"));
        std::fs::write(dir.join(VERTICAL), format!("0.0 {bad} 0.0 0.0\n")).unwrap();
        let error = key_numbers(&dir, 5000.0).unwrap_err();
        assert!(
            matches!(error, CaptureError::Invalid(_)),
            "{bad}: {error:?}"
        );
        assert!(error.to_string().contains("vertical_U.xy"), "{error}");
    }
}

#[test]
fn a_sample_row_with_the_wrong_columns_is_refused() {
    let dir = results("columns");
    std::fs::write(dir.join(HORIZONTAL), "# comment\n0.0 0.1 0.2\n").unwrap();
    let error = key_numbers(&dir, 5000.0).unwrap_err();
    assert!(matches!(error, CaptureError::Invalid(_)), "{error:?}");
    assert!(error.to_string().contains("horizontal_U.xy"), "{error}");
}

#[test]
fn no_time_folder_for_the_end_time_is_refused() {
    let dir = results("no-time");
    let error = result_bundle(&dir, 4000.0).unwrap_err();
    assert!(matches!(error, CaptureError::Missing(_)), "{error:?}");
    assert!(error.to_string().contains("end-time folder"), "{error}");
}

#[cfg(unix)]
#[test]
fn a_link_inside_the_result_tree_is_refused() {
    let dir = results("link");
    let field = dir.join("5000/U");
    std::fs::remove_file(&field).unwrap();
    std::os::unix::fs::symlink(dir.join("log.blockMesh"), field).unwrap();
    let error = result_bundle(&dir, 5000.0).unwrap_err();
    assert!(matches!(error, CaptureError::NotAFile(_)), "{error:?}");
    assert!(error.to_string().contains("5000/U"), "{error}");
}

#[test]
fn the_transfer_archives_are_never_captured() {
    let dir = results("archives");
    std::fs::write(dir.join("input.tar"), "archive").unwrap();
    std::fs::write(dir.join("results.tar"), "archive").unwrap();
    let result = result_bundle(&dir, 5000.0).unwrap();
    let mesh = mesh_bundle(&dir).unwrap();
    for name in result.names().into_iter().chain(mesh.names()) {
        assert!(!name.ends_with(".tar"), "{name}");
    }
}
