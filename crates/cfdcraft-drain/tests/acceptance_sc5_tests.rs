//! A run is called converged only when every monitored quantity has settled and the balances
//! close. Small residuals alone never make the word, a drifting monitor, an open balance and a
//! missing monitor file each stop it, and the run is recorded all the same.

use cfdcraft_assess::{Files, Spec, assess};
use cfdcraft_drain::watch;
use cfdcraft_ops::convergence::files_from_dir;
use cfdcraft_queue::JobState;
use cfdcraft_settle::{BalanceStatus, Settings, Status, VerdictState};
use cfdcraft_text::{Line, Text};

mod support;
use support::*;

#[path = "support/acceptance.rs"]
mod acceptance;
use acceptance::*;

/// Runs the cavity to its end with `files` put in after the solver finishes, and returns the
/// world and the job.
fn ended_with(tag: &str, files: &[(String, String)]) -> (World, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &id);
    play_on(&w, &run_id);
    put_files(&w, &run_id, files);
    assert_eq!(settle(&w, &mut drain, &id, 8), JobState::Finished);
    (w, id)
}

/// The names of the quantities of a verdict that are not settled.
fn open(verdict: &cfdcraft_settle::Verdict) -> Vec<&str> {
    verdict
        .quantities
        .iter()
        .filter(|q| q.status != Status::Settled)
        .map(|q| q.name.as_str())
        .collect()
}

#[test]
fn a_converged_cavity_is_converged_and_the_balance_is_not_applicable() {
    // Criterion 5: converged when every monitored quantity has settled; a closed domain has no
    // inlet flux to balance, and says so instead of passing silently.
    let w = world("sc5-converged");
    let id = w.add();
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &id);
    play_on(&w, &run_id);
    assert_eq!(settle(&w, &mut drain, &id, 8), JobState::Finished);

    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    let verdict = runs[0].verdict.as_ref().expect("a verdict is recorded");
    assert_eq!(verdict.state, VerdictState::Converged, "{verdict:?}");
    assert!(verdict.reasons.is_empty());
    assert!(verdict.residuals_ok);
    assert_eq!(
        verdict.quantities.len(),
        10,
        "five probes, two components each"
    );
    assert_eq!(verdict.mass.status, BalanceStatus::NotApplicable);

    // The live view of the finished job says the same.
    let view = watch::monitors(&w.doc, &id).unwrap();
    assert!(view.ended);
    assert_eq!(view.verdict.state, VerdictState::Converged);
    assert_eq!(view.mass.status, BalanceStatus::NotApplicable);
    let sentence = Text::english().render(&Line::VerdictConverged);
    assert!(sentence.starts_with("Converged: "), "{sentence}");
}

#[test]
fn residual_only_is_never_converged() {
    // Criterion 5: a pass on residuals alone is not converged.
    let w = world("sc5-residual-only");
    let id = w.add();
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &id);
    play_bare(&w, &run_id);
    assert_eq!(settle(&w, &mut drain, &id, 8), JobState::Finished);

    // The residuals are small, and the run is recorded all the same.
    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].result.is_some());
    let verdict = runs[0].verdict.as_ref().expect("a verdict is recorded");
    assert!(verdict.residuals_ok, "the residuals are under their limits");
    assert_eq!(verdict.state, VerdictState::NotConverged);
    assert_ne!(verdict.state, VerdictState::Converged);
    for name in ["probe_1.Ux", "probe_1.Uy", "probe_5.Ux", "probe_5.Uy"] {
        let reason = format!("not_settled:{name}");
        assert!(verdict.reasons.contains(&reason), "{:?}", verdict.reasons);
    }
    let words = verdict.reasons.join(", ");
    let sentence = Text::english().render(&Line::VerdictNotConverged { reasons: words });
    assert!(sentence.starts_with("Not converged: "), "{sentence}");
}

#[test]
fn a_drifting_monitor_blocks_converged() {
    // Criterion 5: one monitor that is still moving stops the word, whatever else has settled.
    let ramp = (
        "postProcessing/probe_3/0/U".to_owned(),
        probe_text(300, Shape::Ramp, 0.03),
    );
    let (w, id) = ended_with("sc5-drifting", &[ramp]);

    let runs = w.runs();
    assert_eq!(runs.len(), 1, "the run is recorded");
    let verdict = runs[0].verdict.as_ref().expect("a verdict is recorded");
    assert_eq!(verdict.state, VerdictState::NotConverged);
    assert_eq!(
        verdict.reasons,
        ["not_settled:probe_3.Ux", "not_settled:probe_3.Uy"]
    );
    assert_eq!(open(verdict), ["probe_3.Ux", "probe_3.Uy"]);
    let settled = verdict
        .quantities
        .iter()
        .filter(|q| q.status == Status::Settled)
        .count();
    assert_eq!(settled, 8);

    let view = watch::monitors(&w.doc, &id).unwrap();
    assert_eq!(view.verdict.state, VerdictState::NotConverged);
}

#[test]
fn an_open_inlet_outlet_balance_blocks_converged_on_the_channel() {
    // Criterion 5: with an inlet, the balance has to close. The channel has an inlet and an outlet.
    let spec = channel_spec();
    assert!(spec.patches.iter().any(|p| p.inlet));
    let dir = scratch("sc5-channel").join("results");

    // The outlet gives back 1 % more than the inlet takes in.
    write_channel_run(&dir, &spec, 1.01, true);
    let files = files_from_dir(&dir, &spec);
    let open_one = assess(&spec, &files, &Settings::default());
    assert!(
        open_one
            .quantities
            .iter()
            .all(|q| q.status == Status::Settled),
        "every monitored quantity has settled: {:?}",
        open_one.quantities
    );
    assert_eq!(open_one.mass.status, BalanceStatus::Failed);
    let share = open_one.mass.imbalance.expect("the imbalance is measured");
    assert!((share - 0.01).abs() < 1e-6, "{share}");
    assert_eq!(open_one.verdict.state, VerdictState::NotConverged);
    assert_eq!(open_one.verdict.reasons, ["mass_imbalance"]);

    // Closing it gives the word.
    write_channel_run(&dir, &spec, 1.0, true);
    let files = files_from_dir(&dir, &spec);
    let closed = assess(&spec, &files, &Settings::default());
    assert_eq!(closed.mass.status, BalanceStatus::Ok);
    assert_eq!(closed.verdict.state, VerdictState::Converged);
    assert!(closed.verdict.reasons.is_empty());
}

#[test]
fn a_missing_flux_file_is_not_a_pass() {
    // Criterion 5: a balance that cannot be read cannot close.
    let spec = channel_spec();
    let dir = scratch("sc5-channel-missing").join("results");
    write_channel_run(&dir, &spec, 1.0, false);
    let files = files_from_dir(&dir, &spec);
    assert!(!files.monitor_files.contains_key("balance_outlet"));

    let found = assess(&spec, &files, &Settings::default());
    assert_eq!(found.mass.status, BalanceStatus::Failed);
    assert_eq!(found.mass.imbalance, None);
    assert!(found.notes.contains(&"missing:balance_outlet".to_owned()));
    assert_eq!(found.verdict.state, VerdictState::NotConverged);
    assert!(found.verdict.reasons.contains(&"mass_imbalance".to_owned()));
}

#[test]
fn a_run_with_nothing_monitored_is_not_judged() {
    // Criterion 5: no monitored quantity means nothing to call settled, whatever the residuals.
    let files = Files {
        log: "Time = 1\nEnd\n".to_owned(),
        ..Files::default()
    };
    let found = assess(&Spec::default(), &files, &Settings::default());
    assert_eq!(found.verdict.state, VerdictState::NotJudged);
    assert_eq!(found.verdict.reasons, ["no_monitored_quantity"]);
    let sentence = Text::english().render(&Line::VerdictNotJudged {
        reason: found.verdict.reasons.join(", "),
    });
    assert!(
        sentence.starts_with("Convergence cannot be judged: "),
        "{sentence}"
    );
}

#[test]
fn the_word_converged_appears_only_in_verdict_sentences() {
    // Criterion 5: no other sentence says a run converged.
    let text = Text::english();
    let mut found = Vec::new();
    for line in Line::samples() {
        let sentence = text.render(&line);
        if sentence.contains("onverged") {
            assert!(
                matches!(
                    line,
                    Line::VerdictConverged | Line::VerdictNotConverged { .. }
                ),
                "a sentence outside the verdict says converged: {sentence}"
            );
            found.push(sentence);
        }
    }
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(|s| s.starts_with("Converged: ")));
    assert!(found.iter().any(|s| s.starts_with("Not converged: ")));
}

#[test]
fn a_job_that_did_not_finish_is_never_shown_as_converged() {
    let (w, id) = ended_with("sc5-notfinished", &[]);
    assert_eq!(
        watch::monitors(&w.doc, &id).unwrap().verdict.state,
        VerdictState::Converged
    );

    // The same files under a job that failed, was stopped or was lost.
    for state in [
        JobState::Failed,
        JobState::Interrupted,
        JobState::Stopped,
        JobState::Cancelled,
    ] {
        let mut queue = w.queue();
        queue.jobs[0].state = state;
        queue.jobs[0].code = Some("CFD-SOLVER-099".to_owned());
        std::fs::write(
            cfdcraft_doc::store_dir(&w.doc).join("queue.json"),
            queue.to_json(),
        )
        .unwrap();
        let view = watch::monitors(&w.doc, &id).unwrap();
        assert_eq!(view.state, state);
        assert_eq!(view.verdict.state, VerdictState::NotConverged, "{state:?}");
        assert_eq!(view.verdict.reasons, ["not_finished"], "{state:?}");
        assert!(view.rows.iter().all(|r| r.state == Status::Settled));
    }
}
