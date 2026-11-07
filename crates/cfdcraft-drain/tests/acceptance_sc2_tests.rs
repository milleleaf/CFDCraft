//! Probes, a line and a surface integral defined in the study are watched while the run goes
//! on, shown next to the residuals with their last value, tail mean, spread and a word, and kept
//! in the recorded result. The numbers come from the files the drain has copied; the host is
//! never asked.

use cfdcraft_assess::assess;
use cfdcraft_capture::entry_names;
use cfdcraft_drain::watch::{self, RowView};
use cfdcraft_ops::convergence::{assess_dir, files_from_bundle};
use cfdcraft_ops::save_document;
use cfdcraft_queue::JobState;
use cfdcraft_settle::{Settings, Status, VerdictState};
use cfdcraft_store::Store;
use serde_json::json;

mod support;
use support::*;

#[path = "support/acceptance.rs"]
mod acceptance;
use acceptance::*;

/// The quantities of the listed monitors, in the order the table shows them.
const NAMES: [&str; 5] = [
    "probe_1.Ux",
    "probe_1.Uy",
    "line_1.mean_Ux",
    "line_1.mean_Uy",
    "surface_1.flux",
];

fn states_of(rows: &[RowView]) -> Vec<Status> {
    rows.iter().map(|row| row.state).collect()
}

#[test]
fn monitors_defined_before_the_run_are_watched_next_to_the_residuals() {
    // Criterion 2: probes, a line and a surface defined in the study are mirrored live and shown
    // with last value, tail mean, spread and a settled or drifting word.
    let w = monitored_world("sc2-watch");
    let id = w.add();
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &id);

    // Early: twenty iterations and two write times of the line.
    w.shared.grow_log(&run_id, 1, 20);
    put_files(
        &w,
        &run_id,
        &listed_files(20, 2, Shape::Settled, Shape::Settled),
    );
    drain.tick().unwrap();
    let early = watch::monitors(&w.doc, &id).unwrap();
    assert_eq!(early.iterations, 20);
    let names: Vec<&str> = early.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, NAMES);
    let labels: Vec<Option<&str>> = early.rows.iter().map(|r| r.label.as_deref()).collect();
    assert_eq!(
        labels,
        [
            Some("centre.Ux"),
            Some("centre.Uy"),
            Some("mid.mean_Ux"),
            Some("mid.mean_Uy"),
            Some("lidflux.flux")
        ]
    );
    assert_eq!(
        states_of(&early.rows),
        [
            Status::TooEarly,
            Status::TooEarly,
            Status::TooFewSamples,
            Status::TooFewSamples,
            Status::TooEarly
        ]
    );
    assert!(row_sentence(&early.rows[0]).ends_with(": too early to tell."));
    assert!(row_sentence(&early.rows[2]).ends_with(": too few samples to tell."));

    // The residuals are shown beside them, from the same copies.
    let given = live_spec(&w, &id);
    let found = assess_dir(&w.live(&id), &given.spec, &given.settings);
    let fields: Vec<&str> = found.residuals.iter().map(|r| r.field.as_str()).collect();
    assert_eq!(fields, ["Ux", "Uy", "p"]);
    for residual in &found.residuals {
        assert!(residual.last < residual.first, "{residual:?}");
    }

    // Later: three hundred iterations and twenty write times. Every quantity has settled.
    w.shared.grow_log(&run_id, 21, 300);
    put_files(
        &w,
        &run_id,
        &listed_files(300, 20, Shape::Settled, Shape::Settled),
    );
    drain.tick().unwrap();
    let calls = w.shared.calls().len();
    let later = watch::monitors(&w.doc, &id).unwrap();
    assert_eq!(
        w.shared.calls().len(),
        calls,
        "the watcher never asks the host"
    );
    assert_eq!(later.iterations, 300);
    assert_eq!(later.state, JobState::Running);
    assert_eq!(states_of(&later.rows), [Status::Settled; 5]);
    for row in &later.rows {
        assert!(row.last.is_finite() && row.mean.is_finite() && row.spread >= 0.0);
        let sentence = row_sentence(row);
        assert!(sentence.contains(": last "), "{sentence}");
        assert!(sentence.contains(", tail mean "), "{sentence}");
        assert!(sentence.contains(", spread "), "{sentence}");
        assert!(sentence.ends_with(": settled."), "{sentence}");
    }
    assert!((later.rows[0].last - 0.03).abs() < 1e-3);
}

#[test]
fn a_drifting_quantity_is_called_drifting_with_its_numbers() {
    // Criterion 2: a quantity that is still moving is called drifting, with its numbers.
    let w = monitored_world("sc2-drifting");
    let id = w.add();
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &id);
    w.shared.grow_log(&run_id, 1, 300);
    put_files(
        &w,
        &run_id,
        &listed_files(300, 20, Shape::Ramp, Shape::Settled),
    );
    drain.tick().unwrap();

    let view = watch::monitors(&w.doc, &id).unwrap();
    assert_eq!(
        states_of(&view.rows),
        [
            Status::Drifting,
            Status::Drifting,
            Status::Settled,
            Status::Settled,
            Status::Settled
        ]
    );
    let drifting = &view.rows[0];
    assert!(drifting.spread > 0.0);
    assert!(
        drifting.last > drifting.mean,
        "a climbing series ends above its mean"
    );
    assert!(
        row_sentence(drifting).ends_with(": drifting."),
        "{}",
        row_sentence(drifting)
    );
    let numbers = view
        .verdict
        .quantities
        .iter()
        .find(|q| q.name == "probe_1.Ux")
        .expect("the probe is in the verdict");
    assert!(numbers.drift != 0.0, "{numbers:?}");
    assert_eq!(numbers.status, Status::Drifting);

    // And it is why the run is not called converged.
    assert_eq!(view.verdict.state, VerdictState::NotConverged);
    // A run still in progress has not reached its residual limit either, so that reason follows.
    assert_eq!(
        view.verdict.reasons[..2],
        ["not_settled:probe_1.Ux", "not_settled:probe_1.Uy"]
    );
}

#[test]
fn the_recorded_result_keeps_the_series_and_the_verdict() {
    // Criterion 2: the monitors are kept in the recorded result, and the verdict made again from
    // them is the verdict recorded.
    let w = monitored_world("sc2-recorded");
    let id = w.add();
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &id);
    play_on(&w, &run_id);
    put_files(
        &w,
        &run_id,
        &listed_files(300, 20, Shape::Settled, Shape::SlowRamp),
    );
    assert_eq!(settle(&w, &mut drain, &id, 8), JobState::Finished);

    let runs = w.runs();
    assert_eq!(runs.len(), 1);
    let recorded = runs[0].verdict.clone().expect("a verdict is recorded");
    assert_eq!(recorded.state, VerdictState::NotConverged);
    assert_eq!(recorded.reasons, ["not_settled:surface_1.flux"]);

    // The stored result holds the monitor files of all three kinds.
    let blob = runs[0].result.as_ref().expect("a result is stored").blob;
    let bytes = Store::open(cfdcraft_doc::store_dir(&w.doc))
        .unwrap()
        .read(blob)
        .unwrap();
    let names = entry_names(&bytes).unwrap_or_default();
    for wanted in [
        "postProcessing/probe_1/0/U",
        "postProcessing/surface_1/0/surfaceFieldValue.dat",
    ] {
        assert!(names.iter().any(|n| n == wanted), "{wanted} in {names:?}");
    }
    let line_files = names
        .iter()
        .filter(|n| n.starts_with("postProcessing/line_1/") && n.ends_with("/line_U.xy"))
        .count();
    assert_eq!(line_files, 20);

    // The same settings give the recorded verdict; a looser drift tolerance gives another one.
    let given = live_spec(&w, &id);
    let files = files_from_bundle(&bytes, &given.spec);
    let again = assess(&given.spec, &files, &given.settings);
    assert_eq!(again.verdict, recorded);
    let looser = Settings {
        drift_rel: 0.01,
        ..given.settings.clone()
    };
    let relaxed = assess(&given.spec, &files, &looser);
    assert_eq!(relaxed.verdict.state, VerdictState::Converged);
    assert_ne!(relaxed.verdict, recorded);

    // The recorded run is read back the same way, with the study's own names.
    let shown = watch::monitors_of_run(&w.doc, "run-1").unwrap();
    assert_eq!(shown.verdict, recorded);
    assert_eq!(shown.recorded, Some(recorded.clone()));
    let flux = shown
        .rows
        .iter()
        .find(|row| row.name == "surface_1.flux")
        .expect("the surface is in the table");
    assert_eq!(flux.label.as_deref(), Some("lidflux.flux"));
    assert_eq!(flux.state, Status::Drifting);

    // The tolerance judges the run and does not change what is run: the result stays up to date.
    let before = status(&w.doc);
    assert_eq!(before.result.state, "fresh");
    let relaxed_study = edit(&listed_study(), |study| {
        study["monitors"]["settle"] = json!({"drift_rel": 0.01});
    });
    passed(&save_document(&relaxed_study, &w.doc, true).report);
    let after = status(&w.doc);
    assert_eq!(after.result.state, "fresh");
    assert_eq!(after.result.key, before.result.key);
}
