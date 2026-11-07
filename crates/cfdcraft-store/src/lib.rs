//! A content-addressed store for heavy results kept beside a study document.
//!
//! Layout: a folder named `<name>.cfdstore` holds a `blobs` folder, and every blob is a
//! file called by the BLAKE3 hash of its content, written as 64 lower-case hexadecimal
//! characters. Storing the same content twice is one file.
//!
//! Blobs are created with the shared crash-safe exclusive-create call, so a crash leaves
//! either no blob or a whole one, and an existing blob is never replaced.
//!
//! A file that changes while it is being stored is refused instead of being stored under
//! the wrong name. [`Store::verify`] is the check to run before trusting a blob that
//! travelled (a pack, a copy).
//!
//! There is deliberately no function that removes a blob. Pruning arrives later as an
//! explicit feature.

mod hash;
mod store;

pub use hash::ContentHash;
pub use store::{Store, StoreError};
