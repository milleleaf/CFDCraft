//! The optional backend member of a job: an old file reads as before, a named backend round trips,
//! and an unknown member is still refused.

use cfdcraft_queue::{Queue, QueueError};

/// A queue file as it was written before the member existed.
const OLD: &str = r#"{
  "schema_version": 1,
  "next_number": 2,
  "jobs": [
    {
      "id": "q-001",
      "kind": "solve",
      "state": "queued"
    }
  ]
}
"#;

#[test]
fn an_old_job_reads_and_writes_unchanged() {
    let queue = Queue::from_json(OLD).unwrap();
    assert_eq!(queue.jobs[0].backend, None);
    let text = queue.to_json();
    assert!(!text.contains("backend"), "{text}");
    // Reading what was written gives the same queue, and writing that again gives the same text.
    let again = Queue::from_json(&text).unwrap();
    assert_eq!(again, queue);
    assert_eq!(again.to_json(), text);
}

#[test]
fn a_named_backend_round_trips() {
    let mut queue = Queue::from_json(OLD).unwrap();
    queue.jobs[0].backend = Some("esi-v2606".to_owned());
    let text = queue.to_json();
    assert!(text.contains("\"backend\": \"esi-v2606\""), "{text}");
    assert_eq!(Queue::from_json(&text).unwrap(), queue);

    // An id with dots, such as the ones that carry a solver version, is plain text too.
    queue.jobs[0].backend = Some("su2-8.6.0".to_owned());
    assert_eq!(Queue::from_json(&queue.to_json()).unwrap(), queue);
}

#[test]
fn an_unknown_member_is_still_refused_and_a_hostile_id_is_damage() {
    let unknown = OLD.replace(
        "\"state\": \"queued\"",
        "\"state\": \"queued\", \"engine\": \"x\"",
    );
    assert!(matches!(
        Queue::from_json(&unknown),
        Err(QueueError::Damaged { .. })
    ));

    let long = "a".repeat(41);
    for bad in [
        "",
        "ESI",
        "a b",
        "x; rm -rf /",
        "../x",
        "a\\nb",
        long.as_str(),
    ] {
        let text = OLD.replace(
            "\"state\": \"queued\"",
            &format!(
                "\"state\": \"queued\", \"backend\": {}",
                serde_json::to_string(bad).unwrap()
            ),
        );
        assert!(
            matches!(Queue::from_json(&text), Err(QueueError::Damaged { .. })),
            "{bad:?}"
        );
    }
}
