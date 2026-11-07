//! The lock contract at unit level: holder validation, the remote command text,
//! busy parsing and staleness. A busy lock is not a failure.

use cfdcraft_executor::lock::{Holder, acquire_command, parse_busy_output, release_command};

fn holder() -> Holder {
    Holder::new(
        "cfdcraft-cavity-42-7",
        "cfdcraft-cavity-42-7",
        1_000,
        600,
        "test-client_1.0",
    )
    .unwrap()
}

const LOCK: &str = "/data/cfdcraft/work/.cfdcraft-job-lock";

#[test]
fn holder_round_trips_and_rejects_shell_characters() {
    let h = holder();
    let back: Holder = serde_json::from_str(&h.to_json()).unwrap();
    assert_eq!(back, h);
    for bad in ["a b", "a'b", "a;b", "a/b", "a\"b", "a$b", "a`b", "", "a\nb"] {
        assert!(Holder::new(bad, "c", 0, 1, "x").is_err(), "run id {bad:?}");
        assert!(
            Holder::new("r", bad, 0, 1, "x").is_err(),
            "container {bad:?}"
        );
    }
    for bad in ["a b", "a'b", "a;b", "a/b", "a\"b", "", "a\nb"] {
        assert!(Holder::new("r", "c", 0, 1, bad).is_err(), "client {bad:?}");
    }
}

#[test]
fn acquire_command_only_carries_validated_text() {
    let h = holder();
    let cmd = acquire_command(LOCK, &h.to_json()).unwrap();
    assert!(cmd.contains(LOCK) && cmd.contains(&h.run_id));
    // The holder JSON is the only quoted payload, and it has no single quote.
    assert!(!h.to_json().contains('\''));
    // A single quote anywhere in the holder text is refused.
    assert!(acquire_command(LOCK, &h.to_json().replace("test-client_1.0", "x'y")).is_err());
    // A holder built by hand around the constructor is still refused.
    let mut sneaky = h.clone();
    sneaky.run_id = "a'; touch pwned; '".into();
    assert!(acquire_command(LOCK, &sneaky.to_json()).is_err());
    // So is any path that is not plain.
    for bad in ["", "/a b/lock", "/a'b", "/a;b", "/a$(x)"] {
        assert!(acquire_command(bad, &h.to_json()).is_err(), "{bad:?}");
        assert!(release_command(bad).is_err(), "{bad:?}");
    }
    assert!(acquire_command(LOCK, "not json").is_err());
}

#[test]
fn busy_output_without_a_valid_holder_is_none() {
    let h = holder();
    assert_eq!(
        parse_busy_output(&format!("{}\n", h.to_json())),
        Some(h.clone())
    );
    for bad in ["", "   \n", "{\"run_id\":\"x\"", "garbage", "[]", "{}"] {
        assert_eq!(parse_busy_output(bad), None, "{bad:?}");
    }
    // Well-formed JSON with hostile field text is not a holder either.
    let mut hostile = h;
    hostile.container = "x y".into();
    assert_eq!(parse_busy_output(&hostile.to_json()), None);
}

#[test]
fn staleness_boundary_and_remaining_time() {
    let h = holder(); // started 1000, cap 600
    let edge = 1_000 + 600 + 300;
    assert!(!h.is_stale(edge, 300));
    assert!(h.is_stale(edge + 1, 300));
    assert!(!h.is_stale(0, 300));
    assert_eq!(h.remaining_seconds(1_000), 600);
    assert_eq!(h.remaining_seconds(1_599), 1);
    assert_eq!(h.remaining_seconds(1_600), 0);
    assert_eq!(h.remaining_seconds(u64::MAX), 0);
    let huge = Holder::new("r", "c", u64::MAX, u64::MAX, "x").unwrap();
    assert!(!huge.is_stale(u64::MAX, u64::MAX));
}

/// The remote text itself, run for real in a local shell against a temp dir:
/// first caller wins, second is busy and sees the holder, release frees it, and
/// a missing parent is neither (exit 73).
#[cfg(unix)]
#[test]
fn acquire_and_release_text_behave_in_a_real_shell() {
    use std::process::Command;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/executor-lock-text")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let path = format!("{}/.cfdcraft-job-lock", root.display());
    let sh = |cmd: &str| Command::new("sh").args(["-c", cmd]).output().unwrap();
    let h = holder();
    let acquire = acquire_command(&path, &h.to_json()).unwrap();

    let first = sh(&acquire);
    assert_eq!(first.status.code(), Some(0));
    let second = sh(&acquire);
    assert_eq!(second.status.code(), Some(75));
    assert_eq!(
        parse_busy_output(&String::from_utf8_lossy(&second.stdout)),
        Some(h)
    );
    assert_eq!(sh(&release_command(&path).unwrap()).status.code(), Some(0));
    assert!(!std::path::Path::new(&path).exists());

    let orphan = format!("{}/missing/.cfdcraft-job-lock", root.display());
    let lost = sh(&acquire_command(&orphan, &holder().to_json()).unwrap());
    assert_eq!(lost.status.code(), Some(73));
    let _ = std::fs::remove_dir_all(&root);
}
