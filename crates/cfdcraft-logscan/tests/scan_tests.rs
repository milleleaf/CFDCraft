mod support;

use cfdcraft_logscan::{Event, FatalKind, Scanner};
use support::{P0, UX0, cavity_log_with_end, fall, near};

const FINAL_ITERATION: &str =
    include_str!("../../cfdcraft-backend-esi/tests/fixtures/v2512-simpleFoam-final-iteration.log");

#[test]
fn a_log_in_two_pieces_equals_the_whole_log() {
    let log = cavity_log_with_end(1, 20, 0.8);
    assert!(log.is_ascii(), "cuts must fall on character boundaries");
    let mut whole = Scanner::new();
    let whole_events = whole.feed(&log);
    assert!(whole_events.len() > 100);
    for cut in 0..=log.len() {
        let mut parts = Scanner::new();
        let mut events = parts.feed(&log[..cut]);
        events.extend(parts.feed(&log[cut..]));
        assert_eq!(events, whole_events, "events differ when cut at {cut}");
        assert_eq!(
            parts.state(),
            whole.state(),
            "state differs when cut at {cut}"
        );
    }
    let mut bytes = Scanner::new();
    let mut events = Vec::new();
    for i in 0..log.len() {
        events.extend(bytes.feed(&log[i..=i]));
    }
    assert_eq!(events, whole_events);
    assert_eq!(bytes.state(), whole.state());
}

#[test]
fn residuals_time_and_end_are_read() {
    let mut scanner = Scanner::new();
    scanner.feed(&cavity_log_with_end(1, 20, 0.8));
    let state = scanner.state();
    assert_eq!(state.iterations, 20);
    assert_eq!(state.last_time, Some(20.0));
    assert!(near(state.first_initial["Ux"], UX0));
    assert!(near(state.last_initial["p"], fall(P0, 1, 20, 0.8)));
    assert!(state.ended);
    assert_eq!(state.execution_seconds, Some(10.0));
    assert_eq!(state.fatal, None);
    assert!(!state.non_finite);
}

#[test]
fn a_partial_last_line_waits_for_its_newline() {
    let mut scanner = Scanner::new();
    let first = scanner.feed("Time = 7\nsmoothSolver:  Solving for Ux, Initial res");
    assert_eq!(first, vec![Event::Time { time: 7.0 }]);
    assert!(scanner.state().last_initial.is_empty());
    let second = scanner.feed("idual = 1.5e-3, Final residual = 2.0e-8, No Iterations 4\n");
    assert_eq!(
        second,
        vec![Event::Residual {
            field: "Ux".to_string(),
            initial: 1.5e-3,
            final_: 2.0e-8,
            iterations: 4,
        }]
    );
}

#[test]
fn the_real_final_iteration_fixture_parses() {
    let mut scanner = Scanner::new();
    let events = scanner.feed(FINAL_ITERATION);
    let fields: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Event::Residual { field, .. } => Some(field.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(fields, ["Ux", "Uy", "p"]);
    let state = scanner.state();
    assert!(state.ended);
    assert_eq!(state.last_time, Some(2000.0));
    assert_eq!(state.execution_seconds, Some(108.42));
    assert_eq!(state.fatal, None, "the start-up banner is not a trap");
    assert!(!state.non_finite);
    assert!(!events.iter().any(|e| matches!(e, Event::Fatal { .. })));
}

#[test]
fn foundation_style_times_are_read() {
    let mut scanner = Scanner::new();
    let events = scanner.feed("Time = 1s\nTime = 2.5s\n");
    assert_eq!(
        events,
        vec![Event::Time { time: 1.0 }, Event::Time { time: 2.5 }]
    );
    assert_eq!(scanner.state().last_time, Some(2.5));
}

#[test]
fn continuity_and_courant_are_read() {
    let mut scanner = Scanner::new();
    let events = scanner.feed(
        "time step continuity errors : sum local = 1e-3, global = 2e-9, cumulative = 3e-9\n\
         Courant Number mean: 0.1 max: 0.4\n\
         time step continuity errors : sum local = 1e-5, global = 2e-9, cumulative = 5e-9\n\
         Courant Number mean: 0.2 max: 0.9\n\
         time step continuity errors : sum local = 1e-4, global = 2e-9, cumulative = 7e-9\n\
         Courant Number mean: 0.1 max: 0.2\n",
    );
    assert_eq!(events.len(), 6);
    assert_eq!(
        events[0],
        Event::Continuity {
            local: 1e-3,
            global: 2e-9,
            cumulative: 3e-9,
        }
    );
    assert_eq!(
        events[1],
        Event::Courant {
            mean: 0.1,
            max: 0.4
        }
    );
    let state = scanner.state();
    assert_eq!(state.local_errors, [1e-3, 1e-5, 1e-4]);
    assert_eq!(state.min_local, Some(1e-5));
    assert_eq!(state.max_courant, Some(0.9));
}

#[test]
fn finalising_after_end_is_remembered() {
    let mut scanner = Scanner::new();
    let events = scanner.feed("End\nFinalising parallel run\n");
    assert_eq!(events, vec![Event::End, Event::Finalising]);
    assert!(scanner.state().ended);
    assert!(scanner.state().finalising);
}

#[test]
fn an_unreadable_number_is_ignored_not_fatal() {
    let mut scanner = Scanner::new();
    let events = scanner.feed(
        "Time = 1\n\
         smoothSolver:  Solving for Ux, Initial residual = abc, Final residual = 1e-8, No Iterations 2\n\
         Time = zzz\n",
    );
    assert_eq!(events, vec![Event::Time { time: 1.0 }]);
    assert!(scanner.state().first_initial.is_empty());
    assert_eq!(scanner.state().iterations, 1);
}

#[test]
fn a_trap_is_reported_once_and_the_banner_is_not_one() {
    let mut scanner = Scanner::new();
    let banner = scanner.feed(
        "trapFpe: Floating point exception trapping enabled (FOAM_SIGFPE).\n\
         sigFpe : Enabling floating point exception trapping (FOAM_SIGFPE).\n",
    );
    assert!(banner.is_empty());
    let crash = scanner.feed(
        "#1  Foam::sigFpe::sigHandler(int) at ??:?\n\
         Floating point exception (core dumped)\n\
         Initial residual = nan\n\
         Initial residual = -nan\n",
    );
    assert_eq!(
        crash,
        vec![
            Event::Fatal {
                kind: FatalKind::FloatingPoint
            },
            Event::NonFinite,
        ]
    );
    assert_eq!(scanner.state().fatal, Some(FatalKind::FloatingPoint));
    assert!(scanner.state().non_finite);
}

#[test]
fn a_huge_line_is_skipped_whole_in_any_chunking() {
    let huge = "x".repeat(10_000);
    let mut whole = Scanner::new();
    let events = whole.feed(&format!("{huge}\nEnd\n"));
    assert_eq!(events, vec![Event::End]);
    let mut pieces = Scanner::new();
    let mut seen = Vec::new();
    seen.extend(pieces.feed(&huge[..5000]));
    seen.extend(pieces.feed(&huge[5000..]));
    seen.extend(pieces.feed("\nEnd\n"));
    assert_eq!(seen, vec![Event::End]);
    assert_eq!(pieces.state(), whole.state());
}
