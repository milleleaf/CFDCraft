mod support;

use cfdcraft_assess::{Files, Spec, assess};
use cfdcraft_monitors::FieldKind;
use cfdcraft_settle::{BalanceStatus, Settings, Status, VerdictState};
use support::{Shape, cavity, log, probe_spec, put, velocity_probe};

fn status_of(a: &cfdcraft_assess::Assessment, name: &str) -> Status {
    a.quantities
        .iter()
        .find(|q| q.name == name)
        .unwrap_or_else(|| panic!("no quantity {name}"))
        .status
}

#[test]
fn a_converged_closed_cavity_is_converged() {
    let (spec, files) = cavity(Shape::Settles);
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(
        a.verdict.state,
        VerdictState::Converged,
        "{:?}",
        a.verdict.reasons
    );
    assert_eq!(a.mass.status, BalanceStatus::NotApplicable);
    assert_eq!(a.energy.status, BalanceStatus::NotApplicable);
    assert_eq!(a.quantities.len(), 10);
    assert!(a.quantities.iter().all(|q| q.status == Status::Settled));
    assert!(a.ended);
    assert_eq!(a.iterations, 1500);
    assert!(a.divergence.is_none());
    assert!(a.verdict.residuals_ok);
    assert!(a.notes.is_empty());
}

#[test]
fn one_drifting_probe_blocks_converged() {
    let (spec, files) = cavity(Shape::Ramps);
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
    assert!(
        a.verdict
            .reasons
            .contains(&"not_settled:probe_3.Ux".to_owned())
    );
    assert_eq!(status_of(&a, "probe_3.Ux"), Status::Drifting);
    assert_eq!(status_of(&a, "probe_1.Ux"), Status::Settled);
    assert_eq!(status_of(&a, "probe_5.Uy"), Status::Settled);
}

#[test]
fn small_residuals_with_no_monitor_are_not_judged() {
    let spec = Spec {
        residual_limits: Some((1e-6, 1e-6)),
        ..Spec::default()
    };
    let files = Files {
        log: log(1500, true),
        ..Files::default()
    };
    let a = assess(&spec, &files, &Settings::default());
    assert!(a.verdict.residuals_ok);
    assert!(a.quantities.is_empty());
    assert_eq!(a.verdict.state, VerdictState::NotJudged);
    assert_eq!(a.verdict.reasons, ["no_monitored_quantity"]);
}

#[test]
fn residuals_above_the_limit_block_converged_even_if_everything_settled() {
    let (mut spec, files) = cavity(Shape::Settles);
    spec.residual_limits = Some((1e-12, 1e-12));
    let a = assess(&spec, &files, &Settings::default());
    assert!(a.quantities.iter().all(|q| q.status == Status::Settled));
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
    assert_eq!(a.verdict.reasons, ["residuals_above_limit"]);
    let ux = a.residuals.iter().find(|r| r.field == "Ux").unwrap();
    assert!(ux.first > ux.last);
}

#[test]
fn a_run_still_in_progress_is_assessed_from_a_partial_log() {
    let (spec, mut files) = cavity(Shape::Settles);
    let whole = log(30, false);
    files.log = whole[..whole.len() - 10].to_owned();
    for i in 1..=5 {
        put(
            &mut files,
            &format!("probe_{i}"),
            velocity_probe(30, Shape::Settles, 0.2),
        );
    }
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(a.iterations, 30);
    assert!(!a.ended);
    assert!(a.quantities.iter().all(|q| q.status == Status::TooEarly));
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
}

#[test]
fn the_labels_ride_along_for_display_and_names_come_from_the_function_object() {
    let (spec, files) = cavity(Shape::Settles);
    let a = assess(&spec, &files, &Settings::default());
    assert_eq!(
        a.labels.get("probe_2").map(String::as_str),
        Some("label of probe_2")
    );
    assert!(
        a.quantities
            .iter()
            .all(|q| q.name.starts_with("probe_") && !q.name.contains("label"))
    );
    assert!(a.quantities.iter().any(|q| q.name == "probe_1.Ux"));
}

#[test]
fn settings_are_applied() {
    let mut spec = Spec::default();
    spec.probes.push(probe_spec("probe_1", FieldKind::Velocity));
    let mut files = Files {
        log: log(1500, true),
        ..Files::default()
    };
    put(
        &mut files,
        "probe_1",
        velocity_probe(1500, Shape::Jitters, 1.0),
    );
    let strict = assess(&spec, &files, &Settings::default());
    assert_eq!(status_of(&strict, "probe_1.Ux"), Status::Oscillating);
    let loose = Settings {
        spread_rel: 0.1,
        ..Settings::default()
    };
    let a = assess(&spec, &files, &loose);
    assert_eq!(status_of(&a, "probe_1.Ux"), Status::Settled);
}

#[test]
fn a_monitor_with_no_data_yet_is_too_early_with_a_missing_note() {
    let mut spec = Spec::default();
    spec.probes.push(probe_spec("probe_1", FieldKind::Velocity));
    let files = Files {
        log: log(100, false),
        ..Files::default()
    };
    let a = assess(&spec, &files, &Settings::default());
    let names: Vec<&str> = a.quantities.iter().map(|q| q.name.as_str()).collect();
    assert_eq!(names, ["probe_1.Ux", "probe_1.Uy"]);
    assert!(
        a.quantities
            .iter()
            .all(|q| q.status == Status::TooEarly && q.samples == 0)
    );
    assert_eq!(a.notes, ["missing:probe_1"]);
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
}

#[test]
fn a_log_that_shows_the_solver_failed_is_never_converged() {
    let (spec, calm) = cavity(Shape::Settles);
    let base = assess(&spec, &calm, &Settings::default());
    assert_eq!(base.verdict.state, VerdictState::Converged);

    for tail in [
        "--> FOAM FATAL ERROR:\ncannot continue\n",
        "Courant Number mean: nan max: nan\n",
    ] {
        let mut files = calm.clone();
        files.log.push_str(tail);
        let a = assess(&spec, &files, &Settings::default());
        assert!(a.quantities.iter().all(|q| q.status == Status::Settled));
        assert_eq!(a.verdict.state, VerdictState::NotConverged, "{tail}");
        assert_eq!(a.verdict.reasons, ["solver_failed"], "{tail}");
    }
}
