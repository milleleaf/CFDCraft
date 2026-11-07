//! The backend list: one row per registered backend, with its label and its notes.

use cfdcraft_backend_esi::dialect::{IMAGE, IMAGE_NOTE_V2606};
use cfdcraft_ops::backends::{GOLDEN_COMPILE_PASSED, rows};
use cfdcraft_ops::repro::{BackendsData, IMAGE_NOT_PINNED};
use cfdcraft_ops::{ExitClass, Repro, backend_list};
use cfdcraft_text::Line;

fn data() -> BackendsData {
    let report = backend_list();
    assert_eq!(report.class(), ExitClass::Passed);
    match report.repro {
        Some(Repro::Backends(data)) => *data,
        other => panic!("expected the backend list, found {other:?}"),
    }
}

fn row_ids(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| match line {
            Line::BackendRow { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn backend_list_has_one_row_per_backend() {
    let data = data();
    assert_eq!(data.backends.len(), 6);
    let registered: Vec<String> = rows().iter().map(|r| r.backend.id().to_owned()).collect();
    let listed: Vec<String> = data.backends.iter().map(|b| b.id.clone()).collect();
    assert_eq!(listed, registered);
    assert_eq!(row_ids(&data.lines()), registered);

    // The artifact is plain data with the named members.
    let json = serde_json::to_value(&data).unwrap();
    let array = json["backends"].as_array().unwrap();
    assert_eq!(array.len(), 6);
    for entry in array {
        let mut keys: Vec<&str> = entry
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "admission",
                "claims",
                "family",
                "id",
                "image",
                "note",
                "preview"
            ]
        );
    }

    let by = |id: &str| data.backends.iter().find(|b| b.id == id).unwrap();
    assert_eq!(by("esi-v2512").image, IMAGE);
    assert_eq!(by("esi-v2512").admission, "default");
    assert_eq!(by("esi-v2512").note, "");
    assert_eq!(by("esi-v2606").note, IMAGE_NOTE_V2606);
    assert!(by("esi-v2606").note.contains("rc2 packages"));
    assert!(by("esi-v2606").note.contains("tagged final"));
    assert_eq!(by("of14-20261008").family, "of14");
    assert_eq!(by("of14-20261008").image, IMAGE_NOT_PINNED);
    for id in ["su2-8.6.0", "codesaturne-9.2.0", "fieldcraft-lbm-0.7.5"] {
        assert_eq!(by(id).note, "", "{id}");
    }
    assert_eq!(
        data.backends.iter().filter(|b| !b.note.is_empty()).count(),
        1
    );
}

#[test]
fn a_preview_backend_is_labelled() {
    let data = data();
    let lines = data.lines();
    let preview: Vec<&str> = data
        .backends
        .iter()
        .filter(|b| b.preview)
        .map(|b| b.id.as_str())
        .collect();
    assert_eq!(preview, ["fieldcraft-lbm-0.7.5"]);
    assert_eq!(
        lines.iter().filter(|l| **l == Line::BackendPreview).count(),
        1
    );
    let at = lines
        .iter()
        .position(|l| matches!(l, Line::BackendRow { id, .. } if id == "fieldcraft-lbm-0.7.5"))
        .unwrap();
    assert_eq!(lines[at + 1], Line::BackendPreview);
    assert_eq!(
        data.backends.iter().find(|b| b.preview).map(|b| b.claims),
        Some(0),
        "nothing is claimed for the preview backend yet"
    );
}

#[test]
fn a_candidate_is_marked_and_the_default_is_not() {
    let data = data();
    let lines = data.lines();
    let candidates: Vec<String> = lines
        .iter()
        .filter_map(|line| match line {
            Line::BackendCandidate { id } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let expected: Vec<String> = data
        .backends
        .iter()
        .filter(|b| b.admission == "candidate")
        .map(|b| b.id.clone())
        .collect();
    assert_eq!(candidates, expected);
    assert!(!candidates.iter().any(|id| id == "esi-v2512"));
    assert_eq!(candidates.len(), 5);

    // A row whose image is not pinned says so, once.
    let pending: Vec<String> = lines
        .iter()
        .filter_map(|line| match line {
            Line::BackendImagePending { id } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let unpinned: Vec<String> = data
        .backends
        .iter()
        .filter(|b| b.image == IMAGE_NOT_PINNED)
        .map(|b| b.id.clone())
        .collect();
    assert_eq!(pending, unpinned);
    assert!(pending.iter().any(|id| id == "of14-20261008"));
    assert!(!pending.iter().any(|id| id == "esi-v2512"));
}

#[test]
fn the_golden_table_is_data() {
    // An id is added to this list only after that backend's golden tests are green in CI.
    assert_eq!(GOLDEN_COMPILE_PASSED.to_vec(), ["esi-v2512"]);
}
