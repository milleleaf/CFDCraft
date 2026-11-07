//! Two real processes start a run against the same fake box at the same time:
//! exactly one runs, exactly one is told the box is busy.
#![cfg(unix)]

mod support;

use std::{path::PathBuf, process::Stdio};
use support::*;

const GROUP: &str = "executor-race-tests";

/// Child side. Does nothing unless the parent set the root. Writes `won`,
/// `busy`, or the error text to `outcome/<pid>`. Anyone who did not win then
/// touches `release`, so the winner's fake `docker run` can finish; the winner
/// waits only for the loser's completion, never for its success.
#[test]
fn race_worker() {
    let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
        return;
    };
    let pid = std::process::id();
    let outcome = run_job(&root, "race", &root.join(format!("results-{pid}")));
    let text = match &outcome {
        Ok(_) => "won".to_string(),
        Err(e) if e.is_busy() => "busy".to_string(),
        Err(e) => format!("error: {e}"),
    };
    std::fs::create_dir_all(root.join("outcome")).unwrap();
    std::fs::write(root.join("outcome").join(pid.to_string()), &text).unwrap();
    if text != "won" {
        std::fs::write(root.join("release"), "").unwrap();
    }
}

fn race_once(tag: &str) {
    let root = new_root(GROUP, tag);
    let spawn = || {
        worker(&root, "race_worker", "hold")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    // Both are running before either is waited on.
    let (a, b) = (spawn(), spawn());
    for child in [a, b] {
        assert_worker_ok(&child.wait_with_output().unwrap());
    }

    let mut results: Vec<String> = std::fs::read_dir(root.join("outcome"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect();
    results.sort();
    assert_eq!(results, ["busy", "won"], "one winner, one busy refusal");
    let events = events(&root);
    assert_eq!(
        events.iter().filter(|e| *e == "run").count(),
        1,
        "exactly one docker run: {events:?}"
    );
    assert!(!lock_dir(&root).exists(), "no lock directory left");
    assert!(!root.join("running").exists(), "no container left");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn two_simultaneous_starts_have_exactly_one_winner() {
    race_once("a");
}
