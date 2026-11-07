use std::path::{Path, PathBuf};

use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_capture::{
    CaptureError, entry_names, mesh_bundle, mesh_bundle_limited, read_entry, store_bundle,
};
use cfdcraft_store::Store;
use zip::{CompressionMethod, DateTime, ZipArchive};

fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("capture-bundle-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const SPEC: FixtureSpec = FixtureSpec {
    end_time: 5000,
    cells: 64,
    samples: 5,
    shift: 0.0,
};

fn results_in(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    write_results(&dir, &SPEC).unwrap();
    dir
}

fn blob_count(store: &Store) -> usize {
    std::fs::read_dir(store.root().join("blobs"))
        .unwrap()
        .count()
}

#[test]
fn the_mesh_of_a_results_folder_is_stored_and_read_back() {
    let root = scratch("stored");
    let results = results_in(&root, "results");
    let store = Store::create(root.join("study.cfdstore")).unwrap();

    let bundle = mesh_bundle(&results).unwrap();
    let (hash, size) = store_bundle(&store, &bundle).unwrap();
    let bytes = store.read(hash).unwrap();

    assert_eq!(bytes.len() as u64, size);
    assert_eq!(
        entry_names(&bytes).unwrap(),
        [
            "constant/polyMesh/boundary",
            "constant/polyMesh/faces",
            "constant/polyMesh/neighbour",
            "constant/polyMesh/owner",
            "constant/polyMesh/points",
        ]
    );
    assert_eq!(
        read_entry(&bytes, "constant/polyMesh/points").unwrap(),
        std::fs::read(results.join("constant/polyMesh/points")).unwrap()
    );
}

#[test]
fn the_same_files_give_the_same_bundle_bytes() {
    let root = scratch("same");
    let a = mesh_bundle(&results_in(&root, "a")).unwrap();
    let b = mesh_bundle(&results_in(&root.join("elsewhere"), "b")).unwrap();
    assert_eq!(a.to_bytes().unwrap(), b.to_bytes().unwrap());
    assert_eq!(a.hash().unwrap(), b.hash().unwrap());
}

#[test]
fn a_changed_file_changes_the_bundle_hash() {
    let root = scratch("changed");
    let a = results_in(&root, "a");
    let b = results_in(&root, "b");
    let before = mesh_bundle(&a).unwrap().hash().unwrap();
    assert_eq!(before, mesh_bundle(&b).unwrap().hash().unwrap());

    let owner = b.join("constant/polyMesh/owner");
    let mut text = std::fs::read(&owner).unwrap();
    text.push(b'x');
    std::fs::write(owner, text).unwrap();

    assert_ne!(before, mesh_bundle(&b).unwrap().hash().unwrap());
}

#[test]
fn storing_the_same_bundle_twice_is_one_blob() {
    let root = scratch("twice");
    let results = results_in(&root, "results");
    let store = Store::create(root.join("study.cfdstore")).unwrap();
    let bundle = mesh_bundle(&results).unwrap();

    let first = store_bundle(&store, &bundle).unwrap();
    let second = store_bundle(&store, &bundle).unwrap();

    assert_eq!(first, second);
    assert_eq!(blob_count(&store), 1);
}

#[test]
fn a_missing_mesh_file_is_refused_by_name() {
    let root = scratch("missing");
    let results = results_in(&root, "results");
    let store = Store::create(root.join("study.cfdstore")).unwrap();
    std::fs::remove_file(results.join("constant/polyMesh/owner")).unwrap();

    let error = mesh_bundle(&results).unwrap_err();

    assert!(matches!(error, CaptureError::Missing(_)), "{error:?}");
    assert!(error.to_string().contains("owner"), "{error}");
    assert_eq!(blob_count(&store), 0);
}

#[cfg(unix)]
#[test]
fn a_link_instead_of_a_file_is_refused() {
    use std::os::unix::fs::symlink;

    let root = scratch("link");
    let results = results_in(&root, "results");
    let points = results.join("constant/polyMesh/points");
    std::fs::remove_file(&points).unwrap();
    symlink(results.join("log.blockMesh"), points).unwrap();
    let error = mesh_bundle(&results).unwrap_err();
    assert!(matches!(error, CaptureError::NotAFile(_)), "{error:?}");

    let other = results_in(&root, "other");
    let poly = other.join("constant/polyMesh");
    std::fs::remove_dir_all(&poly).unwrap();
    symlink(results.join("constant"), poly).unwrap();
    let error = mesh_bundle(&other).unwrap_err();
    assert!(matches!(error, CaptureError::NotAFile(_)), "{error:?}");
}

#[test]
fn an_oversized_file_is_refused() {
    let root = scratch("big");
    let results = results_in(&root, "results");
    let error = mesh_bundle_limited(&results, 8).unwrap_err();
    assert!(matches!(error, CaptureError::TooLarge(_)), "{error:?}");
    assert!(error.to_string().contains("constant/polyMesh/"), "{error}");
}

#[test]
fn a_bundle_is_a_plain_stored_zip_with_a_fixed_time() {
    let root = scratch("plain");
    let bytes = mesh_bundle(&results_in(&root, "results"))
        .unwrap()
        .to_bytes()
        .unwrap();
    let mut zip = ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(zip.len(), 5);
    for index in 0..zip.len() {
        let file = zip.by_index(index).unwrap();
        assert_eq!(file.compression(), CompressionMethod::Stored);
        assert_eq!(file.last_modified(), Some(DateTime::default()));
    }
}
