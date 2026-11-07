use cfdcraft_text::{Line, Text};

fn s(v: &str) -> String {
    v.to_string()
}

fn english(line: &Line) -> String {
    Text::english().render(line)
}

#[test]
fn the_preview_line_says_not_gated() {
    assert_eq!(
        english(&Line::BackendPreview),
        "Preview result: this backend's results are not gated results."
    );
}

#[test]
fn a_backend_row_names_id_family_admission_and_image() {
    let line = Line::BackendRow {
        id: s("su2-8.6.0"),
        family: s("su2"),
        admission: s("candidate"),
        image: s("not pinned"),
    };
    assert_eq!(
        english(&line),
        "su2-8.6.0 (su2): candidate, image not pinned."
    );
}

#[test]
fn every_new_line_renders_its_sentence() {
    let id = || s("su2-8.6.0");
    let cases: Vec<(Line, &str)> = vec![
        (Line::BackendNone, "No backend is registered."),
        (
            Line::BackendCandidate { id: id() },
            "su2-8.6.0 is a candidate: it has not passed all three gate levels yet.",
        ),
        (
            Line::BackendImagePending { id: id() },
            "The image of su2-8.6.0 is not pinned yet, so its cases cannot run.",
        ),
        (
            Line::BackendUnknown { id: s("nope-1") },
            "No backend has the id nope-1.",
        ),
        (
            Line::ParityTableHeader {
                benchmark: s("step"),
                case: s("medium"),
                baseline: s("esi-v2512"),
            },
            "Parity of step, medium, against esi-v2512.",
        ),
        (
            Line::ParityColumn {
                backend: id(),
                image: s("sha256:aa"),
                gate: s("PASS"),
                converged: s("yes"),
            },
            "su2-8.6.0 (image sha256:aa): gate PASS, converged yes.",
        ),
        (
            Line::ParityLevels {
                golden: s("pass"),
                oracle: s("fail"),
                parity: s("not_run"),
            },
            "Levels: golden compile pass, reference oracle fail, parity not_run.",
        ),
        (
            Line::ParityNoRuns {
                benchmark: s("step"),
                backend: id(),
            },
            "There is no recorded run of step under su2-8.6.0.",
        ),
        (
            Line::ParityConverged { backend: id() },
            "su2-8.6.0 is converged.",
        ),
        (
            Line::ParityNotConverged { backend: id() },
            "su2-8.6.0 is not converged; its column is shown but does not count towards admission.",
        ),
        (
            Line::ImportDone {
                name: s("cavity.cfdcase"),
                files: s("12"),
                refused: s("0"),
            },
            "Imported cavity.cfdcase: 12 files copied, 0 refused.",
        ),
        (
            Line::ImportRefusedEntry {
                file: s("system/fvSolution"),
                line: s("7"),
                rule: s("CFD-GUARD-002"),
            },
            "system/fvSolution, line 7: refused by CFD-GUARD-002.",
        ),
        (
            Line::ImportDialect {
                dialect: s("esi-v2606"),
            },
            "Detected case dialect: esi-v2606.",
        ),
        (
            Line::ImportReadOnly {
                name: s("cavity.cfdcase"),
            },
            "The original in cavity.cfdcase is read-only; edit the working copy.",
        ),
        (
            Line::ImportInventory {
                key: s("mesh kind"),
                value: s("blockMesh"),
            },
            "mesh kind: blockMesh.",
        ),
        (
            Line::ImportNoMatchingBackend {
                dialect: s("foundation-14"),
            },
            "No backend runs a case of the dialect foundation-14, so this case can be inspected but not run.",
        ),
    ];
    for (line, want) in cases {
        assert_eq!(english(&line), want);
    }
}
