//! The backend registry, its admission states, and the findings it gives.

use cfdcraft_backend::{
    Backend, CaseFile, CaseTree, CompileFailure, CompileOptions, Family, LimitKind, Refusal,
    RunPlan, RunStep,
};
use cfdcraft_backend_esi::Dialect;
use cfdcraft_model::{Capabilities, Diagnostic, ResolvedStudy, Severity};
use cfdcraft_ops::{
    backends::{
        Admission, Row, by_id, default_row, identity::tool_identity_for, image_or_refusal, resolve,
        rows,
    },
    prepare_row, tool_identity,
};
use std::collections::BTreeSet;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

const SOURCES: [(&str, &str); 1] = [("writer.rs", "fn write() {}")];

/// A backend with fixed small values, used where a test needs a row of its own.
struct Fake {
    image: Option<&'static str>,
    preview: bool,
}

impl Backend for Fake {
    fn id(&self) -> &'static str {
        "fake-1"
    }
    fn family(&self) -> Family {
        Family::Lbm
    }
    fn image(&self) -> Option<&'static str> {
        self.image
    }
    fn preview(&self) -> bool {
        self.preview
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::unrestricted()
    }
    fn compile(
        &self,
        _study: &ResolvedStudy,
        _options: &CompileOptions,
    ) -> Result<CaseTree, CompileFailure> {
        Ok(CaseTree {
            files: vec![CaseFile {
                rel_path: "input.txt".into(),
                bytes: b"hello".to_vec(),
            }],
        })
    }
    fn scan(&self, _tree: &CaseTree) -> Vec<Refusal> {
        Vec::new()
    }
    fn read_back(&self, _study: &ResolvedStudy, _tree: &CaseTree) -> Vec<Diagnostic> {
        Vec::new()
    }
    fn export(
        &self,
        _study: &ResolvedStudy,
        tree: &CaseTree,
        _hash: &str,
        _options: &CompileOptions,
    ) -> CaseTree {
        tree.clone()
    }
    fn run_plan(&self, _study: &ResolvedStudy, _np: u32) -> RunPlan {
        RunPlan {
            solver: "fake".into(),
            steps: vec![RunStep {
                command: "fake input.txt".into(),
                log: "log.fake".into(),
            }],
            solver_log: "log.fake".into(),
            judged_logs: vec!["log.fake".into()],
            final_fields: vec!["out.txt".into()],
            transient: false,
            buoyant: false,
            energy_field: "T".to_owned(),
            pressure_field: String::new(),
            residual_checks: vec![("rho".into(), LimitKind::Pressure)],
            potential: false,
        }
    }
    fn writer_sources(&self) -> &'static [(&'static str, &'static str)] {
        &SOURCES
    }
}

static NO_IMAGE: Fake = Fake {
    image: None,
    preview: false,
};
static PREVIEW: Fake = Fake {
    image: Some("example/fake@sha256:abc"),
    preview: true,
};

fn row_of(backend: &'static dyn Backend, admission: Admission) -> Row {
    Row { backend, admission }
}

fn codes(found: &[Diagnostic]) -> Vec<&str> {
    found.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn the_registry_finds_rows_by_id() {
    let row = by_id("esi-v2512").expect("the default backend is registered");
    assert_eq!(row.admission, Admission::Default);
    assert!(by_id("nope").is_none());
    assert_eq!(default_row().backend.id(), "esi-v2512");
    let all = rows();
    let ids: BTreeSet<&str> = all.iter().map(|r| r.backend.id()).collect();
    assert_eq!(ids.len(), all.len(), "every id is unique");
    assert_eq!(
        [
            Admission::Candidate,
            Admission::Supported,
            Admission::Default
        ]
        .map(Admission::as_str),
        ["candidate", "supported", "default"]
    );
}

#[test]
fn a_candidate_row_adds_the_candidate_warning() {
    let options = CompileOptions::default();
    let candidate = prepare_row(CAVITY, &row_of(&NO_IMAGE, Admission::Candidate), &options)
        .expect("the fake backend writes a case");
    assert_eq!(codes(&candidate.warnings), ["CFD-BACKEND-008"]);
    assert_eq!(candidate.warnings[0].severity, Severity::Warning);
    assert!(candidate.warnings[0].message.contains("fake-1"));

    let default = prepare_row(CAVITY, &default_row(), &options).unwrap();
    assert!(
        default.warnings.iter().all(|d| d.code != "CFD-BACKEND-008"),
        "{:?}",
        default.warnings
    );

    let preview = prepare_row(CAVITY, &row_of(&PREVIEW, Admission::Supported), &options).unwrap();
    assert_eq!(codes(&preview.warnings), ["CFD-BACKEND-009"]);
    assert_eq!(preview.warnings[0].severity, Severity::Warning);
}

#[test]
fn an_unpinned_image_is_refused_at_run_time_with_its_own_code() {
    let refused = image_or_refusal(&row_of(&NO_IMAGE, Admission::Candidate)).unwrap_err();
    assert_eq!(refused.code, "CFD-BACKEND-010");
    assert_eq!(refused.severity, Severity::Error);
    assert!(refused.message.contains("fake-1"));
    assert!(refused.hint.is_some());

    let image = image_or_refusal(&by_id("esi-v2512").unwrap()).unwrap();
    assert!(image.contains("@sha256:"), "{image}");
}

#[test]
fn an_unknown_id_is_a_named_diagnostic() {
    let found = resolve("nope").unwrap_err();
    assert_eq!(found.code, "CFD-BACKEND-001");
    assert!(found.message.contains("nope"), "{}", found.message);

    let hostile = format!("a\u{1b}[31m{}", "x".repeat(200));
    let found = resolve(&hostile).unwrap_err();
    assert!(
        !found.message.chars().any(char::is_control),
        "{}",
        found.message
    );
    assert!(found.message.len() < 200, "{}", found.message);

    assert!(resolve("esi-v2512").is_ok());
}

#[test]
fn the_backend_identity_equals_the_old_one_for_esi() {
    let row = by_id("esi-v2512").unwrap();
    assert_eq!(
        tool_identity_for(row.backend),
        tool_identity(Dialect::EsiV2512)
    );

    let unpinned = tool_identity_for(&NO_IMAGE);
    assert_eq!(unpinned.image_digest, "unpinned");
    assert_eq!(unpinned.dialect, "fake-1");
    assert_eq!(unpinned.sidecar_lock, None);
    assert!(unpinned.writer_version.contains('+'));

    let pinned = tool_identity_for(&PREVIEW);
    assert_eq!(pinned.image_digest, "sha256:abc");
    // The same sources give the same writer version whatever the image.
    assert_eq!(pinned.writer_version, unpinned.writer_version);
}

#[test]
fn the_foundation_14_row_is_registered_as_a_candidate() {
    let row = by_id("of14-20261008").expect("the Foundation 14 backend is registered");
    assert_eq!(row.admission, Admission::Candidate);
    assert_eq!(row.backend.family(), Family::Of14);
    assert_eq!(
        row.backend.image(),
        None,
        "an image is added only after an approved push"
    );
    let ids: Vec<&str> = rows().iter().map(|r| r.backend.id()).collect();
    let at = ids.iter().position(|id| *id == "of14-20261008").unwrap();
    assert_eq!(&ids[..at], ["esi-v2512", "esi-v2606"], "{ids:?}");
}

#[test]
fn the_registry_still_has_one_default() {
    let defaults: Vec<&str> = rows()
        .iter()
        .filter(|r| r.admission == Admission::Default)
        .map(|r| r.backend.id())
        .collect();
    assert_eq!(defaults, ["esi-v2512"]);
}

#[test]
fn all_six_backends_are_registered_in_a_fixed_order() {
    let all = rows();
    let ids: Vec<&str> = all.iter().map(|r| r.backend.id()).collect();
    assert_eq!(
        ids,
        [
            "esi-v2512",
            "esi-v2606",
            "of14-20261008",
            "su2-8.6.0",
            "codesaturne-9.2.0",
            "fieldcraft-lbm-0.7.5"
        ]
    );
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len());
    for row in &all {
        let expected = if row.backend.id() == "esi-v2512" {
            Admission::Default
        } else {
            Admission::Candidate
        };
        assert_eq!(row.admission, expected, "{}", row.backend.id());
    }
}

#[test]
fn only_the_lattice_backend_is_a_preview() {
    for row in rows() {
        let id = row.backend.id();
        assert_eq!(row.backend.preview(), id == "fieldcraft-lbm-0.7.5", "{id}");
        let pinned = id.starts_with("esi-");
        assert_eq!(
            row.backend.image().is_some(),
            pinned,
            "{id}: an image is added only after an approved push"
        );
    }
}

#[test]
fn families_match_the_ids() {
    for row in rows() {
        let id = row.backend.id();
        let prefix = id.split('-').next().unwrap();
        let prefix = if prefix == "fieldcraft" {
            "lbm"
        } else {
            prefix
        };
        assert_eq!(row.backend.family().name(), prefix, "{id}");
    }
}
