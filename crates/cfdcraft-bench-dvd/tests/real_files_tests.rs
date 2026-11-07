//! The heated-cavity comparer over the heat-flux and line files a real solver wrote.
//!
//! The files come from sixty iterations of the cavity at the coarse mesh and a Rayleigh number of
//! 1e5: far from a solution, so only what the files show is asserted (their layout, the sign of the
//! heat flux and what the readers make of it), never whether the benchmark passes.

mod support;

use cfdcraft_bench_dvd::{Setup, measure};
use cfdcraft_model::ResolvedStudy;
use support::{Case, Tree, study};

const HOT: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-heated-hot-gradT.dat");
const COLD: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-heated-cold-gradT.dat");
const HORIZONTAL: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-heated-line_1-U.xy");
const VERTICAL: &str =
    include_str!("../../cfdcraft-monitors/tests/fixtures/v2512-real-heated-line_2-U.xy");

fn real_study() -> ResolvedStudy {
    study(&Case {
        ra: 1e5,
        ..Case::default()
    })
}

fn near(found: f64, wanted: f64) -> bool {
    (found - wanted).abs() <= 1e-9 * wanted.abs()
}

/// The monitors are numbered in label order, so the solver wrote the hot wall's heat flux as
/// `surface_2` and the cold wall's as `surface_1`, and the horizontal line as `line_1` and the
/// vertical one as `line_2`.
fn real_tree(setup: &Setup) -> Tree {
    let mut tree = Tree::default();
    tree.put(
        &format!("postProcessing/{}/0/surfaceFieldValue.dat", setup.hot_fo),
        HOT,
    );
    tree.put(
        &format!("postProcessing/{}/0/surfaceFieldValue.dat", setup.cold_fo),
        COLD,
    );
    tree.put(
        &format!("postProcessing/{}/60/line_U.xy", setup.horizontal_fo),
        HORIZONTAL,
    );
    tree.put(
        &format!("postProcessing/{}/60/line_U.xy", setup.vertical_fo),
        VERTICAL,
    );
    tree
}

#[test]
fn the_study_asks_for_the_function_objects_the_solver_wrote() {
    let setup = Setup::from_study(&real_study()).unwrap();
    assert_eq!(setup.hot_fo, "surface_2");
    assert_eq!(setup.cold_fo, "surface_1");
    assert_eq!(setup.horizontal_fo, "line_1");
    assert_eq!(setup.vertical_fo, "line_2");
    assert!(near(setup.rayleigh, 1e5), "{}", setup.rayleigh);
    assert!(near(setup.delta_t, 1.0));
}

#[test]
fn the_real_heat_flux_and_line_files_are_measured() {
    let r = real_study();
    let setup = Setup::from_study(&r).unwrap();
    let measured = measure(&real_tree(&setup), &r).unwrap();
    // Nu = -(L / dT) x integral / area, from the last row: -0.4455807999518 over an area of 0.1.
    assert!(near(measured.nu_hot, 4.455807999518), "{}", measured.nu_hot);
    // The x component of the hot-wall integral is negative, so the Nusselt number is positive:
    // the sign convention of the comparer holds for the real file.
    assert!(measured.nu_hot > 0.0);
    // The two walls' heat flows differ by 0.33 percent after 60 iterations.
    assert!(
        near(measured.energy_imbalance, 0.0033248109915423874),
        "{}",
        measured.energy_imbalance
    );
    assert!(
        near(measured.u_max, 9.279236981226532),
        "{}",
        measured.u_max
    );
    assert!(
        near(measured.u_max_y, 0.7687896779205154),
        "{}",
        measured.u_max_y
    );
    assert!(
        near(measured.w_max, 29.017280160399807),
        "{}",
        measured.w_max
    );
    assert!(
        near(measured.w_max_x, 0.059214857386146076),
        "{}",
        measured.w_max_x
    );
}
