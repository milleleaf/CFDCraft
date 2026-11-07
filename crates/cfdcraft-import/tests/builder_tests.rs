//! The candidate list built from an etc probe: pure, sorted, and never listing a file with a hit.

use cfdcraft_import::{Allowlist, Candidate, Excluded, build_candidate, parse_table};

const PROBE: &str = "ARCH x86_64
ENVIRONMENT present in the login shell
PROJECT_VERSION 2512
LIST etc/caseDicts/setConstraintTypes
LIST etc/caseDicts/mesh/generation/snappyHexMeshDict.cfg
LIST etc/caseDicts/functionTemplates/field
LIST etc/caseDicts/functions/streamlinesLine
HIT etc/caseDicts/functionTemplates/field type coded
COUNT files 4 hits 1
PROBE-DONE etc-listing
";

fn rows(c: &Candidate) -> (Vec<&str>, Vec<&str>) {
    (
        c.etc.iter().map(String::as_str).collect(),
        c.functions.iter().map(String::as_str).collect(),
    )
}

#[test]
fn the_builder_excludes_every_file_with_a_hit() {
    let c = build_candidate(PROBE);
    assert_eq!(
        rows(&c),
        (
            vec![
                "caseDicts/mesh/generation/snappyHexMeshDict.cfg",
                "caseDicts/setConstraintTypes"
            ],
            vec!["streamlinesLine"]
        )
    );
    assert_eq!(
        c.excluded,
        [Excluded {
            path: "etc/caseDicts/functionTemplates/field".to_string(),
            reasons: vec![
                "type coded".to_string(),
                "under functionTemplates".to_string()
            ],
        }]
    );
    let text = c.render();
    assert_eq!(
        text.lines().skip(2).collect::<Vec<_>>(),
        [
            "etc caseDicts/mesh/generation/snappyHexMeshDict.cfg",
            "etc caseDicts/setConstraintTypes",
            "func streamlinesLine",
            "# excluded",
            "# etc/caseDicts/functionTemplates/field: type coded, under functionTemplates",
        ]
    );

    // The order of the probe lines does not change the result.
    let reversed: Vec<&str> = PROBE.lines().rev().collect();
    let again = build_candidate(&reversed.join("\n"));
    assert_eq!(again, c);
    assert_eq!(again.render(), text);
}

#[test]
fn a_file_with_any_hit_is_left_out_with_every_construct() {
    let probe = "LIST etc/caseDicts/a
LIST etc/caseDicts/b
LIST etc/caseDicts/c
LIST etc/caseDicts/functions/f
HIT etc/caseDicts/a #calc
HIT etc/caseDicts/a libs
HIT etc/caseDicts/b systemCall
HIT etc/caseDicts/functions/f codeStream
";
    let c = build_candidate(probe);
    assert_eq!(rows(&c), (vec!["caseDicts/c"], vec![]));
    let left: Vec<(&str, Vec<&str>)> = c
        .excluded
        .iter()
        .map(|e| {
            (
                e.path.as_str(),
                e.reasons.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        left,
        [
            ("etc/caseDicts/a", vec!["#calc", "libs"]),
            ("etc/caseDicts/b", vec!["systemCall"]),
            ("etc/caseDicts/functions/f", vec!["codeStream"]),
        ]
    );
}

#[test]
fn function_templates_are_never_listed() {
    let probe = "LIST etc/caseDicts/functionTemplates/field
LIST etc/caseDicts/functionTemplates/sub/fieldAverage
LIST etc/caseDicts/functions/functionTemplates/x
LIST etc/caseDicts/postProcessing/functionTemplates/y
LIST etc/caseDicts/setConstraintTypes
";
    let c = build_candidate(probe);
    assert_eq!(rows(&c), (vec!["caseDicts/setConstraintTypes"], vec![]));
    assert_eq!(c.excluded.len(), 4);
    assert!(
        c.excluded
            .iter()
            .all(|e| e.reasons == ["under functionTemplates"]),
        "{:?}",
        c.excluded
    );
}

#[test]
fn function_names_are_the_path_below_their_folder() {
    let probe = "LIST etc/caseDicts/functions/streamlinesLine
LIST etc/caseDicts/postProcessing/probes/pressure
LIST etc/caseDicts/functions
LIST etc/caseDicts/functionsExtra/y
LIST etc/other/z
LIST etc/caseDicts
LIST usr/etc/caseDicts/q
";
    let c = build_candidate(probe);
    assert_eq!(
        rows(&c),
        (
            vec!["caseDicts/functions", "caseDicts/functionsExtra/y"],
            vec!["probes/pressure", "streamlinesLine"]
        )
    );
    assert!(c.excluded.is_empty());
}

#[test]
fn a_name_the_list_cannot_hold_is_left_out() {
    let probe = "LIST etc/caseDicts/a b
LIST etc/caseDicts/$x
LIST etc/caseDicts/ok
";
    let c = build_candidate(probe);
    assert_eq!(rows(&c), (vec!["caseDicts/ok"], vec![]));
    assert_eq!(c.excluded.len(), 2);
    assert!(
        c.excluded
            .iter()
            .all(|e| e.reasons == ["name the list cannot hold"])
    );
}

#[test]
fn an_empty_or_unrelated_probe_gives_an_empty_candidate() {
    for text in [
        "",
        "NO-CASEDICTS the folder was not found\nPROBE-DONE etc-listing\n",
    ] {
        let c = build_candidate(text);
        assert_eq!(c, Candidate::default());
        assert_eq!(parse_table(&c.render()).unwrap(), Allowlist::empty());
    }
}

#[test]
fn a_rendered_candidate_loads_as_a_table() {
    let c = build_candidate(PROBE);
    let list = parse_table(&c.render()).unwrap();
    assert_eq!(
        list,
        Allowlist::new(c.etc.iter(), c.functions.iter()).unwrap()
    );
    assert_eq!(list.len(), 3);
}
