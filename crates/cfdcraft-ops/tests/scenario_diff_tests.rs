//! Showing a variant and comparing two of them: the differences read in study terms, a raw
//! override is visible in the study, the diff and the result key, and a variant that fails its
//! own checks says why.

use std::path::{Path, PathBuf};

use cfdcraft_keys::{Axis, Change};
use cfdcraft_model::Severity;
use cfdcraft_ops::scenario_ops::{scenario_diff, scenario_list, scenario_set, scenario_show};
use cfdcraft_ops::{
    DiffData, DocOutcome, ExitClass, RawView, Repro, ScenarioData, ScenarioSummary,
};
use cfdcraft_scenarios::Override;
use cfdcraft_text::Line;
use serde_json::json;

#[path = "repro_support/mod.rs"]
mod repro_support;
use repro_support::*;

const RELAX_U: &str = "esi system/fvSolution relaxationFactors/equations/U";

fn lid(value: f64) -> Override {
    Override::Set {
        setting: "/boundary_conditions/lid/velocity/value".to_owned(),
        value: json!(value),
    }
}

fn cells_x(count: u32) -> Override {
    Override::Set {
        setting: "/mesh/cells_x".to_owned(),
        value: json!(count),
    }
}

fn raw(label: &str, file: &str, entry: &str, value: &str) -> Override {
    Override::Set {
        setting: format!("/raw_overrides/{label}"),
        value: json!({ "backend": "esi", "file": file, "entry": entry, "value": value }),
    }
}

fn relax_u(value: &str) -> Override {
    raw(
        "relax-u",
        "system/fvSolution",
        "relaxationFactors/equations/U",
        value,
    )
}

fn create(path: &Path, id: &str, parent: Option<&str>, sets: &[Override]) {
    let out = scenario_set(path, id, parent, sets, None);
    passed(&out.report);
}

/// A document of the cavity with `fast-lid` (lid 0.2), `fine` (cells in x 256) and `fine-fast`
/// (below `fine`, lid 0.3).
fn branches(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    let path = new_document(&dir, "cavity", CAVITY);
    create(&path, "fast-lid", None, &[lid(0.2)]);
    create(&path, "fine", None, &[cells_x(256)]);
    create(&path, "fine-fast", Some("fine"), &[lid(0.3)]);
    path
}

fn diff_data(out: &DocOutcome) -> &DiffData {
    match &out.report.repro {
        Some(Repro::ScenarioDiff(data)) => data,
        other => panic!("expected the data of a diff, found {other:?}"),
    }
}

fn shown(out: &DocOutcome) -> &ScenarioData {
    match &out.report.repro {
        Some(Repro::Scenario(data)) => data,
        other => panic!("expected the data of a variant, found {other:?}"),
    }
}

fn listed(path: &Path) -> Vec<ScenarioSummary> {
    let out = scenario_list(path);
    passed(&out.report);
    match out.report.repro {
        Some(Repro::Scenarios(data)) => data.scenarios,
        other => panic!("expected the list of variants, found {other:?}"),
    }
}

fn has(data: &DiffData, reason: &str) -> bool {
    data.reasons.iter().any(|found| found == reason)
}

#[test]
fn a_diff_reads_in_study_terms() {
    let path = branches("dif-terms");
    let out = scenario_diff(&path, "base", "fine-fast");
    passed(&out.report);

    let data = diff_data(&out);
    assert!(!data.same);
    assert_eq!(
        (data.from.as_str(), data.to.as_str()),
        ("base", "fine-fast")
    );
    assert!(
        has(data, "lid velocity 0.1 → 0.3 m/s"),
        "{:?}",
        data.reasons
    );
    assert!(has(data, "cells in x 128 → 256"), "{:?}", data.reasons);
    assert!(
        data.sentence.starts_with("fine-fast differs from base: "),
        "{}",
        data.sentence
    );
    assert!(data.sentence.ends_with('.'), "{}", data.sentence);
    assert_eq!(
        out.line,
        Some(Line::ScenarioDiffers {
            from: "base".to_owned(),
            to: "fine-fast".to_owned(),
            reasons: data.reasons.join("; "),
        })
    );
    assert_eq!(say(out.line.as_ref().unwrap()), data.sentence);
}

#[test]
fn a_diff_between_two_branches_goes_through_both_effective_studies() {
    let path = branches("dif-branches");
    let out = scenario_diff(&path, "fast-lid", "fine");
    passed(&out.report);
    let data = diff_data(&out);
    assert!(
        has(data, "lid velocity 0.2 → 0.1 m/s"),
        "{:?}",
        data.reasons
    );
    assert!(has(data, "cells in x 128 → 256"), "{:?}", data.reasons);
}

#[test]
fn two_variants_that_make_the_same_case_are_the_same() {
    let path = branches("dif-same");
    create(&path, "copy", None, &[]);
    let out = scenario_diff(&path, "base", "copy");
    passed(&out.report);

    let data = diff_data(&out);
    assert!(data.same);
    assert!(data.changes.is_empty() && data.reasons.is_empty());
    assert_eq!(
        out.line,
        Some(Line::ScenarioSame {
            from: "base".to_owned(),
            to: "copy".to_owned()
        })
    );
}

#[test]
fn the_diff_does_not_repeat_the_mesh_or_the_tool() {
    let path = branches("dif-mesh-tool");
    for (a, b) in [
        ("base", "fine"),
        ("fine", "fine-fast"),
        ("fast-lid", "fine-fast"),
    ] {
        let out = scenario_diff(&path, a, b);
        passed(&out.report);
        let data = diff_data(&out);
        assert!(!data.changes.is_empty());
        assert!(
            data.changes
                .iter()
                .all(|change| !matches!(change, Change::MeshChanged | Change::Tool { .. })),
            "{:?}",
            data.changes
        );
    }
    let out = scenario_diff(&path, "base", "fine");
    let data = diff_data(&out);
    assert!(
        matches!(
            data.changes.as_slice(),
            [Change::Cells { axis: Axis::X, .. }]
        ),
        "{:?}",
        data.changes
    );
}

#[test]
fn a_raw_override_shows_in_the_diff_and_changes_the_result_key_only() {
    let path = branches("dif-raw");
    create(&path, "tuned", None, &[relax_u("0.5")]);

    let out = scenario_diff(&path, "base", "tuned");
    passed(&out.report);
    let data = diff_data(&out);
    assert_eq!(
        data.changes,
        vec![Change::RawOverride {
            id: "relax-u".to_owned(),
            from: None,
            to: Some(format!("{RELAX_U} = 0.5")),
        }]
    );
    assert!(
        has(
            data,
            &format!("raw override relax-u none → {RELAX_U} = 0.5")
        ),
        "{:?}",
        data.reasons
    );

    let rows = listed(&path);
    let base = rows.iter().find(|row| row.id == "base").unwrap();
    let tuned = rows.iter().find(|row| row.id == "tuned").unwrap();
    assert_eq!(tuned.mesh_key, base.mesh_key);
    assert!(tuned.result_key.is_some());
    assert_ne!(tuned.result_key, base.result_key);
    assert_eq!(tuned.kind, "setting");

    let value = Override::Set {
        setting: "/raw_overrides/relax-u/value".to_owned(),
        value: json!("0.7"),
    };
    create(&path, "tuned-more", Some("tuned"), &[value]);
    let out = scenario_diff(&path, "tuned", "tuned-more");
    passed(&out.report);
    let data = diff_data(&out);
    assert!(
        has(
            data,
            &format!("raw override relax-u {RELAX_U} = 0.5 → {RELAX_U} = 0.7")
        ),
        "{:?}",
        data.reasons
    );
}

#[test]
fn a_mesh_dictionary_override_changes_the_mesh_key_too() {
    let path = branches("dif-raw-mesh");
    create(
        &path,
        "meshy",
        None,
        &[raw("blk", "system/blockMeshDict", "scale", "1")],
    );

    let out = scenario_diff(&path, "base", "meshy");
    passed(&out.report);
    let data = diff_data(&out);
    assert!(
        data.changes.contains(&Change::RawMesh),
        "{:?}",
        data.changes
    );
    assert!(
        data.changes
            .iter()
            .any(|change| matches!(change, Change::RawOverride { id, .. } if id == "blk")),
        "{:?}",
        data.changes
    );

    let rows = listed(&path);
    let base = rows.iter().find(|row| row.id == "base").unwrap();
    let meshy = rows.iter().find(|row| row.id == "meshy").unwrap();
    assert!(meshy.mesh_key.is_some());
    assert_ne!(meshy.mesh_key, base.mesh_key);
    assert_eq!(meshy.kind, "geometry");
}

#[test]
fn show_reports_overrides_raw_overrides_and_keys() {
    let path = branches("dif-show");
    create(&path, "tuned", None, &[relax_u("0.5")]);

    let out = scenario_show(&path, "tuned");
    passed(&out.report);
    assert_eq!(out.line, None);
    let data = shown(&out);
    assert_eq!(data.summary.id, "tuned");
    assert_eq!(data.summary.parent.as_deref(), Some("base"));
    assert!(data.summary.mesh_key.is_some() && data.summary.result_key.is_some());
    assert_eq!(data.overrides, vec![relax_u("0.5")]);
    assert_eq!(
        data.raw_overrides,
        vec![RawView {
            id: "relax-u".to_owned(),
            backend: "esi".to_owned(),
            file: "system/fvSolution".to_owned(),
            entry: "relaxationFactors/equations/U".to_owned(),
            value: "0.5".to_owned(),
        }]
    );
    assert!(
        out.report
            .artifacts
            .iter()
            .any(|a| a.kind == "scenario" && a.name == "tuned")
    );

    // The study itself shows no changes and no raw entry.
    let base = scenario_show(&path, "base");
    passed(&base.report);
    let data = shown(&base);
    assert_eq!(data.summary.kind, "base");
    assert!(data.overrides.is_empty() && data.raw_overrides.is_empty());

    // A raw value the checks refuse is a warning here, and the variant has no keys.
    create(
        &path,
        "unsafe-raw",
        None,
        &[raw("bad", "system/fvSolution", "scale", "1 #include x")],
    );
    let out = scenario_show(&path, "unsafe-raw");
    passed(&out.report);
    assert!(
        out.report
            .diagnostics
            .iter()
            .any(|d| d.code == "CFD-RAW-004" && d.severity == Severity::Warning),
        "{}",
        out.report.render_text()
    );
    let data = shown(&out);
    assert_eq!(data.summary.mesh_key, None);
    assert_eq!(data.summary.result_key, None);
    assert_eq!(data.raw_overrides.len(), 1);
}

#[test]
fn an_unknown_variant_is_cfd_scn_001_in_show_and_diff() {
    let path = branches("dif-unknown");
    let outs = [
        scenario_show(&path, "ghost"),
        scenario_diff(&path, "base", "ghost"),
        scenario_diff(&path, "ghost", "base"),
    ];
    for out in outs {
        assert_eq!(out.report.class(), ExitClass::StudyInvalid);
        assert_eq!(codes(&out.report), ["CFD-SCN-001"]);
        assert!(out.report.diagnostics[0].message.contains("ghost"));
        assert_eq!(out.line, None);
    }
}

#[test]
fn a_diff_of_a_variant_that_fails_the_checks_reports_its_findings() {
    let path = branches("dif-failing");
    create(&path, "stopped-lid", None, &[lid(0.0)]);

    let out = scenario_diff(&path, "base", "stopped-lid");
    assert_eq!(out.report.class(), ExitClass::StudyInvalid);
    assert!(
        codes(&out.report).contains(&"CFD-VAL-002"),
        "{}",
        out.report.render_text()
    );
    assert!(out.report.repro.is_none());
    assert_eq!(out.line, None);
}

#[test]
fn reading_operations_do_not_change_the_file() {
    let path = branches("dif-readonly");
    let before = snapshot(&path);
    passed(&scenario_list(&path).report);
    passed(&scenario_show(&path, "fine-fast").report);
    passed(&scenario_diff(&path, "base", "fine-fast").report);
    assert_eq!(snapshot(&path), before);
}
