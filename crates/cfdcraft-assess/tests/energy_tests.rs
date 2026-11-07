mod support;

use std::fmt::Write as _;

use cfdcraft_assess::{Assessment, Files, HeatSpec, Spec, SurfaceQuantityKind, assess};
use cfdcraft_settle::{BalanceStatus, Settings, Status, VerdictState};
use support::{Shape, cavity, put, surface_spec};

/// A surface file with the temperature-gradient integral as a vector column: `rows` copies of one
/// vector, at times 1, 2, 3...
fn heat_file(rows: u32, vector: [f64; 3]) -> String {
    let mut text = String::from(
        "# Region type : patch isothermal2\n# Faces       : 20\n# Area        : 0.100000000000\n# Scale factor : 1\n#\n# Time\tareaIntegrate(gradT)\n",
    );
    for i in 1..=rows {
        let [x, y, z] = vector;
        let _ = writeln!(text, "{:.12}\t({x:.12} {y:.12} {z:.12})", f64::from(i));
    }
    text
}

/// A settled closed cavity that solves energy, with two heat-flux surfaces read along `component`.
fn heated(component: Option<usize>, hot: [f64; 3], cold: [f64; 3]) -> (Spec, Files) {
    let (mut spec, mut files) = cavity(Shape::Settles);
    spec.solves_energy = true;
    spec.heat = Some(HeatSpec {
        hot: "surface_1".to_owned(),
        cold: "surface_2".to_owned(),
    });
    for (fo, vector) in [("surface_1", hot), ("surface_2", cold)] {
        let mut surface = surface_spec(fo, SurfaceQuantityKind::HeatFlux);
        surface.component = component;
        spec.surfaces.push(surface);
        put(&mut files, fo, heat_file(60, vector));
    }
    (spec, files)
}

fn run(spec: &Spec, files: &Files) -> Assessment {
    assess(spec, files, &Settings::default())
}

fn status_of(a: &Assessment, name: &str) -> Status {
    a.quantities
        .iter()
        .find(|q| q.name == name)
        .unwrap_or_else(|| panic!("no quantity {name}"))
        .status
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn balanced_wall_heat_flows_close_the_energy_balance() {
    let (spec, files) = heated(Some(0), [-12.0, 0.0, 0.0], [-11.99, 0.0, 0.0]);
    let a = run(&spec, &files);
    assert_eq!(a.energy.status, BalanceStatus::Ok, "{:?}", a.energy);
    assert!(close(a.energy.imbalance.unwrap_or(f64::NAN), 0.01 / 12.0));
    assert!(close(a.energy.reference.unwrap_or(f64::NAN), 12.0));
    assert_eq!(a.mass.status, BalanceStatus::NotApplicable);
    assert_eq!(
        a.verdict.state,
        VerdictState::Converged,
        "{:?}",
        a.verdict.reasons
    );
    assert!(a.notes.is_empty(), "{:?}", a.notes);
}

#[test]
fn unbalanced_flows_fail_the_energy_balance() {
    let (spec, files) = heated(Some(0), [-12.0, 0.0, 0.0], [-10.0, 0.0, 0.0]);
    let a = run(&spec, &files);
    assert_eq!(a.energy.status, BalanceStatus::Failed);
    assert!(close(a.energy.imbalance.unwrap_or(f64::NAN), 2.0 / 12.0));
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
    assert_eq!(a.verdict.reasons, ["energy_imbalance"]);
}

#[test]
fn no_wall_monitors_means_unavailable_never_converged() {
    let (mut spec, mut files) = heated(Some(0), [-12.0, 0.0, 0.0], [-11.99, 0.0, 0.0]);
    spec.heat = None;
    spec.surfaces.clear();
    files.monitor_files.remove("surface_1");
    files.monitor_files.remove("surface_2");
    let a = run(&spec, &files);
    assert!(a.notes.contains(&"unavailable:energy".to_owned()));
    assert_eq!(a.energy.status, BalanceStatus::Failed);
    assert_ne!(a.verdict.state, VerdictState::Converged);
    assert_eq!(a.verdict.reasons, ["energy_imbalance"]);
}

#[test]
fn a_missing_or_unreadable_flow_file_fails_the_balance() {
    let (spec, mut files) = heated(Some(0), [-12.0, 0.0, 0.0], [-12.0, 0.0, 0.0]);
    files.monitor_files.remove("surface_2");
    let a = run(&spec, &files);
    assert_eq!(a.energy.status, BalanceStatus::Failed);
    assert!(a.notes.contains(&"missing:surface_2".to_owned()));
    assert_ne!(a.verdict.state, VerdictState::Converged);

    let (spec, mut files) = heated(Some(0), [-12.0, 0.0, 0.0], [-12.0, 0.0, 0.0]);
    put(&mut files, "surface_1", "not a surface file".to_owned());
    let a = run(&spec, &files);
    assert_eq!(a.energy.status, BalanceStatus::Failed);
    assert!(a.notes.contains(&"parse:surface_1".to_owned()));
    assert_ne!(a.verdict.state, VerdictState::Converged);

    // A wall the specification does not monitor as a heat flux is missing too.
    let (mut spec, files) = heated(Some(0), [-12.0, 0.0, 0.0], [-12.0, 0.0, 0.0]);
    spec.heat = Some(HeatSpec {
        hot: "surface_1".to_owned(),
        cold: "surface_9".to_owned(),
    });
    let a = run(&spec, &files);
    assert_eq!(a.energy.status, BalanceStatus::Failed);
    assert!(a.notes.contains(&"missing:surface_9".to_owned()));
}

#[test]
fn the_component_picks_the_axis() {
    let hot = [0.0, -12.0, 0.0];
    let cold = [0.0, -11.99, 0.0];
    let (spec, files) = heated(Some(1), hot, cold);
    let a = run(&spec, &files);
    assert_eq!(a.energy.status, BalanceStatus::Ok, "{:?}", a.energy);
    assert!(close(a.energy.reference.unwrap_or(f64::NAN), 12.0));

    // Along x the same files hold nothing: there is no flow to balance against.
    let (spec, files) = heated(Some(0), hot, cold);
    assert_eq!(run(&spec, &files).energy.status, BalanceStatus::Failed);

    // A vector column read without a component is not a scalar.
    let (spec, files) = heated(None, hot, cold);
    let a = run(&spec, &files);
    assert!(a.notes.contains(&"parse:surface_1".to_owned()));
    assert_eq!(status_of(&a, "surface_1.heat_flux"), Status::Error);
    assert_ne!(a.verdict.state, VerdictState::Converged);
}

#[test]
fn heat_flux_series_are_quantities_too() {
    let (spec, files) = heated(Some(0), [-12.0, 0.0, 0.0], [-11.99, 0.0, 0.0]);
    let a = run(&spec, &files);
    assert_eq!(status_of(&a, "surface_1.heat_flux"), Status::Settled);
    assert_eq!(status_of(&a, "surface_2.heat_flux"), Status::Settled);
    let hot = a
        .quantities
        .iter()
        .find(|q| q.name == "surface_1.heat_flux")
        .map(|q| q.last);
    assert!(close(hot.unwrap_or(f64::NAN), -12.0));
}

#[test]
fn a_study_that_solves_no_energy_is_unchanged() {
    let (mut spec, files) = heated(Some(0), [-12.0, 0.0, 0.0], [-10.0, 0.0, 0.0]);
    spec.solves_energy = false;
    let a = run(&spec, &files);
    assert_eq!(a.energy.status, BalanceStatus::NotApplicable);
    assert!(a.notes.is_empty(), "{:?}", a.notes);
    assert_eq!(a.verdict.state, VerdictState::Converged);
}
