use std::fs;
use std::path::PathBuf;

use cfdcraft_store::{ContentHash, Store, StoreError};

const CAVITY: &[u8] = include_bytes!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn fresh(name: &str) -> PathBuf {
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn blob_names(store: &Store) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(store.root().join("blobs"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn cavity_study_is_stored_by_its_hash_and_read_back() {
    let store = Store::create(fresh("cavity").join("cavity.cfdstore")).unwrap();
    let hash = store.put_bytes(CAVITY).unwrap();
    assert_eq!(hash.to_string(), blake3::hash(CAVITY).to_hex().to_string());
    assert!(store.root().join("blobs").join(hash.to_string()).is_file());
    assert_eq!(store.read(hash).unwrap(), CAVITY);
}

#[test]
fn storing_twice_changes_nothing() {
    let store = Store::create(fresh("twice").join("s.cfdstore")).unwrap();
    let first = store.put_bytes(CAVITY).unwrap();
    let path = store.blob_path(&first);
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    let second = store.put_bytes(CAVITY).unwrap();
    assert_eq!(first, second);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    assert_eq!(blob_names(&store), vec![first.to_string()]);
}

#[test]
fn hash_text_is_strictly_validated() {
    let good = "a".repeat(64);
    let parsed = ContentHash::parse(&good).unwrap();
    assert_eq!(parsed.to_string(), good);
    let real = ContentHash::of(b"x").to_string();
    assert_eq!(ContentHash::parse(&real).unwrap().to_string(), real);

    for bad in [
        "A".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        String::new(),
        format!("{good}\n"),
        "../etc/passwd".to_owned(),
        format!("{}/{}", "a".repeat(31), "a".repeat(32)),
        format!("{}\\{}", "a".repeat(31), "a".repeat(32)),
        format!("{}..", "a".repeat(62)),
        "é".repeat(32),
    ] {
        assert!(
            matches!(ContentHash::parse(&bad), Err(StoreError::BadHash(_))),
            "accepted {bad:?}"
        );
    }

    let json = serde_json::to_string(&parsed).unwrap();
    assert_eq!(json, format!("\"{good}\""));
    assert_eq!(serde_json::from_str::<ContentHash>(&json).unwrap(), parsed);
    assert!(serde_json::from_str::<ContentHash>("\"../x\"").is_err());
    assert!(serde_json::from_str::<ContentHash>("5").is_err());

    let schema = serde_json::to_value(schemars::schema_for!(ContentHash)).unwrap();
    assert_eq!(schema["pattern"], "^[0-9a-f]{64}$");
}

#[test]
fn open_needs_blobs_and_create_repeats() {
    let dir = fresh("open");
    fs::create_dir_all(&dir).unwrap();
    assert!(matches!(Store::open(&dir), Err(StoreError::NotAStore(_))));
    let root = dir.join("deep").join("s.cfdstore");
    Store::create(&root).unwrap();
    Store::create(&root).unwrap();
    assert!(root.join("blobs").is_dir());
    Store::open(&root).unwrap();
}

#[test]
fn contains_and_reader_report_missing() {
    let store = Store::create(fresh("contains").join("s.cfdstore")).unwrap();
    let hash = store.put_bytes(b"result").unwrap();
    assert!(store.contains(&hash));
    let other = ContentHash::of(b"never stored");
    assert!(!store.contains(&other));
    let mut text = String::new();
    std::io::Read::read_to_string(&mut store.reader(hash).unwrap(), &mut text).unwrap();
    assert_eq!(text, "result");
    match store.reader(other) {
        Err(e @ StoreError::Missing(_)) => assert!(e.to_string().contains(&other.to_string())),
        _ => panic!("expected Missing"),
    }
}
