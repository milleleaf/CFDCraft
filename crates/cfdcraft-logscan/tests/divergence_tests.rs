mod support;

use std::collections::BTreeMap;

use cfdcraft_logscan::{
    Divergence, DivergenceCode, FatalKind, Limits, LogState, Scanner, divergence,
};
use support::{cavity_log, local_log, near, ux_log};

const SIGFPE_CRASH: &str =
    include_str!("../../cfdcraft-backend-esi/tests/fixtures/of14-windaround-buildings.log");

fn scan(text: &str) -> LogState {
    let mut scanner = Scanner::new();
    scanner.feed(text);
    scanner.state().clone()
}

fn check(state: &LogState, transient: bool) -> Option<Divergence> {
    divergence(state, &Limits::default(), transient)
}

#[test]
fn a_healthy_cavity_log_never_diverges() {
    let state = scan(&cavity_log(1, 5000, 0.99));
    assert_eq!(state.iterations, 5000);
    assert_eq!(check(&state, false), None);
    assert_eq!(check(&state, true), None);
}

#[test]
fn a_residual_over_both_thresholds_is_a_blowup() {
    let state = scan(&ux_log(&[1e-3, 1e-1, 1e1, 1e3, 5e3]));
    let found = check(&state, false).expect("a blow-up");
    assert_eq!(found.code, DivergenceCode::ResidualBlowup);
    assert_eq!(found.field.as_deref(), Some("Ux"));
    assert!(near(found.value, 5e3));
    assert!(near(found.limit, 1e2), "the floor is the larger limit here");
}

#[test]
fn the_limit_is_the_factor_when_the_first_value_is_large() {
    let state = scan(&ux_log(&[1e1, 1e6]));
    let found = check(&state, false).expect("a blow-up");
    assert!(near(found.limit, 1e5));
}

#[test]
fn a_continued_run_starting_tiny_does_not_trip_the_factor_alone() {
    let state = scan(&ux_log(&[1e-9, 1e-6, 1e-3]));
    assert_eq!(check(&state, false), None);
}

#[test]
fn a_residual_above_the_floor_but_below_the_factor_is_fine() {
    let state = scan(&ux_log(&[1e1, 5e2]));
    assert_eq!(check(&state, false), None);
}

#[test]
fn a_trap_is_found_but_the_banner_is_not() {
    let crash = scan(SIGFPE_CRASH);
    assert_eq!(crash.fatal, Some(FatalKind::FloatingPoint));
    assert_eq!(
        check(&crash, false).map(|d| d.code),
        Some(DivergenceCode::Trap)
    );

    let esi_banner = scan("trapFpe: Floating point exception trapping enabled (FOAM_SIGFPE).\n");
    assert_eq!(check(&esi_banner, false), None);
    let foundation_banner =
        scan("sigFpe : Enabling floating point exception trapping (FOAM_SIGFPE).\n");
    assert_eq!(check(&foundation_banner, false), None);

    let nan = scan(
        "smoothSolver:  Solving for Ux, Initial residual = nan, Final residual = 1e-8, No Iterations 3\n",
    );
    assert_eq!(
        check(&nan, false).map(|d| d.code),
        Some(DivergenceCode::Trap)
    );
}

#[test]
fn a_stored_number_that_cannot_come_from_a_log_is_a_trap() {
    let mut state = scan(&ux_log(&[1e-3, 1e-3]));
    state.last_initial.insert("Ux".to_string(), f64::NAN);
    assert_eq!(
        check(&state, false).map(|d| d.code),
        Some(DivergenceCode::Trap)
    );
    let mut negative = scan(&ux_log(&[1e-3]));
    negative.last_initial.insert("p".to_string(), -1.0);
    assert_eq!(
        check(&negative, false).map(|d| d.code),
        Some(DivergenceCode::Trap)
    );
}

#[test]
fn courant_counts_only_for_transient_runs() {
    let state = scan("Courant Number mean: 2 max: 80\n");
    let found = check(&state, true).expect("a Courant finding");
    assert_eq!(found.code, DivergenceCode::Courant);
    assert!(near(found.value, 80.0));
    assert!(near(found.limit, 50.0));
    assert_eq!(check(&state, false), None);
}

#[test]
fn growing_continuity_is_found() {
    // Falls to about 1e-12, then rises eightfold per step over the last ten steps.
    let mut growing: Vec<f64> = (0..30).map(|k| 1e-3 * 0.5_f64.powi(k)).collect();
    growing.extend((1..=10).map(|j| 1e-10 * 8_f64.powi(j)));
    let found = check(&scan(&local_log(&growing)), false).expect("growing continuity");
    assert_eq!(found.code, DivergenceCode::ContinuityGrowing);
    assert!(found.value > 1e-3);

    // The same shape that stays below the floor is not a finding.
    let mut small: Vec<f64> = (0..30).map(|k| 1e-3 * 0.5_f64.powi(k)).collect();
    small.extend((1..=10).map(|j| 1e-12 * 4_f64.powi(j)));
    assert_eq!(check(&scan(&local_log(&small)), false), None);

    // A flat tail is not a finding.
    let mut flat: Vec<f64> = (0..30).map(|k| 1e-3 * 0.5_f64.powi(k)).collect();
    flat.extend(std::iter::repeat_n(1e-12, 10));
    assert_eq!(check(&scan(&local_log(&flat)), false), None);
}

#[test]
fn codes_are_stable() {
    assert_eq!(DivergenceCode::Trap.code(), "CFD-RUN-003");
    assert_eq!(DivergenceCode::ResidualBlowup.code(), "CFD-RUN-004");
    assert_eq!(DivergenceCode::Courant.code(), "CFD-RUN-005");
    assert_eq!(DivergenceCode::ContinuityGrowing.code(), "CFD-RUN-006");
}

#[test]
fn the_first_rule_that_fires_wins_in_a_fixed_order() {
    let mut state = LogState {
        fatal: Some(FatalKind::FloatingPoint),
        first_initial: BTreeMap::from([("Ux".to_string(), 1e-3)]),
        last_initial: BTreeMap::from([("Ux".to_string(), 5e3)]),
        max_courant: Some(80.0),
        min_local: Some(1e-9),
        local_errors: vec![0.5; 10],
        ..LogState::default()
    };
    let order = [
        DivergenceCode::Trap,
        DivergenceCode::ResidualBlowup,
        DivergenceCode::Courant,
        DivergenceCode::ContinuityGrowing,
    ];
    assert_eq!(check(&state, true).map(|d| d.code), Some(order[0]));
    state.fatal = None;
    assert_eq!(check(&state, true).map(|d| d.code), Some(order[1]));
    state.last_initial.insert("Ux".to_string(), 1e-3);
    assert_eq!(check(&state, true).map(|d| d.code), Some(order[2]));
    state.max_courant = None;
    assert_eq!(check(&state, true).map(|d| d.code), Some(order[3]));
    state.local_errors.clear();
    assert_eq!(check(&state, true), None);
}
