use cfdcraft_queue::{
    Job, JobKind, JobState, MAX_BYTES, Origin, Progress, Queue, QueueError, StopCause, StopMode,
    StopRequest,
};

fn sample() -> Queue {
    Queue {
        schema_version: 1,
        next_number: 4,
        jobs: vec![
            Job {
                id: "q-001".to_owned(),
                scenario: Some("base".to_owned()),
                kind: JobKind::Continue {
                    run: "run-3".to_owned(),
                },
                origin: Some(Origin {
                    sweep: "lid".to_owned(),
                    index: 2,
                }),
                state: JobState::Running,
                attempts: 1,
                run_id: Some("cfdcraft-demo-1".to_owned()),
                run: None,
                code: None,
                stop: Some(StopRequest {
                    mode: StopMode::WriteNow,
                    cause: StopCause::User,
                }),
                progress: Some(Progress {
                    time: Some(0.5),
                    iterations: 10,
                    settled: 1,
                    drifting: 2,
                    monitors: 3,
                    updated: 7,
                    summary: Some("2 of 3 settled".to_owned()),
                }),
            },
            Job {
                id: "q-002".to_owned(),
                scenario: None,
                kind: JobKind::Solve,
                origin: None,
                state: JobState::Queued,
                attempts: 0,
                run_id: None,
                run: None,
                code: None,
                stop: None,
                progress: None,
            },
        ],
    }
}

#[test]
fn a_good_queue_round_trips_through_json() {
    let queue = sample();
    let text = queue.to_json();
    assert!(text.ends_with("}\n"));
    assert_eq!(Queue::from_json(&text).unwrap(), queue);

    // An empty file body is a queue with no jobs, and a missing counter starts at 1.
    let bare = Queue::from_json("{\"schema_version\": 1}").unwrap();
    assert_eq!(bare, Queue::new());
}

#[test]
fn unknown_members_and_a_newer_schema_are_refused() {
    let newer = Queue::from_json("{\"schema_version\": 2, \"jobs\": []}").unwrap_err();
    assert_eq!(newer, QueueError::NewerSchema { found: 2 });

    // The version is read first: a newer file is called newer even when its shape has changed.
    let changed = Queue::from_json("{\"schema_version\": 3, \"extra\": true}").unwrap_err();
    assert_eq!(changed, QueueError::NewerSchema { found: 3 });

    for text in [
        "{\"schema_version\": 1, \"extra\": true}",
        "{\"schema_version\": 1, \"jobs\": [{\"id\": \"q-001\", \"kind\": \"solve\", \"state\": \"queued\", \"extra\": 1}]}",
        "{\"schema_version\": 1, \"jobs\": [{\"id\": \"q-001\", \"kind\": \"explode\", \"state\": \"queued\"}]}",
        "{\"schema_version\": 1, \"jobs\": [{\"id\": \"q-001\", \"kind\": \"solve\", \"state\": \"paused\"}]}",
        "{\"jobs\": []}",
        "{\"schema_version\": 0}",
        "{\"schema_version\": \"1\"}",
        "[]",
        "not json",
        "",
    ] {
        let err = Queue::from_json(text).unwrap_err();
        assert!(matches!(err, QueueError::Damaged { .. }), "{text}: {err}");
    }

    // A file that is well formed but breaks a rule is refused by the rule.
    let broken = "{\"schema_version\": 1, \"jobs\": [{\"id\": \"q-001\", \"kind\": \"solve\", \"state\": \"running\"}]}";
    assert!(matches!(
        Queue::from_json(broken).unwrap_err(),
        QueueError::Damaged { .. }
    ));

    // A file that is too large is refused before it is parsed.
    let huge = format!(
        "{{\"schema_version\": 1, \"pad\": \"{}\"}}",
        "x".repeat(MAX_BYTES)
    );
    assert!(matches!(
        Queue::from_json(&huge).unwrap_err(),
        QueueError::Damaged { .. }
    ));
}

#[test]
fn the_json_shape_is_stable() {
    let expected = r#"{
  "schema_version": 1,
  "next_number": 4,
  "jobs": [
    {
      "id": "q-001",
      "scenario": "base",
      "kind": {
        "continue": {
          "run": "run-3"
        }
      },
      "origin": {
        "sweep": "lid",
        "index": 2
      },
      "state": "running",
      "attempts": 1,
      "run_id": "cfdcraft-demo-1",
      "stop": {
        "mode": "write_now",
        "cause": "user"
      },
      "progress": {
        "time": 0.5,
        "iterations": 10,
        "settled": 1,
        "drifting": 2,
        "monitors": 3,
        "updated": 7,
        "summary": "2 of 3 settled"
      }
    },
    {
      "id": "q-002",
      "kind": "solve",
      "state": "queued"
    }
  ]
}
"#;
    assert_eq!(sample().to_json(), expected);
}
