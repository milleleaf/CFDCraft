//! The second dialect of the OpenCFD line, v2606, writes exactly what v2512 writes for every golden
//! study, apart from a file named in the delta table (empty today), and states its own identity.

mod plan_studies;
mod shape_studies;

use cfdcraft_backend::{Backend, CaseTree, CompileOptions, Family};
use cfdcraft_backend_esi::{
    Dialect,
    dialect::{self, IMAGE_NOTE_V2606, IMAGE_V2606, V2606_DELTA},
    rows::backends,
};
use cfdcraft_model::ResolvedStudy;
use plan_studies::{TRANSIENT_ONLY, cavity, heated, resolve_text};

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn first() -> &'static dyn Backend {
    backends()[0]
}

fn second() -> &'static dyn Backend {
    backends()[1]
}

/// The golden studies, the solver-plan studies and the two shaped ones.
fn studies() -> Vec<(&'static str, ResolvedStudy)> {
    vec![
        ("cavity v0", resolve_text(CAVITY_V0)),
        ("cavity v1", resolve_text(CAVITY_V1)),
        ("channel", resolve_text(CHANNEL_V1)),
        ("steady cavity", cavity("{}", "5000", "500", 0)),
        ("transient cavity", cavity(TRANSIENT_ONLY, "8", "0.5", 0)),
        ("heated cavity", heated("", "1000", "100")),
        (
            "cylinder",
            shape_studies::resolved(&shape_studies::cylinder_json(0, "cylinder")),
        ),
        (
            "step",
            shape_studies::resolved(&shape_studies::step_json(true, 0, "floor")),
        ),
    ]
}

/// The tree without the files the delta table names.
fn without_delta(tree: &CaseTree) -> CaseTree {
    let mut kept = tree.clone();
    kept.files.retain(|f| {
        !V2606_DELTA
            .iter()
            .any(|d| f.rel_path == std::path::Path::new(d.file))
    });
    kept
}

#[test]
fn the_cavity_is_written_identically_by_both_dialects() {
    let study = resolve_text(CAVITY_V0);
    let options = CompileOptions { np: 1 };
    let old = first().compile(&study, &options).unwrap();
    let new = second().compile(&study, &options).unwrap();
    assert_eq!(new, old);
    assert!(!new.files.is_empty());
    assert_eq!(
        second().export(&study, &new, "abc123", &options),
        first().export(&study, &old, "abc123", &options)
    );
}

#[test]
fn the_second_row_states_its_own_identity() {
    let row = second();
    assert_eq!(row.id(), "esi-v2606");
    assert_eq!(row.image(), Some(IMAGE_V2606));
    assert_ne!(row.image(), first().image());
    assert_eq!(row.family(), Family::Esi);
    assert!(!row.preview());
    assert_eq!(row.capabilities(), first().capabilities());
    assert_eq!(row.capabilities(), Dialect::EsiV2606.capabilities());
}

#[test]
fn the_image_is_pinned_by_a_digest() {
    for image in [IMAGE_V2606, dialect::IMAGE] {
        let (name, digest) = image.split_once("@sha256:").expect("a digest, not a tag");
        assert!(name.starts_with("opencfd/"), "{image}");
        assert_eq!(digest.len(), 64, "{image}");
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{image}"
        );
    }
}

#[test]
fn every_golden_study_is_written_identically() {
    for (name, study) in studies() {
        for np in [1, 2] {
            let options = CompileOptions { np };
            let old = first().compile(&study, &options).unwrap();
            let new = second().compile(&study, &options).unwrap();
            assert_eq!(
                without_delta(&new),
                without_delta(&old),
                "{name}, {np} processes"
            );
            assert_eq!(
                without_delta(&second().export(&study, &new, "abc123", &options)),
                without_delta(&first().export(&study, &old, "abc123", &options)),
                "{name}, {np} processes, exported"
            );
        }
    }
}

#[test]
fn the_delta_table_is_empty_today() {
    assert_eq!(
        V2606_DELTA.len(),
        0,
        "a real difference between v2512 and v2606 goes into V2606_DELTA with the file and the \
         reason before a difference may pass; do not loosen the equality test instead"
    );
}

#[test]
fn the_dialects_share_one_writer_and_differ_in_image() {
    assert_eq!(
        first().writer_sources().len(),
        second().writer_sources().len()
    );
    assert_eq!(first().writer_sources(), second().writer_sources());
    assert_ne!(first().image(), second().image());
    assert_eq!(Dialect::ALL, [Dialect::EsiV2512, Dialect::EsiV2606]);
}

#[test]
fn the_note_states_both_facts() {
    assert!(IMAGE_NOTE_V2606.contains("rc2"));
    assert!(IMAGE_NOTE_V2606.contains("final"));
    assert!(IMAGE_NOTE_V2606.contains("2606.0~rc2-1"));
}
