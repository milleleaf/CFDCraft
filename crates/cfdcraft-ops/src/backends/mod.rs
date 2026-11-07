//! The registry of solver backends. A row is a backend and its admission state; everything that
//! runs a study asks the registry for a row by id and never matches on a dialect. One module per
//! solver family lists that family's rows here.

pub mod codesaturne;
pub mod esi;
pub mod identity;
pub mod lbm;
pub mod of14;
pub mod su2;

use cfdcraft_backend::Backend;
use cfdcraft_model::Diagnostic;
use cfdcraft_text::{Line, Text};
use std::fmt;

/// How far a backend has been proven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// It has not passed all three gate levels; its results carry a warning.
    Candidate,
    /// It has passed all three levels and may be chosen.
    Supported,
    /// The backend used when none is named.
    Default,
}

impl Admission {
    /// The stable name of the state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Supported => "supported",
            Self::Default => "default",
        }
    }
}

/// One backend of the registry and where it stands.
#[derive(Clone, Copy)]
pub struct Row {
    pub backend: &'static dyn Backend,
    pub admission: Admission,
}

impl fmt::Debug for Row {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Row")
            .field("id", &self.backend.id())
            .field("admission", &self.admission)
            .finish()
    }
}

/// The admission state of every backend that has one. A state changes only by a reviewed edit of
/// this table.
const ADMISSION: [(&str, Admission); 6] = [
    ("esi-v2512", Admission::Default),
    ("esi-v2606", Admission::Candidate),
    ("of14-20261008", Admission::Candidate),
    ("su2-8.6.0", Admission::Candidate),
    ("codesaturne-9.2.0", Admission::Candidate),
    ("fieldcraft-lbm-0.7.5", Admission::Candidate),
];

/// The backends whose writer matches its golden files byte for byte in CI: the first of the three
/// admission levels. An id is added only after that backend's golden tests are green in CI, as a
/// reviewed edit of this list.
pub const GOLDEN_COMPILE_PASSED: &[&str] = &["esi-v2512"];

/// The state of the backend with this id; an id that is not in the table is a candidate.
fn admission_of(id: &str) -> Admission {
    ADMISSION
        .iter()
        .find(|(known, _)| *known == id)
        .map_or(Admission::Candidate, |(_, state)| *state)
}

/// The rows of one backend crate's slice, each with its state from the admission table.
fn rows_of(backends: &[&'static dyn Backend]) -> Vec<Row> {
    backends
        .iter()
        .map(|backend| Row {
            backend: *backend,
            admission: admission_of(backend.id()),
        })
        .collect()
}

/// Every registered backend, the default first.
pub fn rows() -> Vec<Row> {
    // One line per family; a new backend crate is one module and one line here.
    [
        esi::rows(),
        of14::rows(),
        su2::rows(),
        codesaturne::rows(),
        lbm::rows(),
    ]
    .concat()
}

/// The row with exactly this id.
pub fn by_id(id: &str) -> Option<Row> {
    rows().into_iter().find(|row| row.backend.id() == id)
}

/// The row used when no backend is named.
pub fn default_row() -> Row {
    let all = rows();
    // The registry always holds the default row, and the first row is the fallback.
    all.iter()
        .copied()
        .find(|row| row.admission == Admission::Default)
        .unwrap_or(all[0])
}

/// A word given by a person, shortened and with control characters replaced, for a message.
pub(crate) fn bounded(text: &str) -> String {
    text.chars()
        .take(40)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// The row with this id, or the finding that no backend has it.
#[allow(clippy::result_large_err)]
pub fn resolve(id: &str) -> Result<Row, Diagnostic> {
    by_id(id).ok_or_else(|| {
        let line = Line::BackendUnknown { id: bounded(id) };
        Diagnostic::error(
            "CFD-BACKEND-001",
            "backend",
            "",
            Text::english().render(&line),
        )
        .with_hint("List the registered backends to see the ids.")
    })
}

/// The image the row's backend runs in, or the finding that none is pinned yet.
#[allow(clippy::result_large_err)]
pub fn image_or_refusal(row: &Row) -> Result<&'static str, Diagnostic> {
    row.backend.image().ok_or_else(|| {
        let line = Line::BackendImagePending {
            id: bounded(row.backend.id()),
        };
        Diagnostic::error(
            "CFD-BACKEND-010",
            "backend",
            "",
            Text::english().render(&line),
        )
        .with_hint("Build the image and have it approved before running this backend.")
    })
}

/// The warnings a result from this row carries: a candidate has not passed all three gate levels,
/// and a preview backend's results are not gated results.
pub fn row_warnings(row: &Row) -> Vec<Diagnostic> {
    let text = Text::english();
    let mut found = Vec::new();
    if row.admission == Admission::Candidate {
        let line = Line::BackendCandidate {
            id: bounded(row.backend.id()),
        };
        found.push(Diagnostic::warning(
            "CFD-BACKEND-008",
            "backend",
            "",
            text.render(&line),
        ));
    }
    if row.backend.preview() {
        found.push(Diagnostic::warning(
            "CFD-BACKEND-009",
            "backend",
            "",
            text.render(&Line::BackendPreview),
        ));
    }
    found
}
