//! Removing one stored file: it says whether it removed anything, and it never touches a folder
//! or what a link points at.

use std::fs;
use std::path::PathBuf;

use cfdcraft_store::{Store, StoreError};

fn fresh(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("remove-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn remove_deletes_one_blob_and_says_whether_it_did() {
    let store = Store::create(fresh("one").join("s.cfdstore")).unwrap();
    let kept = store.put_bytes(b"kept").unwrap();
    let gone = store.put_bytes(b"gone").unwrap();

    assert!(store.remove(&gone).unwrap());
    assert!(!store.contains(&gone));
    assert!(store.contains(&kept));
    store.verify(&kept).unwrap();

    assert!(!store.remove(&gone).unwrap());
    assert!(store.contains(&kept));
}

#[test]
fn remove_never_follows_a_link_or_removes_a_folder() {
    let root = fresh("folder");
    let store = Store::create(root.join("s.cfdstore")).unwrap();

    // A folder with a file in it at the name of a blob.
    let hash = store.put_bytes(b"folder").unwrap();
    let path = store.blob_path(&hash);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("inside"), b"x").unwrap();

    assert!(matches!(store.remove(&hash), Err(StoreError::Io(_))));
    assert!(path.is_dir());
    assert!(path.join("inside").is_file());
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_a_blob_is_removed_and_its_target_stays() {
    let root = fresh("link");
    let store = Store::create(root.join("s.cfdstore")).unwrap();
    let hash = store.put_bytes(b"link").unwrap();
    let path = store.blob_path(&hash);

    let target = root.join("elsewhere");
    fs::write(&target, b"outside the store").unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();

    assert!(store.remove(&hash).unwrap());
    assert!(fs::symlink_metadata(&path).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"outside the store");
}
