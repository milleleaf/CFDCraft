//! A transient case continues from a stop between two writes and still writes its end time, and
//! the end time of every transient case is a whole number of write intervals.

mod plan_studies;

use cfdcraft_backend_esi::{
    CaseFile, CaseTree, Dialect, case::continue_case, compile, guard::scan_tree,
};
use plan_studies::{TRANSIENT_ONLY, cavity};
use std::path::Path;

fn tree(end: &str, interval: &str, level: u32) -> CaseTree {
    compile(
        &cavity(TRANSIENT_ONLY, end, interval, level),
        Dialect::EsiV2512,
    )
    .unwrap()
}

fn field(path: &str, body: &str) -> CaseFile {
    CaseFile {
        rel_path: path.into(),
        bytes: format!(
            "FoamFile\n{{\n    version 2.0;\n    format ascii;\n    class volVectorField;\n    object field;\n}}\n\ninternalField uniform {body};\n"
        )
        .into_bytes(),
    }
}

fn control(tree: &CaseTree) -> String {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new("system/controlDict"))
        .unwrap();
    String::from_utf8(file.bytes.clone()).unwrap()
}

/// The number in the first top-level line `<key> <number>;` of the control dictionary.
fn number(text: &str, key: &str) -> f64 {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(key))
        .filter_map(|rest| rest.trim().strip_suffix(';'))
        .find_map(|n| n.trim().parse::<f64>().ok())
        .unwrap_or_else(|| panic!("no number for {key}"))
}

#[test]
fn a_transient_case_continues_and_writes_its_end_time() {
    let original = tree("8", "0.5", 0);
    let seed = vec![field("3.5/U", "(0 0 0)"), field("3.5/p", "0")];
    let continued = continue_case(&original, &seed).unwrap();
    let text = control(&continued);
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    for wanted in [
        "startFrom latestTime;",
        "writeControl runTime;",
        "writeInterval 4.5;",
        "endTime 8.0;",
        "application pimpleFoam;",
        "deltaT 0.001;",
    ] {
        assert!(lines.contains(&wanted), "{wanted}\n{text}");
    }
    assert!(scan_tree(&continued).is_empty());
    // The first write after the restart falls exactly on the end time.
    assert_eq!(
        3.5 + number(&text, "writeInterval "),
        number(&text, "endTime ")
    );
    assert!(
        continued
            .files
            .iter()
            .any(|f| f.rel_path == Path::new("3.5/p"))
    );
}

#[test]
fn the_end_time_is_a_multiple_of_the_write_interval_in_the_compiled_case() {
    for (end, interval, level) in [
        ("8", "0.5", 0),
        ("8", "0.5", 1),
        ("10", "0.25", 0),
        ("2", "2", 0),
    ] {
        let text = control(&tree(end, interval, level));
        let ratio = number(&text, "endTime ") / number(&text, "writeInterval ");
        assert!(
            (ratio - ratio.round()).abs() < 1e-9,
            "{end} over {interval}: {ratio}"
        );
    }
}
