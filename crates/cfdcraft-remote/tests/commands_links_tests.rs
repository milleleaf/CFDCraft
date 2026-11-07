//! The commands that read and write a file of a run are run for real with `sh`, against a folder
//! that holds links the solver could have made. Nothing outside the run folder is read or changed.
#![cfg(unix)]

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cfdcraft_remote::commands::{
    parse_du_lines, read_text_command, tail_command, time_dirs_command, write_text_command,
};

const ID: &str = "cfdcraft-demo-1-2";

struct Folder {
    work: String,
    run: PathBuf,
    outside: PathBuf,
}

/// A work folder with one run folder, a plain log, and a folder next to it that the run must not
/// reach.
fn make(tag: &str) -> Folder {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("links-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let run = root.join("work").join(ID);
    fs::create_dir_all(run.join("system")).unwrap();
    fs::create_dir_all(root.join("elsewhere")).unwrap();
    fs::write(run.join("log"), "hello\n").unwrap();
    fs::write(run.join("system/controlDict"), "old\n").unwrap();
    fs::write(root.join("elsewhere/secret"), "SECRET\n").unwrap();
    symlink(root.join("elsewhere/secret"), run.join("leak")).unwrap();
    symlink(root.join("elsewhere"), run.join("dirlink")).unwrap();
    symlink(
        root.join("elsewhere/secret"),
        run.join("system/controlDict.tmp"),
    )
    .unwrap();
    Folder {
        work: root.join("work").to_string_lossy().into_owned(),
        run,
        outside: root.join("elsewhere"),
    }
}

/// Runs a command string with `sh`, with `input` on its standard input.
fn sh(command: &str, input: &str) -> (bool, String) {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // A command that refuses may exit before it reads its input.
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    let out = child.wait_with_output().unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn a_plain_file_is_read_and_a_link_out_of_the_run_folder_gives_nothing() {
    let b = make("read");
    let tail =
        |rel: &str, offset: u64| sh(&tail_command(&b.work, ID, rel, offset, 1000).unwrap(), "");
    assert_eq!(tail("log", 0), (true, "hello\n".to_owned()));
    assert_eq!(tail("log", 3), (true, "lo\n".to_owned()));
    assert_eq!(tail("missing", 0), (true, String::new()));
    assert_eq!(tail("leak", 0), (true, String::new()));
    assert_eq!(tail("dirlink/secret", 0), (true, String::new()));

    let read = |rel: &str| sh(&read_text_command(&b.work, ID, rel).unwrap(), "");
    assert_eq!(read("log"), (true, "hello\n".to_owned()));
    assert!(!read("missing").0);
    let (ok, text) = read("leak");
    assert!(!ok && text.is_empty(), "{ok} {text:?}");
    let (ok, text) = read("dirlink/secret");
    assert!(!ok && text.is_empty(), "{ok} {text:?}");
}

#[test]
fn a_write_replaces_a_plain_file_and_never_goes_through_a_link() {
    let b = make("write");
    let write = |rel: &str, text: &str| sh(&write_text_command(&b.work, ID, rel).unwrap(), text);
    let secret = b.outside.join("secret");

    // A link at the final name, or in a folder on the way, is refused; the target is untouched.
    assert!(!write("leak", "overwritten\n").0);
    assert!(!write("dirlink/secret", "overwritten\n").0);
    assert_eq!(fs::read_to_string(&secret).unwrap(), "SECRET\n");

    // A link left at the temporary name is replaced, never written through.
    assert!(write("system/controlDict", "new\n").0);
    assert_eq!(
        fs::read_to_string(b.run.join("system/controlDict")).unwrap(),
        "new\n"
    );
    assert_eq!(fs::read_to_string(&secret).unwrap(), "SECRET\n");
    assert!(
        fs::symlink_metadata(b.run.join("system/controlDict.tmp")).is_err(),
        "the temporary name is gone after the rename"
    );
}

#[test]
fn the_times_of_the_processor_folders_are_added_up_per_name() {
    let b = make("times");
    fs::create_dir_all(b.run.join("0.5")).unwrap();
    fs::write(b.run.join("0.5/U"), vec![b'x'; 10_000]).unwrap();
    for n in 0..4 {
        let dir = b.run.join(format!("processor{n}/100"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("U"), vec![b'x'; 100_000]).unwrap();
    }
    let (ok, out) = sh(&time_dirs_command(&b.work, ID).unwrap(), "");
    assert!(ok);
    let dirs = parse_du_lines(&out);
    let top = dirs
        .iter()
        .find(|d| d.name == "0.5")
        .expect("a folder at the top");
    assert!(top.kb < 100, "{dirs:?}");
    let all = dirs
        .iter()
        .find(|d| d.name == "100")
        .expect("the processor times");
    assert!(all.kb >= 400, "four processors of 100 kB each: {dirs:?}");
}
