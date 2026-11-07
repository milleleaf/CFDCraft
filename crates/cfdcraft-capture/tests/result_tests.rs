use std::path::{Path, PathBuf};

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_capture::{find_time_dir, key_numbers, result_bundle};

fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("capture-result-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn results(test: &str, samples: usize, shift: f64) -> PathBuf {
    let dir = scratch(test);
    let spec = FixtureSpec {
        end_time: 5000,
        cells: 64,
        samples,
        shift,
    };
    write_results(&dir, &spec).unwrap();
    dir
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn the_result_of_a_run_is_bundled_with_its_logs_fields_and_samples() {
    let dir = results("bundled", 3, 0.0);
    let without_flux = result_bundle(&dir, 5000.0).unwrap();
    let expected = [
        "5000/U",
        "5000/p",
        "log.blockMesh",
        "log.checkMesh",
        "log.simpleFoam",
        "postProcessing/centreline/5000/horizontal_U.xy",
        "postProcessing/centreline/5000/vertical_U.xy",
    ];
    assert_eq!(without_flux.names(), expected);

    std::fs::write(dir.join("5000/phi"), "flux\n").unwrap();
    let with_flux = result_bundle(&dir, 5000.0).unwrap();
    assert_eq!(with_flux.names().len(), 8);
    assert!(with_flux.names().contains(&"5000/phi"));
}

#[test]
fn the_end_time_folder_is_found_by_value() {
    let dir = scratch("by-value");
    std::fs::create_dir_all(dir.join("2.5")).unwrap();
    std::fs::create_dir_all(dir.join("5000")).unwrap();
    std::fs::create_dir_all(dir.join("constant")).unwrap();
    assert_eq!(find_time_dir(&dir, 2.5).unwrap(), "2.5");
    assert_eq!(find_time_dir(&dir, 5000.0).unwrap(), "5000");
}

#[test]
fn key_numbers_hold_the_residuals_and_the_centreline() {
    let dir = results("numbers", 7, 0.5);
    let numbers = key_numbers(&dir, 5000.0).unwrap();
    assert_eq!(numbers.len(), 4 + 14);
    assert!(near(numbers["solver.iterations"], 5000.0));
    assert!(near(numbers["residual.Ux"], 1e-7));
    assert!(near(numbers["residual.Uy"], 1.5e-7));
    assert!(near(numbers["residual.p"], 2e-7));
    assert!(near(numbers["centreline.vertical.ux.0000"], 0.5));
    assert!(near(numbers["centreline.vertical.ux.0006"], 0.56));
    assert!(near(numbers["centreline.horizontal.uy.0003"], 0.53));
}

#[test]
fn a_different_shift_gives_different_numbers() {
    let a = key_numbers(&results("shift-a", 7, 0.0), 5000.0).unwrap();
    let b = key_numbers(&results("shift-b", 7, 0.25), 5000.0).unwrap();
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    let differing: Vec<&String> = a.keys().filter(|name| a[*name] != b[*name]).collect();
    assert_eq!(differing.len(), 14);
    assert!(differing.iter().all(|name| name.starts_with("centreline.")));
}

#[test]
fn the_last_iteration_is_the_one_read() {
    let dir = results("last", 2, 0.0);
    std::fs::write(
        dir.join("log.simpleFoam"),
        "Time = 4999\n\
         smoothSolver: Solving for Ux, Initial residual = 9e-03, Final residual = 1e-11, No Iterations 1\n\
         smoothSolver: Solving for Uy, Initial residual = 8e-03, Final residual = 1e-11, No Iterations 1\n\
         PCG: Solving for p, Initial residual = 7e-03, Final residual = 1e-11, No Iterations 1\n\
         Time = 5000\n\
         smoothSolver:  Solving for Ux, Initial residual = 3e-07, Final residual = 1e-11, No Iterations 1\n\
         smoothSolver:  Solving for Uy, Initial residual = 2e-07, Final residual = 1e-11, No Iterations 1\n\
         DICPCG:  Solving for p, Initial residual = 4e-07, Final residual = 1e-11, No Iterations 1\n\
         End\n",
    )
    .unwrap();
    let numbers = key_numbers(&dir, 5000.0).unwrap();
    assert!(near(numbers["solver.iterations"], 5000.0));
    assert!(near(numbers["residual.Ux"], 3e-7));
    assert!(near(numbers["residual.Uy"], 2e-7));
    assert!(near(numbers["residual.p"], 4e-7));
}
