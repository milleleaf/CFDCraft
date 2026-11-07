//! A save that follows a read is refused when someone else saved the file in between, so a run
//! recorded by one process is not lost to a document another process read earlier.

use std::fs;
use std::path::PathBuf;

use cfdcraft_doc::{DocError, Document, open, open_stamped, save, save_if_unchanged};
use cfdcraft_model::Study;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("doc-cas-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("cavity.cfdcraft")
}

fn cavity() -> Study {
    Study::from_json(CAVITY).unwrap()
}

#[test]
fn a_file_nobody_touched_is_saved() {
    let path = fresh("untouched");
    save(&Document::new(cavity()), &path).unwrap();

    let (opened, stamp) = open_stamped(&path).unwrap();
    let mut document = opened.document;
    document.study.mesh.cells_x = 256;
    save_if_unchanged(&document, &path, stamp).unwrap();
    assert_eq!(open(&path).unwrap().document.study.mesh.cells_x, 256);

    // The same bytes can be saved over again with a fresh stamp.
    let (opened, stamp) = open_stamped(&path).unwrap();
    save_if_unchanged(&opened.document, &path, stamp).unwrap();
}

#[test]
fn a_save_made_since_the_read_is_not_overwritten() {
    let path = fresh("changed");
    save(&Document::new(cavity()), &path).unwrap();

    let (opened, stamp) = open_stamped(&path).unwrap();
    // Another process saves the file while this one is busy.
    let mut other = Document::new(cavity());
    other.study.mesh.cells_x = 64;
    save(&other, &path).unwrap();
    let theirs = fs::read(&path).unwrap();

    let mut mine = opened.document;
    mine.study.mesh.cells_x = 256;
    match save_if_unchanged(&mine, &path, stamp) {
        Err(DocError::Changed(changed)) => assert_eq!(changed, path),
        other => panic!("expected the save to be refused, found {other:?}"),
    }
    assert_eq!(fs::read(&path).unwrap(), theirs);
}
