use std::cell::Cell;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::ContentHash;

/// What can go wrong with a store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("'{0}' is not a content hash: it must be 64 lower-case hexadecimal characters")]
    BadHash(String),
    #[error("{} is not a result store: it has no blobs folder", .0.display())]
    NotAStore(PathBuf),
    #[error("the result {0} is not in the store")]
    Missing(ContentHash),
    #[error("the stored result {0} does not match its name; it is damaged")]
    Corrupt(ContentHash),
    #[error("the file changed while it was being stored, so nothing was stored")]
    ChangedWhileStoring,
    #[error("the content has hash {actual} but {expected} was expected, so nothing was stored")]
    HashMismatch {
        expected: ContentHash,
        actual: ContentHash,
    },
    #[error("cannot use the result store: {0}")]
    Io(#[from] io::Error),
}

/// The folder `<name>.cfdstore`, holding `blobs/<hash>`.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

const BUFFER: usize = 64 * 1024;

/// Copy `from` into `to`, returning the hash and length of what was copied.
fn copy_hashing(from: &mut impl Read, to: &mut impl Write) -> io::Result<(ContentHash, u64)> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; BUFFER];
    let mut len = 0u64;
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(&buf[..n]);
        to.write_all(&buf[..n])?;
        len += n as u64;
    }
    Ok((ContentHash::from_digest(hasher.finalize()), len))
}

fn hash_reader(mut from: impl Read) -> io::Result<(ContentHash, u64)> {
    copy_hashing(&mut from, &mut io::sink())
}

impl Store {
    /// Make the store folder and its `blobs` folder; safe to call again.
    pub fn create(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        std::fs::create_dir_all(root.join("blobs"))?;
        Ok(Self { root })
    }

    /// Open an existing store; the `blobs` folder must exist.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.join("blobs").is_dir() {
            return Err(StoreError::NotAStore(root));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The only place a blob path is built.
    pub fn blob_path(&self, hash: &ContentHash) -> PathBuf {
        self.root.join("blobs").join(hash.to_string())
    }

    pub fn contains(&self, hash: &ContentHash) -> bool {
        self.blob_path(hash).is_file()
    }

    /// Store `bytes` under their hash. Storing the same bytes again changes nothing.
    pub fn put_bytes(&self, bytes: &[u8]) -> Result<ContentHash, StoreError> {
        let hash = ContentHash::of(bytes);
        self.create_blob(hash, bytes.len() as u64, |f| f.write_all(bytes))?;
        Ok(hash)
    }

    /// Store what `open` yields. It is opened twice (hash, then copy); if the second
    /// reading differs from the first, nothing is stored.
    pub fn put_with<R: Read>(
        &self,
        open: impl Fn() -> io::Result<R>,
    ) -> Result<ContentHash, StoreError> {
        let (hash, len) = hash_reader(open()?)?;
        if self.contains(&hash) {
            return self.same_length(hash, len).map(|()| hash);
        }
        let changed = Cell::new(false);
        let placed = self.create_blob(hash, len, |f| {
            let (again, _) = copy_hashing(&mut open()?, f)?;
            if again != hash {
                changed.set(true);
                return Err(io::Error::other("source changed"));
            }
            Ok(())
        });
        match placed {
            Err(_) if changed.get() => Err(StoreError::ChangedWhileStoring),
            other => other.map(|()| hash),
        }
    }

    /// Store a file under the hash of its content.
    pub fn put_file(&self, path: impl AsRef<Path>) -> Result<ContentHash, StoreError> {
        let path = path.as_ref();
        self.put_with(|| File::open(path))
    }

    /// Stream `reader` into the blob `expected` in one pass. Nothing is stored when the
    /// hash of what was read is different.
    pub fn put_reader_verified(
        &self,
        expected: ContentHash,
        mut reader: impl Read,
    ) -> Result<(), StoreError> {
        if self.contains(&expected) {
            return self.verify(&expected);
        }
        let actual = Cell::new(None);
        let placed = write_new(&self.blob_path(&expected), |f| {
            let (got, _) = copy_hashing(&mut reader, f)?;
            if got != expected {
                actual.set(Some(got));
                return Err(io::Error::other("hash mismatch"));
            }
            Ok(())
        });
        match (placed, actual.get()) {
            (Err(_), Some(actual)) => Err(StoreError::HashMismatch { expected, actual }),
            (Err(e), None) if e.kind() == io::ErrorKind::AlreadyExists => self.verify(&expected),
            (Err(e), None) => Err(e.into()),
            (Ok(()), _) => Ok(()),
        }
    }

    /// [`Store::put_reader_verified`] over a byte slice.
    pub fn put_verified(&self, expected: ContentHash, bytes: &[u8]) -> Result<(), StoreError> {
        self.put_reader_verified(expected, bytes)
    }

    /// Open a blob for streaming. This does not check the content; use [`Store::verify`].
    pub fn reader(&self, hash: ContentHash) -> Result<File, StoreError> {
        File::open(self.blob_path(&hash)).map_err(|e| missing_or_io(e, hash))
    }

    /// Re-hash a blob and say whether it is whole.
    pub fn verify(&self, hash: &ContentHash) -> Result<(), StoreError> {
        let (actual, _) = hash_reader(self.reader(*hash)?)?;
        if actual == *hash {
            Ok(())
        } else {
            Err(StoreError::Corrupt(*hash))
        }
    }

    /// Read a blob, checking it against its name.
    pub fn read(&self, hash: ContentHash) -> Result<Vec<u8>, StoreError> {
        let mut bytes = Vec::new();
        self.reader(hash)?.read_to_end(&mut bytes)?;
        if ContentHash::of(&bytes) == hash {
            Ok(bytes)
        } else {
            Err(StoreError::Corrupt(hash))
        }
    }

    /// Create the blob; an existing one is success when it has the same length.
    fn create_blob(
        &self,
        hash: ContentHash,
        len: u64,
        fill: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> Result<(), StoreError> {
        match write_new(&self.blob_path(&hash), fill) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => self.same_length(hash, len),
            Err(e) => Err(e.into()),
        }
    }

    fn same_length(&self, hash: ContentHash, len: u64) -> Result<(), StoreError> {
        let have = std::fs::metadata(self.blob_path(&hash))?.len();
        if have == len {
            Ok(())
        } else {
            Err(StoreError::Corrupt(hash))
        }
    }
}

fn write_new(path: &Path, fill: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    craft_atomic::write_new_with(path, fill)
}

fn missing_or_io(e: io::Error, hash: ContentHash) -> StoreError {
    if e.kind() == io::ErrorKind::NotFound {
        StoreError::Missing(hash)
    } else {
        e.into()
    }
}
