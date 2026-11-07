use std::path::PathBuf;

/// One file of a written case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseFile {
    /// The path from the case root.
    pub rel_path: PathBuf,
    pub bytes: Vec<u8>,
}

/// The files of a written case, in the order they are written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseTree {
    pub files: Vec<CaseFile>,
}
