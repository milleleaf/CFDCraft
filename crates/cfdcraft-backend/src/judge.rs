//! What a finished run is judged on, in terms that name no solver. A backend reads the logs and
//! result files of a run through [`RunFiles`] and says whether the run finished and why not. The
//! exit status of the solver process never takes part: only the evidence in the files does.

use std::{
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
};

/// The most a log is read from: a larger file gives its last bytes, so a log is still judged by
/// its end.
pub const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

/// What a run is judged for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JudgeMode {
    /// A run that must have converged to the limits of the study.
    Run,
    /// A run that only has to have reached its end; the verdict word is `smoke_passed`.
    Smoke,
    /// A run that reached its end, judged without a residual limit; the verdict word is `passed`.
    Finished,
    /// A run stopped on request at this time.
    Stopped { time: String },
}

/// The verdict of a judge: `passed`, `failed` or `smoke_passed`, and the reasons it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeOutcome {
    pub verdict: String,
    pub reasons: Vec<String>,
}

impl JudgeOutcome {
    /// A failed verdict with one reason.
    pub fn failed(reason: impl Into<String>) -> Self {
        Self {
            verdict: "failed".to_owned(),
            reasons: vec![reason.into()],
        }
    }
}

/// Whether a run converged, as one backend sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvergenceOutcome {
    pub converged: bool,
    pub reasons: Vec<String>,
}

/// The files a run left, read by a name relative to the case root.
///
/// An implementation refuses an absolute name and any name with a `..` part: `read_text` and `size`
/// return `None` and `list` returns nothing for it.
pub trait RunFiles {
    /// The text of a file, or `None` when it is missing, not text or refused. A file over
    /// [`MAX_READ_BYTES`] gives its last bytes.
    fn read_text(&self, rel: &str) -> Option<String>;
    /// The names directly below `rel_dir`, sorted. Nothing for a missing folder.
    fn list(&self, rel_dir: &str) -> Vec<String>;
    /// The size of a file; `None` for a missing file, a folder or a refused name.
    fn size(&self, rel: &str) -> Option<u64>;
}

/// The files of a run as a folder on disk. It never follows a link, and refuses an absolute name
/// and a `..` part, so a folder that came from a compute host cannot lead a judge outside itself.
#[derive(Debug, Clone)]
pub struct DirRunFiles {
    root: PathBuf,
    cap: u64,
}

impl DirRunFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_cap(root, MAX_READ_BYTES)
    }

    /// The same with another read limit; a test uses a small one.
    pub fn with_cap(root: impl Into<PathBuf>, cap: u64) -> Self {
        Self {
            root: root.into(),
            cap,
        }
    }

    /// The path below the root, or `None` when `rel` is not a plain relative path or any part of
    /// it is a link or missing. `""` is the root itself.
    fn resolve(&self, rel: &str) -> Option<PathBuf> {
        let mut path = self.root.clone();
        for part in Path::new(rel).components() {
            match part {
                Component::Normal(name) => path.push(name),
                Component::CurDir => continue,
                _ => return None,
            }
            let meta = std::fs::symlink_metadata(&path).ok()?;
            if meta.file_type().is_symlink() {
                return None;
            }
        }
        Some(path)
    }
}

impl RunFiles for DirRunFiles {
    fn read_text(&self, rel: &str) -> Option<String> {
        let path = self.resolve(rel)?;
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let len = meta.len();
        let cut = len > self.cap;
        let mut file = std::fs::File::open(path).ok()?;
        if cut {
            file.seek(SeekFrom::Start(len - self.cap)).ok()?;
        }
        let mut bytes = Vec::new();
        file.take(self.cap).read_to_end(&mut bytes).ok()?;
        if cut {
            // The cut may fall inside a character: start at the next whole one.
            let skip = bytes.iter().take_while(|b| (**b & 0xC0) == 0x80).count();
            bytes.drain(..skip);
        }
        String::from_utf8(bytes).ok()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        let Some(path) = self.resolve(rel_dir) else {
            return Vec::new();
        };
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            return Vec::new();
        }
        // A directory entry's type is not followed, so a link is neither a file nor a folder here.
        let mut names: Vec<String> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file() || t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn size(&self, rel: &str) -> Option<u64> {
        let path = self.resolve(rel)?;
        let meta = std::fs::symlink_metadata(path).ok()?;
        meta.is_file().then_some(meta.len())
    }
}
