mod support;

use cfdcraft_assess::{Assessment, Files, Spec, SurfaceQuantityKind, assess};
use cfdcraft_monitors::FieldKind;
use cfdcraft_settle::{BalanceStatus, Settings, Status, VerdictState};
use support::{
    Shape, cavity, line_folders, line_spec, log, patch, probe_spec, put, settling_surface,
    surface_file, surface_spec, velocity_probe,
};

fn status_of(a: &Assessment, name: &str) -> Status {
    a.quantities
        .iter()
        .find(|q| q.name == name)
        .unwrap_or_else(|| panic!("no quantity {name}"))
        .status
}

fn only_log() -> Files {
    Files {
        log: log(1500, true),
        ..Files::default()
    }
}

/// Five settled probes, an inlet and an outlet; the fluxes are the last values of the two files.
fn open_case(inlet: &[f64], outlet: &[f64]) -> (Spec, Files) {
    let (mut spec, mut files) = cavity(Shape::Settles);
    spec.patches = vec![patch("inlet", true), patch("outlet", false)];
    files.monitor_files.remove("balance_movingWall");
    files.monitor_files.remove("balance_fixedWalls");
    put(&mut files, "balance_inlet", surface_file("sum(phi)", inlet));
    put(
        &mut files,
        "balance_outlet",
        surface_file("sum(phi)", outlet),
    );
    (spec, files)
}

#[test]
fn a_line_monitor_reports_component_means_per_write_time() {
    let mut spec = Spec::default();
    spec.lines.push(line_spec("line_1"));
    let mut files = only_log();
    files
        .monitor_files
        .insert("line_1".to_owned(), line_folders(20));
    let a = assess(&spec, &files, &Settings::default());
    let names: Vec<&str> = a.quantities.iter().map(|q| q.name.as_str()).collect();
    assert_eq!(names, ["line_1.mean_Ux", "line_1.mean_Uy"]);
    assert!(a.quantities.iter().all(|q| q.status == Status::Settled));

    files
        .monitor_files
        .insert("line_1".to_owned(), line_folders(4));
    let few = assess(&spec, &files, &Settings::default());
    assert!(
        few.quantities
            .iter()
            .all(|q| q.status == Status::TooFewSamples)
    );
}

#[test]
fn surface_monitors_report_their_column() {
    let mut spec = Spec::default();
    spec.surfaces
        .push(surface_spec("surface_1", SurfaceQuantityKind::Flux));
    spec.surfaces.push(surface_spec(
        "surface_2",
        SurfaceQuantityKind::PressureAverage,
    ));
    spec.surfaces.push(surface_spec(
        "surface_3",
        SurfaceQuantityKind::PressureIntegral,
    ));
    let mut files = only_log();
    put(
        &mut files,
        "surface_1",
        settling_surface("sum(phi)", 1500, 0.5),
    );
    put(
        &mut files,
        "surface_2",
        settling_surface("areaAverage(p)", 1500, 2.0),
    );
    put(
        &mut files,
        "surface_3",
        settling_surface("areaIntegrate(p)", 1500, 3.0),
    );
    let a = assess(&spec, &files, &Settings::default());
    for name in [
        "surface_1.flux",
        "surface_2.pressure_average",
        "surface_3.pressure_integral",
    ] {
        assert_eq!(status_of(&a, name), Status::Settled, "{name}");
    }
    assert_eq!(a.quantities.len(), 3);
}

#[test]
fn a_closing_inlet_outlet_pair_has_an_ok_mass_balance() {
    let (spec, files) = open_case(&[-1.0e-3], &[1.0e-3]);
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(a.mass.status, BalanceStatus::Ok);
    assert_eq!(
        a.verdict.state,
        VerdictState::Converged,
        "{:?}",
        a.verdict.reasons
    );
}

#[test]
fn an_open_balance_fails_and_blocks_converged() {
    let (spec, files) = open_case(&[-1.0e-3], &[1.01e-3]);
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(a.mass.status, BalanceStatus::Failed);
    assert!(a.mass.imbalance.is_some());
    assert!(a.quantities.iter().all(|q| q.status == Status::Settled));
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
    assert_eq!(a.verdict.reasons, ["mass_imbalance"]);
}

#[test]
fn a_missing_flux_file_is_a_failed_balance_with_a_note() {
    let (spec, mut files) = open_case(&[-1.0e-3], &[1.0e-3]);
    files.monitor_files.remove("balance_outlet");
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(a.mass.status, BalanceStatus::Failed);
    assert_eq!(a.mass.imbalance, None);
    assert!(a.notes.contains(&"missing:balance_outlet".to_owned()));
    assert_ne!(a.verdict.state, VerdictState::Converged);
}

#[test]
fn a_closed_domain_ignores_missing_flux_files() {
    let (spec, mut files) = cavity(Shape::Settles);
    files.monitor_files.remove("balance_movingWall");
    files.monitor_files.remove("balance_fixedWalls");
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(a.mass.status, BalanceStatus::NotApplicable);
    assert!(a.notes.is_empty());
    assert_eq!(a.verdict.state, VerdictState::Converged);
}

#[test]
fn a_malformed_monitor_file_is_an_error_quantity_and_blocks_converged() {
    let (spec, mut files) = cavity(Shape::Settles);
    let broken = velocity_probe(1500, Shape::Settles, 0.2) + "1501.000000000000   (nan 0.0 0.0)\n";
    put(&mut files, "probe_2", broken);
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(status_of(&a, "probe_2.U"), Status::Error);
    assert!(
        a.verdict
            .reasons
            .contains(&"not_settled:probe_2.U".to_owned())
    );
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
    assert!(a.notes.contains(&"parse:probe_2".to_owned()));
}

#[test]
fn energy_is_not_applicable_when_not_solved_and_failed_when_solved_without_data() {
    let mut spec = Spec::default();
    spec.probes.push(probe_spec("probe_1", FieldKind::Velocity));
    let mut files = only_log();
    put(
        &mut files,
        "probe_1",
        velocity_probe(1500, Shape::Settles, 0.2),
    );
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(a.energy.status, BalanceStatus::NotApplicable);
    assert_eq!(a.verdict.state, VerdictState::Converged);

    spec.solves_energy = true;
    let b = assess(&spec, &files, &Settings::default());
    assert_eq!(b.energy.status, BalanceStatus::Failed);
    assert_eq!(b.verdict.reasons, ["energy_imbalance"]);
    assert_eq!(b.verdict.state, VerdictState::NotConverged);
}

#[test]
fn divergence_is_reported_beside_the_verdict() {
    let (spec, mut files) = cavity(Shape::Settles);
    let calm = assess(&spec, &files, &Settings::default());
    files.log = support::blown_up_log();
    let a = assess(&spec, &files, &Settings::default());
    let found = a.divergence.expect("a blow-up is reported");
    assert_eq!(found.code.code(), "CFD-RUN-004");
    assert_eq!(a.verdict.quantities, calm.verdict.quantities);
    assert_eq!(a.verdict.reasons, ["residuals_above_limit"]);
}

#[test]
fn every_note_is_a_kind_and_name_word_pair() {
    let (mut spec, mut files) = open_case(&[-1.0e-3], &[1.0e-3]);
    files.monitor_files.remove("balance_outlet");
    files.monitor_files.remove("probe_1");
    put(&mut files, "probe_2", "not a probe file\n".to_owned());
    spec.solves_energy = true;
    let a = assess(&spec, &files, &Settings::default());
    assert!(a.notes.len() >= 4, "{:?}", a.notes);
    for note in &a.notes {
        let (kind, name) = note.split_once(':').expect("kind:name");
        assert!(!kind.is_empty() && !name.is_empty(), "{note}");
        assert!(!note.contains(char::is_whitespace), "{note}");
    }
}
