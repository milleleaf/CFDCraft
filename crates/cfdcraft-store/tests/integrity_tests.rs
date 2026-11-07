use std::cell::Cell;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use cfdcraft_store::{ContentHash, Store, StoreError};

fn fresh(name: &str) -> PathBuf {
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn store(dir: &std::path::Path) -> Store {
    Store::create(dir.join("s.cfdstore")).unwrap()
}

fn blob_count(store: &Store) -> usize {
    fs::read_dir(store.root().join("blobs")).unwrap().count()
}

fn big() -> Vec<u8> {
    (0..3 * 1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
        .collect()
}

#[test]
fn a_large_file_is_stored_by_its_hash() {
    let dir = fresh("big");
    let (s, data) = (store(&dir), big());
    let file = dir.join("field.bin");
    fs::write(&file, &data).unwrap();
    let hash = s.put_file(&file).unwrap();
    assert_eq!(hash.to_string(), blake3::hash(&data).to_hex().to_string());
    assert_eq!(fs::read(s.blob_path(&hash)).unwrap(), data);
    assert_eq!(blob_count(&s), 1);
}

#[test]
fn a_source_that_changes_is_refused() {
    let dir = fresh("changed");
    let s = store(&dir);
    let opens = Cell::new(0);
    let result = s.put_with(|| {
        opens.set(opens.get() + 1);
        Ok(Cursor::new(if opens.get() == 1 {
            b"first".to_vec()
        } else {
            b"second".to_vec()
        }))
    });
    assert!(matches!(result, Err(StoreError::ChangedWhileStoring)));
    assert!(!s.contains(&ContentHash::of(b"first")));
    assert!(!s.contains(&ContentHash::of(b"second")));
    assert_eq!(blob_count(&s), 0);
}

#[test]
fn a_wrong_expected_hash_stores_nothing() {
    let dir = fresh("verified");
    let s = store(&dir);
    let data = big();
    let right = ContentHash::of(&data);
    let wrong = ContentHash::of(b"something else");

    match s.put_reader_verified(wrong, Cursor::new(&data)) {
        Err(StoreError::HashMismatch { expected, actual }) => {
            assert_eq!((expected, actual), (wrong, right));
            let text = StoreError::HashMismatch { expected, actual }.to_string();
            assert!(text.contains(&right.to_string()) && text.contains(&wrong.to_string()));
        }
        other => panic!("expected HashMismatch, got {other:?}"),
    }
    assert!(matches!(
        s.put_verified(wrong, &data),
        Err(StoreError::HashMismatch { .. })
    ));
    assert_eq!(blob_count(&s), 0);

    s.put_reader_verified(right, Cursor::new(&data)).unwrap();
    assert_eq!(s.read(right).unwrap(), data);
    s.put_verified(right, &data).unwrap();
    assert_eq!(blob_count(&s), 1);
}

#[test]
fn damage_is_detected() {
    let dir = fresh("damage");
    let s = store(&dir);
    let hash = s.put_bytes(b"0123456789").unwrap();
    s.verify(&hash).unwrap();

    fs::write(s.blob_path(&hash), b"9876543210").unwrap();
    assert!(matches!(s.verify(&hash), Err(StoreError::Corrupt(_))));
    assert!(matches!(s.read(hash), Err(StoreError::Corrupt(_))));

    // Storing the same content again replaces a damaged blob instead of failing for ever.
    fs::write(s.blob_path(&hash), b"short").unwrap();
    assert_eq!(s.put_bytes(b"0123456789").unwrap(), hash);
    s.verify(&hash).unwrap();

    let absent = ContentHash::of(b"absent");
    assert!(matches!(s.verify(&absent), Err(StoreError::Missing(_))));
}

#[test]
fn storing_a_large_file_again_leaves_the_first_blob() {
    let dir = fresh("again");
    let s = store(&dir);
    let file = dir.join("field.bin");
    fs::write(&file, big()).unwrap();
    let hash = s.put_file(&file).unwrap();
    let before = fs::metadata(s.blob_path(&hash))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(s.put_file(&file).unwrap(), hash);
    assert_eq!(
        fs::metadata(s.blob_path(&hash))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert_eq!(blob_count(&s), 1);
}

const GOOD: &[u8] = b"0123456789";
const SAME_LENGTH_DAMAGE: &[u8] = b"9876543210";

fn damaged(s: &Store, hash: &ContentHash, bytes: &[u8]) {
    fs::write(s.blob_path(hash), bytes).unwrap();
    assert!(matches!(s.verify(hash), Err(StoreError::Corrupt(_))));
}

#[test]
fn a_same_length_damaged_blob_is_not_taken_for_a_stored_one() {
    let dir = fresh("same-length");
    let s = store(&dir);
    let hash = s.put_bytes(GOOD).unwrap();

    damaged(&s, &hash, SAME_LENGTH_DAMAGE);
    assert_eq!(s.put_bytes(GOOD).unwrap(), hash);
    s.verify(&hash).unwrap();
    assert_eq!(fs::read(s.blob_path(&hash)).unwrap(), GOOD);

    let file = dir.join("good.bin");
    fs::write(&file, GOOD).unwrap();
    damaged(&s, &hash, SAME_LENGTH_DAMAGE);
    assert_eq!(s.put_file(&file).unwrap(), hash);
    s.verify(&hash).unwrap();

    damaged(&s, &hash, SAME_LENGTH_DAMAGE);
    s.put_verified(hash, GOOD).unwrap();
    s.verify(&hash).unwrap();
    assert_eq!(blob_count(&s), 1);
}

#[test]
fn a_repair_from_the_wrong_content_leaves_the_damaged_blob_and_no_leftovers() {
    let dir = fresh("bad-repair");
    let s = store(&dir);
    let hash = s.put_bytes(GOOD).unwrap();
    damaged(&s, &hash, SAME_LENGTH_DAMAGE);

    assert!(matches!(
        s.put_verified(hash, b"not the content"),
        Err(StoreError::HashMismatch { .. })
    ));
    assert_eq!(fs::read(s.blob_path(&hash)).unwrap(), SAME_LENGTH_DAMAGE);
    assert_eq!(blob_count(&s), 1);

    // A source that changes while it is read does not replace the damaged blob either.
    let opens = Cell::new(0);
    let result = s.put_with(|| {
        opens.set(opens.get() + 1);
        Ok(Cursor::new(if opens.get() == 1 {
            GOOD.to_vec()
        } else {
            b"changed".to_vec()
        }))
    });
    assert!(matches!(result, Err(StoreError::ChangedWhileStoring)));
    assert_eq!(fs::read(s.blob_path(&hash)).unwrap(), SAME_LENGTH_DAMAGE);
    assert_eq!(blob_count(&s), 1);
}
