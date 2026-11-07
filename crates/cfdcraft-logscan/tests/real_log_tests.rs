//! The scanner, the divergence rules and the error table over logs a real solver wrote.
//!
//! Every number below was read from the fixture file itself. The fixtures are real, with the host
//! name and process id removed, and the two long ones cut in the middle: see `PROVENANCE.md`.

use cfdcraft_logscan::{
    Divergence, DivergenceCode, ErrorId, Event, FatalKind, LOCAL_WINDOW, Limits, LogState, Scanner,
    divergence, explain,
};

const CAVITY: &str = include_str!("fixtures/v2512-real-cavity-simpleFoam.log");
const DIVERGED: &str = include_str!("fixtures/v2512-real-diverged-simpleFoam.log");
const DECOMPOSE: &str = include_str!("fixtures/v2512-real-parallel-decomposePar.log");
const PARALLEL: &str = include_str!("fixtures/v2512-real-parallel-simpleFoam.log");
const RECONSTRUCT: &str = include_str!("fixtures/v2512-real-parallel-reconstructPar.log");
const CYLINDER: &str = include_str!("fixtures/v2512-real-cylinder-pimpleFoam.log");
const HEATED: &str = include_str!("fixtures/v2512-real-heated-buoyantBoussinesqSimpleFoam.log");
const STEP: &str = include_str!("fixtures/v2512-real-step-simpleFoam.log");
const POTENTIAL: &str = include_str!("fixtures/v2512-real-step-potentialFoam.log");
const MESH_FATAL: &str = include_str!("fixtures/v2512-real-step-blockMesh-fatal.log");

fn scan(text: &str) -> (LogState, Vec<Event>) {
    let mut scanner = Scanner::new();
    let events = scanner.feed(text);
    (scanner.state().clone(), events)
}

fn check(state: &LogState) -> Option<Divergence> {
    divergence(state, &Limits::default(), false)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * b.abs().max(1e-300)
}

/// The text of `log` up to the end of the iteration before the line `Time = <next>`.
fn before_time(log: &str, next: u32) -> &str {
    let marker = format!("\nTime = {next}\n");
    let at = log.find(&marker).expect("the iteration is in the file");
    &log[..=at]
}

fn count(events: &[Event], wanted: fn(&Event) -> bool) -> usize {
    events.iter().filter(|e| wanted(e)).count()
}

#[test]
fn the_real_files_are_plain_ascii_with_the_start_up_banner_in_them() {
    for log in [CAVITY, DIVERGED, DECOMPOSE, PARALLEL, RECONSTRUCT] {
        assert!(log.is_ascii());
        assert!(log.contains("Floating point exception trapping enabled (FOAM_SIGFPE)."));
        assert!(log.contains("Host   : redacted"));
        assert!(log.contains("PID    : redacted"));
    }
}

#[test]
fn a_real_cavity_log_is_read_to_its_end() {
    let (state, events) = scan(CAVITY);
    assert_eq!(state.iterations, 500);
    assert_eq!(state.last_time, Some(5000.0));
    assert_eq!(
        state
            .first_initial
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["Ux", "Uy", "p"]
    );
    assert_eq!(state.first_initial["Ux"], 1.0);
    assert_eq!(state.first_initial["Uy"], 0.932220380601);
    assert_eq!(state.first_initial["p"], 1.0);
    assert_eq!(state.last_initial["Ux"], 4.37979951499e-08);
    assert_eq!(state.last_initial["Uy"], 5.10358757954e-08);
    assert_eq!(state.last_initial["p"], 3.66368478664e-08);
    assert_eq!(state.min_local, Some(7.0939369592e-13));
    assert_eq!(state.local_errors.len(), LOCAL_WINDOW);
    assert_eq!(state.local_errors.last(), Some(&2.02644786387e-12));
    assert_eq!(state.execution_seconds, Some(251.01));
    assert_eq!(state.max_courant, None);
    assert!(state.ended);
    assert!(!state.finalising);
    assert_eq!(state.fatal, None, "the trapping banner is not a trap");
    assert!(!state.non_finite);

    assert_eq!(count(&events, |e| matches!(e, Event::Time { .. })), 500);
    assert_eq!(
        count(&events, |e| matches!(e, Event::Residual { .. })),
        1500
    );
    assert_eq!(
        count(&events, |e| matches!(e, Event::Continuity { .. })),
        500
    );
    assert_eq!(
        count(&events, |e| matches!(e, Event::ExecutionTime { .. })),
        500
    );
    assert_eq!(count(&events, |e| matches!(e, Event::End)), 1);
    assert_eq!(events.last(), Some(&Event::End));
}

#[test]
fn a_real_cavity_log_never_diverges() {
    let (state, _) = scan(CAVITY);
    assert_eq!(check(&state), None);
    assert_eq!(divergence(&state, &Limits::default(), true), None);
}

#[test]
fn a_real_log_cut_into_pieces_gives_the_same_events_and_state() {
    for log in [CAVITY, DIVERGED, PARALLEL] {
        let (whole_state, whole_events) = scan(log);
        let mut pieces = Scanner::new();
        let mut events = Vec::new();
        for piece in log.as_bytes().chunks(4001) {
            events.extend(pieces.feed(std::str::from_utf8(piece).expect("ascii")));
        }
        assert_eq!(events, whole_events);
        assert_eq!(pieces.state(), &whole_state);
    }
}

#[test]
fn a_real_blown_up_log_shows_a_continuity_error_that_grew() {
    let (state, events) = scan(DIVERGED);
    assert_eq!(state.iterations, 559);
    assert_eq!(state.last_time, Some(3009.0));
    assert_eq!(state.first_initial["Uy"], 0.999999992731);
    assert_eq!(state.last_initial["Ux"], 0.0190097459202);
    assert_eq!(state.last_initial["Uy"], 0.0227012859252);
    assert_eq!(state.last_initial["p"], 0.273392327228);
    assert_eq!(state.min_local, Some(7.09177711735e-06));
    assert_eq!(state.local_errors.last(), Some(&0.00204712643739));
    assert_eq!(state.execution_seconds, Some(184.89));
    assert!(state.ended, "the stopped solver still printed End");
    assert_eq!(state.fatal, None);
    assert!(!state.non_finite, "no number went to nan or inf");
    assert_eq!(events.last(), Some(&Event::End));

    let found = check(&state).expect("the continuity rule fires");
    assert_eq!(found.code, DivergenceCode::ContinuityGrowing);
    assert_eq!(found.field, None);
    assert!(near(found.value, 0.0019336190377449998), "{}", found.value);
    assert!(near(found.limit, 1e-3), "{}", found.limit);
}

#[test]
fn in_a_real_blown_up_log_the_rule_held_for_a_moment_early_and_held_for_good_late() {
    // A spike at iteration 103: the mean of the last ten values passes the floor, then falls.
    let spike = scan(before_time(DIVERGED, 104)).0;
    let found = check(&spike).expect("the rule holds at iteration 103");
    assert_eq!(found.code, DivergenceCode::ContinuityGrowing);
    assert!(near(found.value, 0.0012113018112957), "{}", found.value);
    assert!(near(found.limit, 1e-3));
    // At iteration 250, the end of the first part of the file, it does not.
    assert_eq!(check(&scan(before_time(DIVERGED, 2701)).0), None);
    // Late in the run it holds from iteration 2802 on and does not stop holding.
    assert_eq!(check(&scan(before_time(DIVERGED, 2802)).0), None);
    let late = scan(before_time(DIVERGED, 2803)).0;
    let found = check(&late).expect("the rule holds at iteration 2802");
    assert!(near(found.value, 0.0011846507071053), "{}", found.value);
}

#[test]
fn a_real_parallel_solver_log_ends_with_end_then_finalising() {
    let (state, events) = scan(PARALLEL);
    assert!(PARALLEL.contains("nProcs : 2"));
    assert!(PARALLEL.contains("Exec   : simpleFoam -parallel"));
    assert_eq!(state.iterations, 120);
    assert_eq!(state.last_time, Some(120.0));
    assert_eq!(state.first_initial["Uy"], 0.990868873148);
    assert_eq!(state.last_initial["Ux"], 0.00173675411941);
    assert_eq!(state.last_initial["Uy"], 0.00198549813583);
    assert_eq!(state.last_initial["p"], 0.000611882136409);
    assert_eq!(state.min_local, Some(4.34186825954e-13));
    assert_eq!(state.local_errors.last(), Some(&8.17342297496e-13));
    assert_eq!(state.execution_seconds, Some(47.93));
    assert!(state.ended);
    assert!(state.finalising);
    assert_eq!(state.fatal, None);
    assert_eq!(check(&state), None);
    let n = events.len();
    assert_eq!(&events[n - 2..], [Event::End, Event::Finalising]);
}

#[test]
fn the_real_decomposition_and_reconstruction_logs_end_cleanly() {
    let (decomposed, events) = scan(DECOMPOSE);
    assert!(DECOMPOSE.contains("Exec   : decomposePar -force"));
    assert_eq!(decomposed.iterations, 1);
    assert_eq!(decomposed.last_time, Some(0.0));
    assert!(decomposed.first_initial.is_empty());
    assert!(decomposed.ended);
    assert!(!decomposed.finalising);
    assert_eq!(decomposed.fatal, None);
    assert_eq!(events.last(), Some(&Event::End));

    let (rebuilt, events) = scan(RECONSTRUCT);
    assert!(RECONSTRUCT.contains("Exec   : reconstructPar -latestTime"));
    assert_eq!(rebuilt.iterations, 1);
    assert_eq!(rebuilt.last_time, Some(120.0));
    assert!(rebuilt.ended);
    assert_eq!(rebuilt.fatal, None);
    assert_eq!(events.last(), Some(&Event::End));
}

#[test]
fn no_real_log_of_a_run_that_started_matches_a_known_error() {
    for log in [CAVITY, DIVERGED, DECOMPOSE, PARALLEL, RECONSTRUCT] {
        assert_eq!(explain(log), ErrorId::Unknown);
    }
}

#[test]
fn the_new_real_files_are_plain_ascii_with_the_banner_and_without_host_or_process() {
    for log in [CYLINDER, HEATED, STEP, POTENTIAL, MESH_FATAL] {
        assert!(log.is_ascii());
        assert!(log.contains("Floating point exception trapping enabled (FOAM_SIGFPE)."));
        assert!(log.contains("Host   : redacted"));
        assert!(log.contains("PID    : redacted"));
    }
}

#[test]
fn a_real_transient_cylinder_log_has_courant_numbers_and_two_continuity_blocks_per_step() {
    let (state, events) = scan(CYLINDER);
    assert!(CYLINDER.contains("Exec   : pimpleFoam"));
    assert_eq!(state.iterations, 60);
    assert_eq!(state.last_time, Some(0.06));
    assert_eq!(
        state
            .first_initial
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["Ux", "Uy", "p"]
    );
    assert_eq!(state.first_initial["Ux"], 1.0);
    assert_eq!(state.first_initial["Uy"], 1.0);
    assert_eq!(state.first_initial["p"], 1.0);
    assert_eq!(state.last_initial["Ux"], 0.00370435736983);
    assert_eq!(state.last_initial["Uy"], 0.00535167352718);
    assert_eq!(state.last_initial["p"], 0.000131891851476);
    assert_eq!(state.max_courant, Some(0.544774103266));
    assert_eq!(state.min_local, Some(1.08842522588e-11));
    assert_eq!(state.local_errors.len(), LOCAL_WINDOW);
    assert_eq!(state.local_errors.last(), Some(&2.02327088097e-11));
    assert_eq!(state.execution_seconds, Some(0.66));
    assert!(state.ended);
    assert_eq!(state.fatal, None);
    assert!(!state.non_finite);
    assert_eq!(count(&events, |e| matches!(e, Event::Time { .. })), 60);
    assert_eq!(count(&events, |e| matches!(e, Event::Residual { .. })), 360);
    assert_eq!(
        count(&events, |e| matches!(e, Event::Continuity { .. })),
        120
    );
    assert_eq!(count(&events, |e| matches!(e, Event::Courant { .. })), 62);
    assert_eq!(
        count(&events, |e| matches!(e, Event::ExecutionTime { .. })),
        60
    );
    assert_eq!(events.last(), Some(&Event::End));
    assert_eq!(divergence(&state, &Limits::default(), true), None);
}

#[test]
fn a_real_buoyant_log_solves_for_the_temperature_and_the_pressure_without_the_hydrostatic_part() {
    let (state, events) = scan(HEATED);
    assert!(HEATED.contains("Exec   : buoyantBoussinesqSimpleFoam"));
    assert!(HEATED.contains("Selecting turbulence model type laminar"));
    assert_eq!(state.iterations, 60);
    assert_eq!(state.last_time, Some(60.0));
    assert_eq!(
        state
            .first_initial
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["T", "Ux", "Uy", "p_rgh"]
    );
    assert_eq!(state.first_initial["T"], 0.999999999999);
    assert_eq!(state.first_initial["p_rgh"], 1.0);
    assert_eq!(state.last_initial["T"], 0.0030109448361);
    assert_eq!(state.last_initial["Ux"], 0.0135155263841);
    assert_eq!(state.last_initial["Uy"], 0.0108961292103);
    assert_eq!(state.last_initial["p_rgh"], 0.0120987471544);
    assert_eq!(state.min_local, Some(1.94010887957e-06));
    assert_eq!(state.local_errors.last(), Some(&1.94010887957e-06));
    assert_eq!(state.execution_seconds, Some(0.15));
    assert!(state.ended);
    assert_eq!(state.fatal, None);
    assert_eq!(count(&events, |e| matches!(e, Event::Residual { .. })), 240);
    assert_eq!(check(&state), None);
}

#[test]
fn a_real_turbulent_step_log_solves_for_omega_and_k_after_the_momentum_and_pressure() {
    let (state, events) = scan(STEP);
    assert!(STEP.contains("Exec   : simpleFoam"));
    assert_eq!(state.iterations, 60);
    assert_eq!(state.last_time, Some(60.0));
    assert_eq!(
        state
            .first_initial
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["Ux", "Uy", "k", "omega", "p"]
    );
    assert_eq!(state.first_initial["omega"], 0.997647844082);
    assert_eq!(state.first_initial["k"], 0.999999999999);
    assert_eq!(state.last_initial["Ux"], 0.0175307031442);
    assert_eq!(state.last_initial["Uy"], 0.112633954768);
    assert_eq!(state.last_initial["p"], 0.0838950598202);
    assert_eq!(state.last_initial["omega"], 0.00378333999258);
    assert_eq!(state.last_initial["k"], 0.00432734210061);
    assert_eq!(state.min_local, Some(0.366694782409));
    assert_eq!(state.local_errors.last(), Some(&0.427182598605));
    assert_eq!(state.execution_seconds, Some(1.84));
    assert!(state.ended);
    assert_eq!(state.fatal, None);
    assert_eq!(count(&events, |e| matches!(e, Event::Residual { .. })), 300);
    // Sixty iterations from a uniform start are far from converged, but nothing is blowing up.
    assert_eq!(check(&state), None);
}

#[test]
fn a_real_potential_flow_log_has_a_potential_residual_and_no_time_steps() {
    let (state, events) = scan(POTENTIAL);
    assert!(POTENTIAL.contains("Exec   : potentialFoam"));
    assert_eq!(state.iterations, 0);
    assert_eq!(state.last_time, None);
    assert_eq!(
        state
            .first_initial
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["Phi"]
    );
    assert_eq!(state.first_initial["Phi"], 1.0);
    assert_eq!(state.last_initial["Phi"], 7.70625305293e-07);
    assert_eq!(state.execution_seconds, Some(0.16));
    assert!(state.ended);
    assert_eq!(state.fatal, None);
    assert_eq!(count(&events, |e| matches!(e, Event::Residual { .. })), 3);
    assert_eq!(events.last(), Some(&Event::End));
}

#[test]
fn a_real_mesh_dictionary_error_is_a_fatal_line_that_the_error_table_does_not_know() {
    // blockMesh refused a symmetry plane that was not planar.
    assert!(MESH_FATAL.contains("Symmetry plane 'startup' is not planar"));
    let (state, events) = scan(MESH_FATAL);
    assert_eq!(state.fatal, Some(FatalKind::FoamFatal));
    assert_eq!(
        count(&events, |e| matches!(e, Event::Fatal { .. })),
        1,
        "reported once"
    );
    assert_eq!(state.iterations, 0);
    assert!(!state.ended, "blockMesh printed no End");
    assert!(!state.non_finite);
    // The table has no entry for a mesh dictionary error: it falls through to Unknown.
    assert_eq!(explain(MESH_FATAL), ErrorId::Unknown);
}

#[test]
fn the_new_real_logs_cut_into_pieces_give_the_same_events_and_state() {
    for log in [CYLINDER, HEATED, STEP, POTENTIAL, MESH_FATAL] {
        let (whole_state, whole_events) = scan(log);
        let mut pieces = Scanner::new();
        let mut events = Vec::new();
        for piece in log.as_bytes().chunks(4001) {
            events.extend(pieces.feed(std::str::from_utf8(piece).expect("ascii")));
        }
        assert_eq!(events, whole_events);
        assert_eq!(pieces.state(), &whole_state);
    }
}

#[test]
fn no_new_real_log_of_a_run_that_started_matches_a_known_error() {
    for log in [CYLINDER, HEATED, STEP, POTENTIAL] {
        assert_eq!(explain(log), ErrorId::Unknown);
    }
}
