//! A crashed or failed save must leave the previous file whole.
//!
//! The kill tests start this same test binary as a child process, which saves a changed document
//! through a file system that stops at one chosen step; the parent waits until the child is
//! stopped there, kills it, and looks at the folder.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use cfdcraft_doc::{
    DocError, Document, open, save, save_with, store_dir, sweep_stale_staging, to_bytes,
};
use cfdcraft_model::Study;
use craft_atomic::{AtomicFs, NewFilePermissions, RealFs};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const DOC_NAME: &str = "cavity.cfdcraft";
const MARKER: &str = "stalled.marker";
const CHILD_TEST: &str = "child_save";
const DIR_VAR: &str = "CFDCRAFT_DOC_CHILD_DIR";
const STALL_VAR: &str = "CFDCRAFT_DOC_CHILD_STALL";

fn doc_a() -> Document {
    Document::new(Study::from_json(CAVITY).unwrap())
}

/// A document that differs from [`doc_a`].
fn doc_b() -> Document {
    let mut doc = doc_a();
    doc.study.mesh.cells_x = 64;
    doc
}

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("crash-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn staging_files(dir: &Path) -> Vec<String> {
    names(dir)
        .into_iter()
        .filter(|n| n.starts_with(".cavity.cfdcraft.") && n.ends_with(".craft_tmp"))
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Stall {
    Sync,
    Rename,
}

/// The real file system, except that one step announces itself with a marker file and then never
/// returns.
struct StallFs {
    at: Stall,
    marker: PathBuf,
}

impl StallFs {
    fn hold(&self) -> ! {
        fs::write(&self.marker, b"stalled").unwrap();
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }
}

impl AtomicFs for StallFs {
    fn create_new(&self, path: &Path, permissions: NewFilePermissions) -> io::Result<File> {
        RealFs.create_new(path, permissions)
    }
    fn sync(&self, file: &File) -> io::Result<()> {
        if self.at == Stall::Sync {
            self.hold();
        }
        RealFs.sync(file)
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if self.at == Stall::Rename {
            self.hold();
        }
        RealFs.rename(from, to)
    }
    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
        RealFs.hard_link(from, to)
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        RealFs.remove(path)
    }
    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        RealFs.sync_dir(dir)
    }
    fn set_permissions(&self, file: &File, perms: fs::Permissions) -> io::Result<()> {
        RealFs.set_permissions(file, perms)
    }
}

/// The child half of the kill tests. It does nothing unless the parent set its variables.
#[test]
#[ignore = "started as a child process by the kill tests"]
fn child_save() {
    let (Ok(dir), Ok(stall)) = (std::env::var(DIR_VAR), std::env::var(STALL_VAR)) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let stalling = StallFs {
        at: if stall == "sync" {
            Stall::Sync
        } else {
            Stall::Rename
        },
        marker: dir.join(MARKER),
    };
    save_with(&doc_b(), &dir.join(DOC_NAME), &stalling).unwrap();
}

/// Start the child, wait until it is stopped at `stall`, and kill it.
fn run_and_kill_child(dir: &Path, stall: &str) {
    let marker = dir.join(MARKER);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
        .env(DIR_VAR, dir)
        .env(STALL_VAR, stall)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while !marker.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the child ended ({status}) before it reached the stall point");
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the child did not reach the stall point within 60 seconds");
        }
        thread::sleep(Duration::from_millis(20));
    }
    child.kill().unwrap();
    child.wait().unwrap();
}

fn previous_file_survives_a_kill(tag: &str, stall: &str) {
    let dir = fresh(tag);
    let path = dir.join(DOC_NAME);
    let a = doc_a();
    save(&a, &path).unwrap();
    let before = fs::read(&path).unwrap();
    assert_eq!(before, to_bytes(&a).unwrap());

    run_and_kill_child(&dir, stall);

    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(open(&path).unwrap().document, a);
    assert_eq!(staging_files(&dir).len(), 1, "{:?}", names(&dir));

    // The orphan the kill left is what the sweeper removes.
    let removed = sweep_stale_staging(&path, Duration::ZERO).unwrap();
    assert_eq!(removed.len(), 1);
    assert!(staging_files(&dir).is_empty(), "{:?}", names(&dir));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn a_kill_after_staging_and_before_the_commit_leaves_the_previous_file() {
    previous_file_survives_a_kill("rename", "rename");
}

#[test]
fn a_kill_while_the_staged_content_is_flushed_leaves_the_previous_file() {
    previous_file_survives_a_kill("sync", "sync");
}

#[test]
fn a_kill_before_the_first_commit_leaves_no_file() {
    let dir = fresh("first");
    let path = dir.join(DOC_NAME);
    run_and_kill_child(&dir, "rename");
    assert!(!path.exists());
    match open(&path).unwrap_err() {
        DocError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::NotFound),
        other => panic!("{other}"),
    }
    assert_eq!(staging_files(&dir).len(), 1, "{:?}", names(&dir));
}

/// The real file system, except that chosen steps fail.
struct FailFs {
    sync: Option<io::ErrorKind>,
    rename: Option<io::ErrorKind>,
}

impl AtomicFs for FailFs {
    fn create_new(&self, path: &Path, permissions: NewFilePermissions) -> io::Result<File> {
        RealFs.create_new(path, permissions)
    }
    fn sync(&self, file: &File) -> io::Result<()> {
        match self.sync {
            Some(kind) => Err(io::Error::new(kind, "injected failure")),
            None => RealFs.sync(file),
        }
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        match self.rename {
            Some(kind) => Err(io::Error::new(kind, "injected failure")),
            None => RealFs.rename(from, to),
        }
    }
    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
        RealFs.hard_link(from, to)
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        RealFs.remove(path)
    }
    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        RealFs.sync_dir(dir)
    }
    fn set_permissions(&self, file: &File, perms: fs::Permissions) -> io::Result<()> {
        RealFs.set_permissions(file, perms)
    }
}

#[test]
fn a_failed_save_leaves_the_previous_file_and_no_staging_file() {
    let failures = [
        FailFs {
            sync: Some(io::ErrorKind::StorageFull),
            rename: None,
        },
        FailFs {
            sync: None,
            rename: Some(io::ErrorKind::Other),
        },
    ];
    for (n, failure) in failures.iter().enumerate() {
        let dir = fresh(&format!("fail{n}"));
        let path = dir.join(DOC_NAME);
        save(&doc_a(), &path).unwrap();
        let before = fs::read(&path).unwrap();

        let error = save_with(&doc_b(), &path, failure).unwrap_err();

        assert!(matches!(error, DocError::Io(_)), "{error}");
        assert!(
            error
                .to_string()
                .contains("the original file was not changed"),
            "{error}"
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(names(&dir), [DOC_NAME]);
    }
}

fn write_staging(dir: &Path, document_name: &str) -> PathBuf {
    let path = dir.join(craft_atomic::temp_name(document_name));
    fs::write(&path, b"left behind").unwrap();
    path
}

fn age(path: &Path, by: Duration) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - by).unwrap();
}

#[test]
fn the_sweeper_removes_only_old_staging_files_of_this_document() {
    let dir = fresh("sweep");
    let path = dir.join(DOC_NAME);
    save(&doc_a(), &path).unwrap();

    let orphan = write_staging(&dir, DOC_NAME);
    let other_doc = write_staging(&dir, "other.cfdcraft");
    let lookalike = dir.join(".cavity.cfdcraft.bak.12-3.craft_tmp");
    fs::write(&lookalike, b"not a staging file").unwrap();
    let neighbour = dir.join("notes.txt");
    fs::write(&neighbour, b"notes").unwrap();

    // A fresh staging file may belong to a save in another process.
    assert!(
        sweep_stale_staging(&path, Duration::from_secs(3600))
            .unwrap()
            .is_empty()
    );
    assert!(orphan.exists());

    assert_eq!(
        sweep_stale_staging(&path, Duration::ZERO).unwrap(),
        std::slice::from_ref(&orphan)
    );
    assert!(!orphan.exists());
    for kept in [&path, &other_doc, &lookalike, &neighbour] {
        assert!(kept.exists(), "{}", kept.display());
    }
}

#[test]
fn a_save_removes_old_orphans_and_leaves_young_ones() {
    let dir = fresh("save-sweeps");
    let path = dir.join(DOC_NAME);
    save(&doc_a(), &path).unwrap();

    let young = write_staging(&dir, DOC_NAME);
    let old = write_staging(&dir, DOC_NAME);
    age(&old, Duration::from_secs(2 * 3600));

    save(&doc_b(), &path).unwrap();

    assert!(young.exists());
    assert!(!old.exists());
    assert_eq!(open(&path).unwrap().document, doc_b());
}

#[test]
fn the_store_folder_sits_beside_the_file_named_after_its_stem() {
    let folder = Path::new("folder");
    assert_eq!(
        store_dir(&folder.join("cavity.cfdcraft")),
        folder.join("cavity.cfdstore")
    );
    assert_eq!(
        store_dir(&folder.join("cavity")),
        folder.join("cavity.cfdstore")
    );
    assert_eq!(
        store_dir(&folder.join("notes.txt")),
        folder.join("notes.cfdstore")
    );
}
