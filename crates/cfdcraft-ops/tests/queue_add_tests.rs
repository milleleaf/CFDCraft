//! Adding a variant or a sweep to the queue: one job per thing asked for, nothing that has no work
//! to do, and a refused request that leaves the queue file exactly as it was. No test needs a
//! compute host or a container engine.

use std::fs;
use std::path::{Path, PathBuf};

use cfdcraft_doc::{open, store_dir};
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_ops::queue_ops::{AddRequest, queue_add, queue_list};
use cfdcraft_ops::scenario_ops::scenario_set;
use cfdcraft_ops::sweep_ops::sweep_create;
use cfdcraft_ops::{ExitClass, JobView, QueueData, Report, Repro, solve_document, solve_scenario};
use cfdcraft_queue::{Action, JobKind, NewJob};
use cfdcraft_scenarios::Override;
use cfdcraft_sweep::{Parameter, Sweep, Target, Values};
use cfdcraft_text::Line;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const LID: &str = "/boundary_conditions/lid/velocity/value";

fn cavity(tag: &str) -> PathBuf {
    new_document(&scratch(tag), "cavity", CAVITY)
}

fn queue_of(report: &Report) -> &QueueData {
    match &report.repro {
        Some(Repro::Queue(data)) => data.as_ref(),
        other => panic!("expected the queue, found {other:?}"),
    }
}

/// The bytes of `queue.json`, or none when there is no such file.
fn queue_bytes(doc: &Path) -> Option<Vec<u8>> {
    fs::read(store_dir(doc).join("queue.json")).ok()
}

fn for_sweep(id: &str) -> AddRequest {
    AddRequest {
        sweep: Some(id.to_owned()),
        ..AddRequest::default()
    }
}

fn for_variant(id: &str) -> AddRequest {
    AddRequest {
        scenario: Some(id.to_owned()),
        ..AddRequest::default()
    }
}

/// Three lid speeds, three points.
fn lids(id: &str) -> Sweep {
    Sweep {
        id: id.to_owned(),
        base: "base".to_owned(),
        parameters: vec![Parameter {
            name: "lid_speed".to_owned(),
            target: Target::Number {
                setting: LID.to_owned(),
            },
            values: Values::List {
                items: vec![0.2, 0.3, 0.4],
            },
        }],
    }
}

/// A sweep over `count` lid speeds spread from 0.01 to 0.5.
fn many_lids(id: &str, count: u32) -> Sweep {
    Sweep {
        id: id.to_owned(),
        base: "base".to_owned(),
        parameters: vec![Parameter {
            name: "lid_speed".to_owned(),
            target: Target::Number {
                setting: LID.to_owned(),
            },
            values: Values::Steps {
                start: 0.01,
                stop: 0.5,
                count,
            },
        }],
    }
}

/// Fill the queue of the document with `count` jobs for the study itself.
fn fill(doc: &Path, count: usize) {
    let jobs: Vec<NewJob> = (0..count)
        .map(|_| NewJob {
            scenario: None,
            kind: JobKind::Solve,
            origin: None,
            backend: None,
        })
        .collect();
    QueueFile::new(doc)
        .update(|_| Ok(vec![Action::Enqueue { jobs }]))
        .unwrap();
}

fn scenarios_of(jobs: &[JobView]) -> Vec<Option<&str>> {
    jobs.iter().map(|job| job.scenario.as_deref()).collect()
}

fn sentence(out: &cfdcraft_ops::DocOutcome) -> String {
    say(out.line.as_ref().expect("a sentence for the person"))
}

#[test]
fn a_variant_is_one_job_with_its_scenario() {
    let doc = cavity("qa-variant");
    let change = Override::Set {
        setting: LID.to_owned(),
        value: json!(0.2),
    };
    passed(&scenario_set(&doc, "fast-lid", Some("base"), &[change], None).report);

    let out = queue_add(&doc, &for_variant("fast-lid"));
    passed(&out.report);
    assert_eq!(sentence(&out), "Added 1 jobs to the queue; 1 are waiting.");
    let data = queue_of(&out.report);
    assert_eq!(data.jobs.len(), 1);
    assert_eq!(data.jobs[0].id, "q-001");
    assert_eq!(data.jobs[0].scenario.as_deref(), Some("fast-lid"));
    assert_eq!(data.jobs[0].kind, "solve");
    assert_eq!(data.jobs[0].origin, None);
}

#[test]
fn an_unknown_variant_is_refused_with_the_scenario_code() {
    let doc = cavity("qa-unknown");
    let out = queue_add(&doc, &for_variant("ghost"));
    assert_eq!(codes(&out.report), ["CFD-SCN-001"]);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(queue_bytes(&doc), None, "nothing is written");

    passed(&queue_add(&doc, &AddRequest::default()).report);
    let before = queue_bytes(&doc);
    let again = queue_add(&doc, &for_variant("ghost"));
    assert_eq!(codes(&again.report), ["CFD-SCN-001"]);
    assert_eq!(queue_bytes(&doc), before);
}

#[test]
fn a_sweep_is_one_job_per_point_in_point_order() {
    let doc = cavity("qa-sweep");
    passed(&sweep_create(&doc, &lids("lids")).report);

    let out = queue_add(&doc, &for_sweep("lids"));
    passed(&out.report);
    assert_eq!(sentence(&out), "Added 3 jobs to the queue; 3 are waiting.");
    let jobs = &queue_of(&out.report).jobs;
    let ids: Vec<&str> = jobs.iter().map(|job| job.id.as_str()).collect();
    assert_eq!(ids, ["q-001", "q-002", "q-003"]);
    assert_eq!(
        scenarios_of(jobs),
        [Some("lids-001"), Some("lids-002"), Some("lids-003")]
    );

    // Each job names the point it came from, as the stored variant does.
    let stored = open(&doc).unwrap().document.artifacts.scenarios;
    for job in jobs {
        let scenario = stored
            .iter()
            .find(|s| Some(s.id.as_str()) == job.scenario.as_deref())
            .expect("the job's variant is stored");
        let origin = scenario
            .origin
            .as_ref()
            .expect("a sweep point has an origin");
        assert_eq!(
            job.origin,
            Some(format!("{} {}", origin.sweep, origin.index))
        );
    }
    let indices: Vec<u32> = jobs
        .iter()
        .map(|job| {
            let text = job.origin.as_deref().unwrap();
            text.rsplit(' ').next().unwrap().parse().unwrap()
        })
        .collect();
    assert!(
        indices.windows(2).all(|pair| pair[0] < pair[1]),
        "{indices:?}"
    );
}

#[test]
fn points_already_up_to_date_are_left_out_and_counted() {
    let dir = scratch("qa-up-to-date");
    let doc = new_document(&dir, "cavity", CAVITY);
    passed(&sweep_create(&doc, &lids("lids")).report);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_scenario(&doc, Some("lids-002"), &mut runner).report);

    let out = queue_add(&doc, &for_sweep("lids"));
    passed(&out.report);
    assert_eq!(sentence(&out), "Added 2 jobs to the queue; 2 are waiting.");
    let jobs = &queue_of(&out.report).jobs;
    assert_eq!(scenarios_of(jobs), [Some("lids-001"), Some("lids-003")]);
}

#[test]
fn nothing_to_add_is_a_reading_not_a_failure() {
    let dir = scratch("qa-nothing");
    let doc = new_document(&dir, "cavity", CAVITY);
    let mut runner = FixtureRunner::new(&dir.join("runs"));
    passed(&solve_document(&doc, &mut runner).report);

    let out = queue_add(&doc, &AddRequest::default());
    passed(&out.report);
    assert_eq!(codes(&out.report), ["CFD-QUEUE-005"]);
    assert_eq!(out.line, Some(Line::QueueNothingToAdd));
    assert!(queue_of(&out.report).jobs.is_empty());
    assert_eq!(queue_bytes(&doc), None, "the queue is unchanged");
}

#[test]
fn a_job_already_waiting_is_not_added_twice() {
    let doc = cavity("qa-twice");
    passed(&queue_add(&doc, &AddRequest::default()).report);
    let before = queue_bytes(&doc);

    let again = queue_add(&doc, &AddRequest::default());
    passed(&again.report);
    assert_eq!(codes(&again.report), ["CFD-QUEUE-005"]);
    assert_eq!(again.line, Some(Line::QueueNothingToAdd));
    assert_eq!(queue_bytes(&doc), before);

    let listed = queue_list(&doc);
    assert_eq!(queue_of(&listed.report).jobs.len(), 1);
}

#[test]
fn two_selectors_at_once_are_refused() {
    let doc = cavity("qa-both");
    let both = AddRequest {
        scenario: Some("fast-lid".to_owned()),
        sweep: Some("lids".to_owned()),
        remake_pruned: false,
        continue_run: None,
        backend: None,
    };
    let out = queue_add(&doc, &both);
    assert_eq!(codes(&out.report), ["CFD-QUEUE-001"]);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(queue_bytes(&doc), None);
}

#[test]
fn more_than_the_job_limit_is_refused_whole() {
    let doc = cavity("qa-limit");
    passed(&sweep_create(&doc, &many_lids("big", 200)).report);
    fill(&doc, 900);
    let before = queue_bytes(&doc);
    assert!(before.is_some());

    let out = queue_add(&doc, &for_sweep("big"));
    assert_eq!(codes(&out.report), ["CFD-QUEUE-001"]);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert_eq!(queue_bytes(&doc), before, "none of the 200 was added");
}

#[test]
fn a_sweep_that_does_not_exist_is_refused() {
    let doc = cavity("qa-no-sweep");
    let out = queue_add(&doc, &for_sweep("ghost"));
    assert_eq!(codes(&out.report), ["CFD-SWP-004"]);
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    let sweep = "ghost".to_owned();
    assert_eq!(
        out.report.diagnostics[0].message,
        say(&Line::SweepUnknown { sweep })
    );
    assert_eq!(queue_bytes(&doc), None, "nothing is written");
}
