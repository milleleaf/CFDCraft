//! The `queue drain` verb, run through the real binary against a fake ssh and a fake docker: the
//! runner that starts and returns, the runner that stays in the foreground, the refusals before
//! anything is spawned, and that the key and the ssh target are never printed. Unix only: the
//! fakes are POSIX shell scripts.
#![cfg(unix)]

#[path = "../../cfdcraft-ops/tests/repro_support/mod.rs"]
mod repro_support;
#[path = "../../cfdcraft-ops/tests/support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use cfdcraft_ops::queue_file::QueueFile;
use repro_support::{CAVITY, new_document, scratch};
use support::{SSH_TARGET, new_root, ssh_log, workspace_root};

const GROUP: &str = "cli-queue-drain";

/// The built binary with the fakes first on its PATH.
fn cli(root: &Path, args: &[String]) -> Output {
    let path = std::env::join_paths(
        std::iter::once(root.join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .env("PATH", path)
        .env("CFC_TEST_ROOT", root)
        .env("CFC_TEST_MODE", "ok")
        .stdin(Stdio::null())
        .current_dir(workspace_root().join("target"))
        .output()
        .unwrap()
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("the binary ended by itself")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A fake box and a new document of the cavity with one waiting job.
fn world(tag: &str) -> (PathBuf, PathBuf) {
    let root = new_root(GROUP, tag);
    let doc = new_document(&scratch(&format!("{GROUP}-{tag}")), "cavity", CAVITY);
    let added = cli(&root, &words(&["queue", "add", &text(&doc)]));
    assert_eq!(code(&added), 0, "{}", err(&added));
    (root, doc)
}

/// `queue drain <doc>` with the key and the hosts file of the fake box, then `extra`.
fn drain_args(root: &Path, doc: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = words(&["queue", "drain"]);
    args.push(text(doc));
    args.extend(["--key".to_owned(), text(&root.join("dummy-key"))]);
    args.extend(["--hosts".to_owned(), text(&root.join("hosts.toml"))]);
    args.extend(["--socket-dir".to_owned(), text(&root.join("sockets"))]);
    args.extend(extra.iter().map(|word| (*word).to_owned()));
    args
}

/// Neither the key nor the ssh target is in what a verb printed.
fn assert_no_secrets(root: &Path, o: &Output) {
    let shown = format!("{}\n{}", out(o), err(o));
    for secret in [SSH_TARGET, "dummy-key", &text(&root.join("dummy-key"))] {
        assert!(!shown.contains(secret), "{secret} is in:\n{shown}");
    }
}

#[test]
fn drain_foreground_on_an_empty_queue_exits_zero_without_touching_the_host() {
    let root = new_root(GROUP, "foreground");
    let doc = new_document(&scratch("cli-queue-drain-fg"), "cavity", CAVITY);
    let o = cli(
        &root,
        &drain_args(&root, &doc, &["--foreground", "--idle-exit", "1"]),
    );
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("finished: there was nothing left to do"),
        "{}",
        out(&o)
    );
    assert_eq!(ssh_log(&root), Vec::<String>::new(), "the host was called");
    assert_no_secrets(&root, &o);
}

#[test]
fn drain_without_foreground_reports_started_or_already_running() {
    let root = new_root(GROUP, "background");
    let doc = new_document(&scratch("cli-queue-drain-bg"), "cavity", CAVITY);
    // The runner started here ends by itself after three idle looks, about fifteen seconds.
    let o = cli(&root, &drain_args(&root, &doc, &["--idle-exit", "3"]));
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("Started the queue runner; it keeps running if you close this program."),
        "{}",
        out(&o)
    );

    let file = QueueFile::new(&doc);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !file.drain_running() {
        assert!(Instant::now() < deadline, "the runner never took its lock");
        std::thread::sleep(Duration::from_millis(50));
    }
    let o = cli(&root, &drain_args(&root, &doc, &["--idle-exit", "3"]));
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).contains("The queue runner is already running."),
        "{}",
        out(&o)
    );
    assert_no_secrets(&root, &o);
}

#[test]
fn drain_refuses_a_missing_key_or_hosts_file_before_spawning() {
    let root = new_root(GROUP, "refuse");
    let doc = new_document(&scratch("cli-queue-drain-refuse"), "cavity", CAVITY);
    let missing = root.join("not-there");
    let good = drain_args(&root, &doc, &[]);
    let modes: [&[&str]; 2] = [&[], &["--foreground"]];
    for flag in ["--key", "--hosts"] {
        for extra in modes {
            let mut args = good.clone();
            args.extend(extra.iter().map(|word| (*word).to_owned()));
            let at = args.iter().position(|word| word == flag).unwrap() + 1;
            args[at] = text(&missing);
            let o = cli(&root, &args);
            assert_eq!(code(&o), 2, "{}", err(&o));
            assert!(
                err(&o).contains("the key file or the hosts file cannot be read"),
                "{}",
                err(&o)
            );
            assert!(!err(&o).contains("not-there"), "{}", err(&o));
            assert!(out(&o).is_empty(), "{}", out(&o));
        }
    }
    assert!(!QueueFile::new(&doc).drain_running());
    assert_eq!(ssh_log(&root), Vec::<String>::new(), "the host was called");
}

#[test]
fn the_key_is_never_printed() {
    let (root, doc) = world("secrets");
    let d = text(&doc);
    let runs = [
        words(&["queue", "reorder", &d, "q-001", "--to", "1"]),
        words(&["queue", "stop", &d, "q-001"]),
        words(&["queue", "cancel", &d, "q-009"]),
        words(&["queue", "cancel", &d, "q-001", "--json"]),
        drain_args(&root, &doc, &["--foreground", "--idle-exit", "1"]),
        drain_args(&root, &doc, &["--foreground", "--idle-exit", "1", "--json"]),
        drain_args(
            &root,
            &doc,
            &["--foreground", "--profile", "no-such-profile"],
        ),
    ];
    for args in &runs {
        let o = cli(&root, args);
        assert_no_secrets(&root, &o);
    }
    // A profile the hosts file does not have is refused in plain words.
    let o = cli(&root, &runs[6]);
    assert_eq!(code(&o), 2, "{}", err(&o));
    assert!(
        err(&o).contains("the compute host could not be set up"),
        "{}",
        err(&o)
    );
}

#[test]
fn mcp_accepts_attached_jobs() {
    let o = Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(["mcp", "--help"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("--attached-jobs"), "{}", out(&o));
}
