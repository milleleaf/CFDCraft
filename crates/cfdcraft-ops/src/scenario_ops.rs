//! Creating, changing, listing, showing, diffing and labelling the variants of a document. Every
//! function takes the path of the document and returns a `DocOutcome`.
//!
//! Finding codes of this family: 001 there is no such variant; 002 the list of variants is
//! refused; 003 a change of a variant does not apply to the study. All are errors and make exit
//! class 2. A report names variants by id and never the path of the document.

use std::collections::HashMap;
use std::path::Path;

use cfdcraft_backend_esi::Dialect;
use cfdcraft_doc::{Document, Stamp, open, open_stamped, save_if_unchanged};
use cfdcraft_keys::{Change, StageKeys, StageRecipes, ToolIdentity, diff_mesh, diff_result};
use cfdcraft_model::{Diagnostic, MigrationNote, Severity, Study, diagnose, has_errors, resolve};
use cfdcraft_runs::is_plain_name;
use cfdcraft_scenarios::{
    ApplyError, BASE, Override, Scenario, ScenarioError, SettingClass, chain, check_scenarios,
    classify, effective_study,
};
use cfdcraft_text::Line;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::documents::{
    DocOutcome, Finding, is_pack_name, migration_with, not_written, pack_kept, problem, refused,
    say, study_hash,
};
use crate::jobs::open_refusal;
use crate::keys::tool_identity;
use crate::reasons::{reason_parts, reason_text};
use crate::report::Report;
use crate::repro::{DiffData, RawView, Repro, ScenarioData, ScenarioSummary, ScenariosData};

const SET: &str = "scenario-set";
const LIST: &str = "scenario-list";
const SHOW: &str = "scenario-show";
const DIFF: &str = "scenario-diff";
const LABEL: &str = "scenario-label";

/// The setting named when a change fails on the study as a whole rather than on one setting.
const WHOLE_STUDY: &str = "the study";

/// The study to use for `scenario` and the tag a run made from it carries.
///
/// `None` and `base` give a copy of the document's own study and no tag. Any other id gives the
/// effective study of that variant and the id as the tag. A variant that does not exist, or whose
/// changes do not apply, is refused with a finding of class 2 for `operation`.
pub fn effective_of(
    operation: &str,
    document: &Document,
    scenario: Option<&str>,
) -> Result<(Study, Option<String>), Box<DocOutcome>> {
    let id = match scenario {
        None | Some(BASE) => return Ok((document.study.clone(), None)),
        Some(id) => id,
    };
    effective_study(&document.study, &document.artifacts.scenarios, id)
        .map(|study| (study, Some(id.to_owned())))
        .map_err(|error| Box::new(refused(operation, problem(refusal(&error)))))
}

/// The finding that tells a person why a variant cannot be used.
fn refusal(error: &ScenarioError) -> Finding {
    match error {
        ScenarioError::Unknown { scenario } => (
            "CFD-SCN-001",
            Line::ScenarioUnknown {
                scenario: scenario.clone(),
            },
        ),
        ScenarioError::Apply { scenario, source } => (
            "CFD-SCN-003",
            Line::OverrideNotApplied {
                scenario: scenario.clone(),
                setting: setting_of(source),
                reason: source.to_string(),
            },
        ),
        other => (
            "CFD-SCN-002",
            Line::ScenarioRefused {
                reason: other.to_string(),
            },
        ),
    }
}

fn setting_of(error: &ApplyError) -> String {
    match error {
        ApplyError::BadSetting { setting, .. }
        | ApplyError::MissingParent { setting }
        | ApplyError::NotAnObject { setting }
        | ApplyError::NothingToRemove { setting } => setting.clone(),
        ApplyError::NotAStudy { .. } => WHOLE_STUDY.to_owned(),
    }
}

/// What a person may change about a variant without changing what it makes. A name or notes given
/// as an empty text are cleared; a field left `None` is kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LabelUpdate {
    /// The new name.
    pub name: Option<String>,
    /// Whether pruning keeps the stored results of the variant.
    pub pinned: Option<bool>,
    /// The new notes.
    pub notes: Option<String>,
}

type Reply = Result<DocOutcome, Box<DocOutcome>>;

fn refusal_outcome(operation: &str, error: &ScenarioError) -> Box<DocOutcome> {
    Box::new(refused(operation, problem(refusal(error))))
}

fn unknown(operation: &str, id: &str) -> Box<DocOutcome> {
    let error = ScenarioError::Unknown {
        scenario: id.chars().take(40).collect(),
    };
    refusal_outcome(operation, &error)
}

/// The document being changed, the stamp of the bytes it was read from and the note for an older
/// file that the save brings forward.
struct Editing {
    document: Document,
    stamp: Stamp,
    migration: Option<MigrationNote>,
}

/// Open the document at `path` to change it. A pack is refused, because saving it back would drop
/// the results it carries.
fn edit_open(operation: &str, path: &Path) -> Result<Editing, Box<DocOutcome>> {
    let kept = || Box::new(refused(operation, problem(pack_kept(path))));
    if is_pack_name(path) {
        return Err(kept());
    }
    let (opened, stamp) =
        open_stamped(path).map_err(|e| Box::new(open_refusal(operation, &e, path)))?;
    if opened.packed_entries > 0 {
        return Err(kept());
    }
    Ok(Editing {
        migration: migration_with(&opened.notes, true),
        document: opened.document,
        stamp,
    })
}

fn read(operation: &str, path: &Path) -> Result<Document, Box<DocOutcome>> {
    open(path)
        .map(|opened| opened.document)
        .map_err(|e| Box::new(open_refusal(operation, &e, path)))
}

/// Save the document, unless the file changed since it was read.
fn save(
    path: &Path,
    operation: &str,
    document: &Document,
    stamp: Stamp,
) -> Result<(), Box<DocOutcome>> {
    save_if_unchanged(document, path, stamp)
        .map_err(|e| Box::new(refused(operation, problem(not_written(path, e)))))
}

fn target_of(item: &Override) -> &str {
    match item {
        Override::Set { setting, .. } | Override::Remove { setting } => setting,
    }
}

/// The recipes of a study as the ESI line makes it, or none when its checks find an error.
fn recipes_of(study: &Study, tool: &ToolIdentity) -> Option<StageRecipes> {
    if has_errors(&diagnose(study, &Dialect::EsiV2512.capabilities())) {
        return None;
    }
    let resolved = resolve(study).ok()?;
    Some(StageRecipes::from_resolved(&resolved, tool))
}

/// The stage keys the variant `id` (or `base`) makes now, as a status asks for them, or none when
/// its study cannot be built.
pub(crate) fn keys_of(document: &Document, id: &str) -> Option<StageKeys> {
    let study = effective_study(&document.study, &document.artifacts.scenarios, id).ok()?;
    let tool = tool_identity(Dialect::EsiV2512);
    recipes_of(&study, &tool).map(|recipes| recipes.keys())
}

/// One line of the list, and the effective study it was made from when that could be built.
struct Row {
    summary: ScenarioSummary,
    study: Option<Study>,
}

fn build_row(document: &Document, tool: &ToolIdentity, id: &str) -> Row {
    let scenarios = &document.artifacts.scenarios;
    let stored = scenarios.iter().find(|s| s.id == id);
    let study = effective_study(&document.study, scenarios, id).ok();
    let keys = study
        .as_ref()
        .and_then(|s| recipes_of(s, tool))
        .map(|recipes| recipes.keys());
    let kind = if id == BASE {
        "base"
    } else {
        let parent = stored
            .and_then(|s| s.parent.as_deref())
            .and_then(|p| effective_study(&document.study, scenarios, p).ok())
            .and_then(|p| serde_json::to_value(&p).ok())
            .unwrap_or(Value::Null);
        let overrides = stored.map(|s| s.overrides.as_slice()).unwrap_or_default();
        match classify(&parent, overrides) {
            SettingClass::Geometry => "geometry",
            SettingClass::Setting => "setting",
        }
    };
    let summary = ScenarioSummary {
        id: id.to_owned(),
        parent: stored.and_then(|s| s.parent.clone()),
        name: stored.and_then(|s| s.name.clone()),
        pinned: stored.is_some_and(|s| s.pinned),
        notes: stored.and_then(|s| s.notes.clone()),
        overrides: stored.map_or(0, |s| s.overrides.len()),
        kind: kind.to_owned(),
        sweep: stored
            .and_then(|s| s.origin.as_ref())
            .map(|origin| origin.sweep.clone()),
        mesh_key: keys.map(|k| k.mesh.to_string()),
        result_key: keys.map(|k| k.result.to_string()),
        mesh_shared_with: None,
    };
    Row { summary, study }
}

/// The base first, then every variant in the order the file lists them, each with the earlier one
/// that makes the same mesh.
fn rows(document: &Document, tool: &ToolIdentity) -> Vec<Row> {
    let listed = document
        .artifacts
        .scenarios
        .iter()
        .map(|s| s.id.as_str())
        .filter(|id| *id != BASE);
    let mut rows: Vec<Row> = std::iter::once(BASE)
        .chain(listed)
        .map(|id| build_row(document, tool, id))
        .collect();
    let mut first: HashMap<String, String> = HashMap::new();
    for r in &mut rows {
        let Some(key) = r.summary.mesh_key.clone() else {
            continue;
        };
        match first.get(&key) {
            Some(earlier) => r.summary.mesh_shared_with = Some(earlier.clone()),
            None => {
                first.insert(key, r.summary.id.clone());
            }
        }
    }
    rows
}

/// One variant in full: its summary, the changes it stores and the raw entries its study holds.
fn scenario_data(document: &Document, tool: &ToolIdentity, id: &str) -> Option<ScenarioData> {
    let found = rows(document, tool)
        .into_iter()
        .find(|r| r.summary.id == id)?;
    let overrides = document
        .artifacts
        .scenarios
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.overrides.clone())
        .unwrap_or_default();
    let raw_overrides = found
        .study
        .iter()
        .flat_map(|study| study.raw_overrides.iter())
        .map(|(label, raw)| RawView {
            id: label.clone(),
            backend: raw.backend.as_str().to_owned(),
            file: raw.file.clone(),
            entry: raw.entry.clone(),
            value: raw.value.clone(),
        })
        .collect();
    Some(ScenarioData {
        summary: found.summary,
        overrides,
        raw_overrides,
    })
}

/// `report`, naming the variant and carrying its full description.
fn described(report: Report, document: &Document, id: &str) -> Report {
    let report = report.with_artifact("scenario", id);
    match scenario_data(document, &tool_identity(Dialect::EsiV2512), id) {
        Some(data) => report.with_repro(Repro::Scenario(Box::new(data))),
        None => report,
    }
}

/// Create the variant `id`, or replace some of the changes of the one that exists.
///
/// A new variant descends from `parent` (the study itself when `None`) and holds exactly `sets`.
/// For an existing variant each change replaces the stored change of the same setting in place and
/// any other is added at the end; naming a parent other than its own is refused. `name`, when it
/// is not empty, becomes the name. The variant and every variant below it must still build their
/// study before anything is saved.
pub fn scenario_set(
    path: &Path,
    id: &str,
    parent: Option<&str>,
    sets: &[Override],
    name: Option<&str>,
) -> DocOutcome {
    set_inner(path, id, parent, sets, name).unwrap_or_else(|refusal| *refusal)
}

fn set_inner(
    path: &Path,
    id: &str,
    parent: Option<&str>,
    sets: &[Override],
    name: Option<&str>,
) -> Reply {
    let Editing {
        mut document,
        stamp,
        migration,
    } = edit_open(SET, path)?;
    if id == BASE {
        let reason = "is the study itself and holds no changes; use the name, pin and notes";
        let error = ScenarioError::BaseMisused {
            reason: reason.to_owned(),
        };
        return Err(refusal_outcome(SET, &error));
    }
    if !is_plain_name(id) {
        let error = ScenarioError::BadId {
            scenario: id.chars().take(40).collect(),
        };
        return Err(refusal_outcome(SET, &error));
    }
    let mut list = document.artifacts.scenarios.clone();
    let created = !list.iter().any(|s| s.id == id);
    if created {
        let parent = parent.unwrap_or(BASE);
        if parent != BASE && !list.iter().any(|s| s.id == parent) {
            return Err(unknown(SET, parent));
        }
        list.push(Scenario {
            id: id.to_owned(),
            parent: Some(parent.to_owned()),
            name: None,
            pinned: false,
            notes: None,
            overrides: Vec::new(),
            origin: None,
        });
    }
    let Some(record) = list.iter_mut().find(|s| s.id == id) else {
        return Err(unknown(SET, id));
    };
    let kept = record.parent.clone().unwrap_or_else(|| BASE.to_owned());
    if let Some(wanted) = parent
        && wanted != kept
    {
        let reason = format!("variant \"{id}\" descends from \"{kept}\" and keeps that parent");
        let found = ("CFD-SCN-002", Line::ScenarioRefused { reason });
        return Err(Box::new(refused(SET, problem(found))));
    }
    for item in sets {
        match record
            .overrides
            .iter()
            .position(|held| target_of(held) == target_of(item))
        {
            Some(held) => record.overrides[held] = item.clone(),
            None => record.overrides.push(item.clone()),
        }
    }
    if let Some(name) = name
        && !name.is_empty()
    {
        record.name = Some(name.to_owned());
    }
    if let Err(error) = check_scenarios(&list) {
        return Err(refusal_outcome(SET, &error));
    }
    document.artifacts.scenarios = list;
    let below: Vec<String> = document
        .artifacts
        .scenarios
        .iter()
        .filter(|s| {
            chain(&document.artifacts.scenarios, &s.id)
                .is_ok_and(|line| line.iter().any(|ancestor| ancestor.id == id))
        })
        .map(|s| s.id.clone())
        .collect();
    for scenario in &below {
        effective_of(SET, &document, Some(scenario))?;
    }
    save(path, SET, &document, stamp)?;

    let line = if created {
        Line::ScenarioCreated {
            scenario: id.to_owned(),
            parent: kept,
        }
    } else {
        Line::ScenarioUpdated {
            scenario: id.to_owned(),
        }
    };
    let report = Report::from_diagnostics(SET, Vec::new(), study_hash(&document.study))
        .with_migration(migration);
    Ok(DocOutcome {
        report: described(report, &document, id),
        line: Some(line),
    })
}

/// The variants of the document at `path`, the study itself first, each with its kind (`geometry`
/// or `setting`), its keys and the earlier variant that makes the same mesh. It passes whatever
/// the state of the variants: one whose study fails its checks or cannot be built has no keys.
pub fn scenario_list(path: &Path) -> DocOutcome {
    list_inner(path).unwrap_or_else(|refusal| *refusal)
}

fn list_inner(path: &Path) -> Reply {
    let document = read(LIST, path)?;
    let scenarios = rows(&document, &tool_identity(Dialect::EsiV2512))
        .into_iter()
        .map(|r| r.summary)
        .collect();
    let report = Report::from_diagnostics(LIST, Vec::new(), study_hash(&document.study))
        .with_repro(Repro::Scenarios(Box::new(ScenariosData { scenarios })));
    Ok(DocOutcome { report, line: None })
}

/// One variant in full: what it stores, the raw entries of its study, and its keys. The findings
/// of its study's own checks are shown as warnings. Nothing is written.
pub fn scenario_show(path: &Path, id: &str) -> DocOutcome {
    show_inner(path, id).unwrap_or_else(|refusal| *refusal)
}

fn show_inner(path: &Path, id: &str) -> Reply {
    let document = read(SHOW, path)?;
    let (study, _) = effective_of(SHOW, &document, Some(id))?;
    let found = diagnose(&study, &Dialect::EsiV2512.capabilities())
        .into_iter()
        .map(|mut finding| {
            finding.severity = Severity::Warning;
            finding
        })
        .collect();
    let report = Report::from_diagnostics(SHOW, found, study_hash(&document.study));
    Ok(DocOutcome {
        report: described(report, &document, id),
        line: None,
    })
}

/// The recipes of the variant `id`, or the refusal that tells why it has none.
fn recipes_for(
    document: &Document,
    id: &str,
    tool: &ToolIdentity,
) -> Result<StageRecipes, Box<DocOutcome>> {
    let (study, _) = effective_of(DIFF, document, Some(id))?;
    let found = diagnose(&study, &Dialect::EsiV2512.capabilities());
    if has_errors(&found) {
        return Err(Box::new(DocOutcome {
            report: Report::from_diagnostics(DIFF, found, None),
            line: None,
        }));
    }
    let resolved = resolve(&study).map_err(|e| {
        Box::new(refused(
            DIFF,
            Diagnostic::error("CFD-MODEL-003", "study", "", e.to_string()),
        ))
    })?;
    Ok(StageRecipes::from_resolved(&resolved, tool))
}

/// How the variant `a` differs from the variant `b`, in the words a status uses: the mesh changes,
/// then the result changes, with the mesh and the tools left out because they are not settings.
/// The ids may be `base`. Nothing is written.
pub fn scenario_diff(path: &Path, a: &str, b: &str) -> DocOutcome {
    diff_inner(path, a, b).unwrap_or_else(|refusal| *refusal)
}

fn diff_inner(path: &Path, a: &str, b: &str) -> Reply {
    let document = read(DIFF, path)?;
    let tool = tool_identity(Dialect::EsiV2512);
    let from = recipes_for(&document, a, &tool)?;
    let to = recipes_for(&document, b, &tool)?;
    let mut changes = diff_mesh(&from.mesh, &to.mesh);
    changes.extend(diff_result(&from.result, &to.result));
    changes.retain(|change| !matches!(change, Change::MeshChanged | Change::Tool { .. }));
    let line = if changes.is_empty() {
        Line::ScenarioSame {
            from: a.to_owned(),
            to: b.to_owned(),
        }
    } else {
        Line::ScenarioDiffers {
            from: a.to_owned(),
            to: b.to_owned(),
            reasons: reason_text(&changes),
        }
    };
    let data = DiffData {
        from: a.to_owned(),
        to: b.to_owned(),
        same: changes.is_empty(),
        sentence: say(&line),
        reasons: reason_parts(&changes),
        changes,
    };
    let report = Report::from_diagnostics(DIFF, Vec::new(), study_hash(&document.study))
        .with_repro(Repro::ScenarioDiff(Box::new(data)));
    Ok(DocOutcome {
        report,
        line: Some(line),
    })
}

/// Set or clear the name, the pin and the notes of a variant or of the study itself. Labelling
/// `base` creates its record at the front of the list. A name or notes with a control character
/// (a newline is allowed in notes), or over the length limits, are refused and nothing is saved.
pub fn scenario_label(path: &Path, id: &str, update: &LabelUpdate) -> DocOutcome {
    label_inner(path, id, update).unwrap_or_else(|refusal| *refusal)
}

fn label_inner(path: &Path, id: &str, update: &LabelUpdate) -> Reply {
    let Editing {
        mut document,
        stamp,
        migration,
    } = edit_open(LABEL, path)?;
    let mut list = document.artifacts.scenarios.clone();
    let at = match list.iter().position(|s| s.id == id) {
        Some(at) => at,
        None if id == BASE => {
            list.insert(
                0,
                Scenario {
                    id: BASE.to_owned(),
                    parent: None,
                    name: None,
                    pinned: false,
                    notes: None,
                    overrides: Vec::new(),
                    origin: None,
                },
            );
            0
        }
        None => return Err(unknown(LABEL, id)),
    };
    let record = &mut list[at];
    if let Some(name) = &update.name {
        record.name = (!name.is_empty()).then(|| name.clone());
    }
    if let Some(pinned) = update.pinned {
        record.pinned = pinned;
    }
    if let Some(notes) = &update.notes {
        record.notes = (!notes.is_empty()).then(|| notes.clone());
    }
    if let Err(error) = check_scenarios(&list) {
        return Err(refusal_outcome(LABEL, &error));
    }
    document.artifacts.scenarios = list;
    save(path, LABEL, &document, stamp)?;

    let report = Report::from_diagnostics(LABEL, Vec::new(), study_hash(&document.study))
        .with_migration(migration);
    Ok(DocOutcome {
        report: described(report, &document, id),
        line: Some(Line::ScenarioLabelled {
            scenario: id.to_owned(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::report::ExitClass;

    const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

    fn variant(id: &str, overrides: Vec<Override>) -> Scenario {
        Scenario {
            id: id.to_owned(),
            parent: Some(BASE.to_owned()),
            name: None,
            pinned: false,
            notes: None,
            overrides,
            origin: None,
        }
    }

    fn set(setting: &str, value: serde_json::Value) -> Override {
        Override::Set {
            setting: setting.to_owned(),
            value,
        }
    }

    fn document_with(scenarios: Vec<Scenario>) -> Document {
        let mut document = Document::new(Study::from_json(CAVITY).unwrap());
        document.artifacts.scenarios = scenarios;
        document
    }

    fn lid_speed(study: &Study) -> f64 {
        study.boundary_conditions["lid"]
            .velocity
            .as_ref()
            .unwrap()
            .value
    }

    /// The class, and the code and message of each finding, of the refusal for `scenario`.
    fn refusal_of(document: &Document, scenario: &str) -> (ExitClass, Vec<(String, String)>) {
        let outcome = *effective_of("status", document, Some(scenario)).unwrap_err();
        let found = outcome
            .report
            .diagnostics
            .iter()
            .map(|d| (d.code.clone(), d.message.clone()))
            .collect();
        (outcome.report.class(), found)
    }

    #[test]
    fn the_study_itself_has_no_tag() {
        let document = document_with(Vec::new());
        for scenario in [None, Some("base")] {
            let (study, tag) = effective_of("status", &document, scenario).unwrap();
            assert_eq!(study, document.study);
            assert_eq!(tag, None);
        }
    }

    #[test]
    fn a_variant_gives_its_effective_study_and_its_tag() {
        let document = document_with(vec![variant(
            "fast-lid",
            vec![set("/boundary_conditions/lid/velocity/value", json!(0.2))],
        )]);
        let (study, tag) = effective_of("status", &document, Some("fast-lid")).unwrap();
        assert_eq!(lid_speed(&study), 0.2);
        assert_eq!(tag.as_deref(), Some("fast-lid"));
        assert_ne!(lid_speed(&document.study), 0.2);
    }

    #[test]
    fn an_unknown_variant_is_cfd_scn_001() {
        let document = document_with(Vec::new());
        let (class, found) = refusal_of(&document, "fast-lid");
        assert_eq!(class, ExitClass::StudyInvalid);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "CFD-SCN-001");
        assert!(found[0].1.contains("fast-lid"), "{}", found[0].1);
    }

    #[test]
    fn an_override_that_does_not_apply_is_cfd_scn_003() {
        let document = document_with(vec![
            variant(
                "lost",
                vec![set("/boundary_conditions/nope/kind", json!("NoSlipWall"))],
            ),
            variant("dense", vec![set("/fluid/density", json!(1.2))]),
            variant("wrong-type", vec![set("/mesh/cells_x", json!("many"))]),
        ]);

        let (class, found) = refusal_of(&document, "lost");
        assert_eq!(class, ExitClass::StudyInvalid);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "CFD-SCN-003");
        assert!(found[0].1.contains("lost"), "{}", found[0].1);
        assert!(
            found[0].1.contains("/boundary_conditions/nope/kind"),
            "{}",
            found[0].1
        );

        for name in ["dense", "wrong-type"] {
            let (class, found) = refusal_of(&document, name);
            assert_eq!(class, ExitClass::StudyInvalid);
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].0, "CFD-SCN-003");
            assert!(found[0].1.contains(name), "{}", found[0].1);
            assert!(
                found[0].1.contains("change to the study in"),
                "{}",
                found[0].1
            );
        }
    }
}
