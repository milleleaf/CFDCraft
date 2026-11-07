//! The reader recovers a mesh dictionary of several blocks, its curved edges and its patches, and
//! the conditions each patch holds; the backend lists what a shaped study must have, and the
//! round-trip check passes only with that list. Every study goes through `resolve` and `compile`.

mod field_studies;
mod shape_studies;

use cfdcraft_backend_esi::{
    Dialect, compile,
    patches::layout_of,
    reader::{EsiReader, source_files},
    shaped_expected_facts,
};
use cfdcraft_model::{
    ResolvedStudy,
    roundtrip::{
        CaseReader, Fact, FactValue, SourceFile, check_roundtrip, check_roundtrip_with,
        grading_text,
    },
};
use shape_studies::{box_json, cylinder_json, graded_json, resolved, step_json};
use std::collections::BTreeMap;

fn read(study: &ResolvedStudy) -> (Vec<SourceFile>, BTreeMap<String, Fact>) {
    let tree = compile(study, Dialect::EsiV2512).unwrap();
    let files = source_files(&tree);
    let facts = EsiReader.read(&files).unwrap();
    (files, facts)
}

/// The check with the backend's list added; empty means every value was read back and matches.
fn found(study: &ResolvedStudy, files: &[SourceFile]) -> Vec<String> {
    check_roundtrip_with(&EsiReader, files, study, &shaped_expected_facts(study))
        .into_iter()
        .map(|d| format!("{} {} {}", d.code, d.setting, d.message))
        .collect()
}

fn value(facts: &BTreeMap<String, Fact>, key: &str) -> FactValue {
    facts
        .get(key)
        .unwrap_or_else(|| panic!("the reader found no fact {key}"))
        .value
        .clone()
}

fn word(text: &str) -> FactValue {
    FactValue::Text(text.into())
}

#[test]
fn the_step_passes_the_round_trip() {
    for startup in [true, false] {
        for level in 0..=2 {
            let study = resolved(&step_json(startup, level, "floor_walls"));
            let (files, _) = read(&study);
            assert_eq!(
                found(&study, &files),
                Vec::<String>::new(),
                "startup {startup}, level {level}"
            );
        }
    }
}

#[test]
fn the_cylinder_passes_the_round_trip() {
    for level in 0..=2 {
        let study = resolved(&cylinder_json(level, "cylinder"));
        let (files, facts) = read(&study);
        assert_eq!(found(&study, &files), Vec::<String>::new(), "level {level}");
        // The cylinder is a wall with the no-slip condition, bounded by eight arcs.
        assert_eq!(value(&facts, "/mesh/patch/obstacle/type"), word("wall"));
        assert_eq!(value(&facts, "/patch/obstacle/kind"), word("no-slip wall"));
        assert_eq!(value(&facts, "/mesh/arcs"), FactValue::Count(8));
        for k in 0..8 {
            assert!(facts.contains_key(&format!("/mesh/arc/{k}/ends")), "{k}");
            for axis in ["x", "y", "z"] {
                assert!(
                    facts.contains_key(&format!("/mesh/arc/{k}/through/{axis}")),
                    "{k} {axis}"
                );
            }
        }
        assert!(!facts.contains_key("/mesh/arc/8/ends"));
    }
}

/// The heated cavity with a wall grading at this level.
fn heated_graded(level: u32) -> String {
    field_studies::heated_json("", "1000", "100").replacen(
        r#""cells_z":1"#,
        &format!(r#""cells_z":1,"level":{level},"grading":{{"x":1.05,"y":1.05}}"#),
        1,
    )
}

#[test]
fn a_graded_box_passes() {
    for level in 0..=2 {
        for (name, json) in [
            ("heated cavity", heated_graded(level)),
            ("lid cavity", graded_json(level)),
        ] {
            let study = field_studies::resolved(&json);
            let (files, facts) = read(&study);
            assert_eq!(
                found(&study, &files),
                Vec::<String>::new(),
                "{name}, level {level}"
            );
            // One block: the box path reads it, and the text is the model's own.
            assert!(!facts.keys().any(|k| k.starts_with("/mesh/block/")));
            let layout = layout_of(&study).unwrap();
            assert_eq!(layout.blocks.len(), 1);
            assert_eq!(
                value(&facts, "/mesh/grading"),
                FactValue::Text(grading_text(&layout.blocks[0].grading)),
                "{name}, level {level}"
            );
            assert!(value_text(&facts, "/mesh/grading").starts_with("(("));
            assert_eq!(
                value(&facts, "/mesh/cells_x"),
                FactValue::Count(study.mesh.cells_x)
            );
        }
    }
}

fn value_text(facts: &BTreeMap<String, Fact>, key: &str) -> String {
    match value(facts, key) {
        FactValue::Text(text) => text,
        other => panic!("{key} is {other}"),
    }
}

#[test]
fn the_reader_facts_have_the_frozen_keys() {
    let study = resolved(&step_json(true, 0, "floor_walls"));
    let (_, facts) = read(&study);
    let layout = layout_of(&study).unwrap();
    assert_eq!(
        value(&facts, "/mesh/vertices"),
        FactValue::Count(layout.vertices.len() as u64)
    );
    assert_eq!(value(&facts, "/mesh/vertex/0/x"), FactValue::Number(0.0));
    assert_eq!(value(&facts, "/mesh/blocks"), FactValue::Count(4));
    assert_eq!(
        value(&facts, "/mesh/cells_total"),
        FactValue::Count(layout.cell_count)
    );
    // The start-up section is the first block: two cells along the flow and the upper channel's
    // rows across it.
    assert_eq!(value(&facts, "/mesh/block/0/cells"), word("2 66 1"));
    let corners = value_text(&facts, "/mesh/block/0/vertices");
    assert_eq!(corners.split(' ').count(), 8, "{corners}");
    assert!(facts.contains_key("/mesh/block/0/grading"));
    assert!(facts.contains_key("/mesh/block/3/grading"));
    assert!(!facts.contains_key("/mesh/block/4/grading"));
    assert_eq!(value(&facts, "/mesh/arcs"), FactValue::Count(0));
    assert_eq!(value(&facts, "/mesh/patch/inlet/type"), word("patch"));
    assert_eq!(value(&facts, "/mesh/patch/startup/type"), word("symmetry"));
    assert_eq!(value(&facts, "/patch/inlet/kind"), word("velocity inlet"));
    assert_eq!(value(&facts, "/patch/startup/kind"), word("symmetry plane"));
    assert_eq!(
        value(&facts, "/patch/inlet/velocity"),
        FactValue::Number(44.2)
    );
    assert_eq!(value(&facts, "/patch/frontAndBack/kind"), word("empty"));
    // The bounding box of the vertices is the domain, and the domain is open.
    assert_eq!(value(&facts, "/domain/length_x"), FactValue::Number(2.286));
    assert_eq!(value(&facts, "/domain/length_y"), FactValue::Number(0.1143));
    assert_eq!(value(&facts, "/domain/length_z"), FactValue::Number(0.0127));
    assert_eq!(value(&facts, "/domain/origin_x"), FactValue::Number(0.0));
    assert_eq!(value(&facts, "/domain/open"), FactValue::Flag(true));
    // The author's labels are never part of a key, and the box faces are not used.
    for key in facts.keys() {
        assert!(
            !key.contains("floor_walls") && !key.contains("top_wall"),
            "{key}"
        );
        assert!(!key.starts_with("/boundary_conditions/"), "{key}");
    }

    let cylinder = resolved(&cylinder_json(0, "cylinder"));
    let (_, facts) = read(&cylinder);
    assert_eq!(value(&facts, "/mesh/arcs"), FactValue::Count(8));
    assert_eq!(value(&facts, "/mesh/blocks"), FactValue::Count(12));
    assert!(facts.contains_key("/mesh/arc/0/ends"));
}

#[test]
fn no_extras_means_unaccounted_leaves() {
    for json in [
        step_json(true, 0, "floor_walls"),
        cylinder_json(0, "cylinder"),
    ] {
        let study = resolved(&json);
        let (files, _) = read(&study);
        let without = check_roundtrip(&EsiReader, &files, &study);
        assert!(!without.is_empty(), "the geometry has no expected facts");
        assert!(
            without.iter().all(|d| d.code == "CFD-RT-001"),
            "{without:?}"
        );
        assert_eq!(found(&study, &files), Vec::<String>::new());
    }
}

#[test]
fn a_box_study_still_uses_the_box_path() {
    let study = resolved(&box_json(128, ""));
    let (files, facts) = read(&study);
    assert_eq!(check_roundtrip(&EsiReader, &files, &study), vec![]);
    assert!(shaped_expected_facts(&study).is_empty());
    assert_eq!(value(&facts, "/mesh/cells_x"), FactValue::Count(128));
    assert_eq!(value(&facts, "/mesh/grading"), word("1 1 1"));
    assert_eq!(
        value(&facts, "/boundary_conditions/by_face/YMax/kind"),
        word("moving wall")
    );
    assert!(
        !facts
            .keys()
            .any(|k| k.starts_with("/patch/") || k.starts_with("/mesh/block") || k == "/mesh/arcs")
    );
}
