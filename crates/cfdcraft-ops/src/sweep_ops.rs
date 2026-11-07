//! Parameter sweeps: planning the points a sweep makes, storing a sweep in a document as variants,
//! and checking every point before anything is solved. Every function takes the path of the
//! document and returns a `DocOutcome`.
//!
//! Finding codes of this family: 001 a sweep has more points than the limit; 002 a sweep is
//! refused; 003 a point fails the checks made before solving (one finding per point); 004 there is
//! no such sweep. 001, 002 and 004 make exit class 2, 003 makes class 1. A variant that is not
//! listed is `CFD-SCN-001` and a refused list of variants `CFD-SCN-002`. A report names sweeps,
//! points and variants by id and never the path of the document, except in a failed write.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use cfdcraft_backend_esi::Dialect;
use cfdcraft_doc::{Document, Stamp, open_stamped, save_if_unchanged};
use cfdcraft_keys::{StageRecipes, ToolIdentity};
use cfdcraft_model::{Diagnostic, MigrationNote, Severity};
use cfdcraft_scenarios::{
    BASE, MAX_NAME, MAX_SCENARIOS, Origin, Override, Scenario, check_scenarios,
};
use cfdcraft_sweep::{Point, Sweep, SweepError, points};
use cfdcraft_text::Line;

use crate::documents::{
    DocOutcome, Finding, is_pack_name, migration_with, not_written, pack_kept, problem, refused,
    study_hash,
};
use crate::jobs::open_refusal;
use crate::keys::tool_identity;
use crate::prepare::prepare;
use crate::report::{ExitClass, Report};
use crate::repro::{PointCheck, PreflightData, Repro, SweepData};
use crate::scenario_ops::effective_of;

const PLAN: &str = "sweep-plan";
const CREATE: &str = "sweep-create";
const PREFLIGHT: &str = "sweep-preflight";

type Reply = Result<DocOutcome, Box<DocOutcome>>;

/// Why a point has no variant to check.
const MISSING: &str = "the variant is missing from the file";

/// An id given by a person, shortened and with control characters replaced, for a message.
fn shown_id(id: &str) -> String {
    id.chars()
        .take(40)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn fail(operation: &str, found: Finding) -> Box<DocOutcome> {
    Box::new(refused(operation, problem(found)))
}

fn refused_sweep(operation: &str, sweep: &str, reason: String) -> Box<DocOutcome> {
    let sweep = shown_id(sweep);
    fail(
        operation,
        ("CFD-SWP-002", Line::SweepRefused { sweep, reason }),
    )
}

/// The finding that tells a person why a sweep cannot be expanded.
fn sweep_refusal(sweep: &str, error: &SweepError) -> Finding {
    let sweep = shown_id(sweep);
    match error {
        SweepError::TooManyPoints { count, max } => (
            "CFD-SWP-001",
            Line::SweepTooLarge {
                sweep,
                points: count.to_string(),
                max: max.to_string(),
            },
        ),
        other => (
            "CFD-SWP-002",
            Line::SweepRefused {
                sweep,
                reason: other.to_string(),
            },
        ),
    }
}

/// The document being used, the stamp of the bytes it was read from and the note for an older
/// file that a save brings forward.
struct Editing {
    document: Document,
    stamp: Stamp,
    migration: Option<MigrationNote>,
}

/// Open the document at `path`. A pack is refused, because saving it back would drop the results
/// it carries and a sweep is never made on one.
fn open_checked(operation: &str, path: &Path) -> Result<Editing, Box<DocOutcome>> {
    let kept = || fail(operation, pack_kept(path));
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

/// The points of `sweep` when the document can take it: the sweep is sound and within the limits,
/// its base is the study or a listed variant, neither the sweep nor a point is listed already, and
/// the file would not hold more variants than it may.
fn expand(
    operation: &str,
    document: &Document,
    sweep: &Sweep,
) -> Result<Vec<Point>, Box<DocOutcome>> {
    let made = points(sweep).map_err(|e| fail(operation, sweep_refusal(&sweep.id, &e)))?;
    let listed: HashSet<&str> = document
        .artifacts
        .scenarios
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    if sweep.base != BASE && !listed.contains(sweep.base.as_str()) {
        let scenario = shown_id(&sweep.base);
        return Err(fail(
            operation,
            ("CFD-SCN-001", Line::ScenarioUnknown { scenario }),
        ));
    }
    if document.artifacts.sweeps.iter().any(|s| s.id == sweep.id) {
        let reason = format!("the file already holds a sweep called \"{}\"", sweep.id);
        return Err(refused_sweep(operation, &sweep.id, reason));
    }
    if let Some(point) = made.iter().find(|p| listed.contains(p.id.as_str())) {
        let reason = format!("the file already holds a variant called \"{}\"", point.id);
        return Err(refused_sweep(operation, &sweep.id, reason));
    }
    let total = listed.len() + made.len();
    if total > MAX_SCENARIOS {
        let reason = format!(
            "the file would hold {total} variants, and at most {MAX_SCENARIOS} are allowed"
        );
        return Err(refused_sweep(operation, &sweep.id, reason));
    }
    Ok(made)
}

/// The variant a point of the sweep is stored as: it descends from the base of the sweep and holds
/// one change for each setting the point sets.
fn variant_of(sweep: &Sweep, point: &Point) -> Scenario {
    Scenario {
        id: point.id.clone(),
        parent: Some(sweep.base.clone()),
        name: (point.label.chars().count() <= MAX_NAME).then(|| point.label.clone()),
        pinned: false,
        notes: None,
        overrides: point
            .assignments
            .iter()
            .map(|a| Override::Set {
                setting: a.setting.clone(),
                value: a.value.clone(),
            })
            .collect(),
        origin: Some(Origin {
            sweep: sweep.id.clone(),
            index: point.index,
        }),
    }
}

fn sweep_report(
    operation: &str,
    document: &Document,
    stored: bool,
    sweep: &Sweep,
    made: Vec<Point>,
) -> Report {
    let data = SweepData {
        sweep: sweep.id.clone(),
        base: sweep.base.clone(),
        stored,
        points: made,
    };
    Report::from_diagnostics(operation, Vec::new(), study_hash(&document.study))
        .with_artifact("sweep", sweep.id.clone())
        .with_repro(Repro::Sweep(Box::new(data)))
}

/// The points `sweep` would make, in order, each with its label and the settings it changes. The
/// document is read and nothing is stored or built. The sweep is refused for the same reasons as
/// [`sweep_create`] refuses it.
pub fn sweep_plan(path: &Path, sweep: &Sweep) -> DocOutcome {
    plan_inner(path, sweep).unwrap_or_else(|refusal| *refusal)
}

fn plan_inner(path: &Path, sweep: &Sweep) -> Reply {
    let Editing { document, .. } = open_checked(PLAN, path)?;
    let made = expand(PLAN, &document, sweep)?;
    Ok(DocOutcome {
        report: sweep_report(PLAN, &document, false, sweep, made),
        line: None,
    })
}

/// Store `sweep` in the document and one variant for each of its points. A variant is named like
/// its point and holds only the settings the point sets; it is solved, diffed and labelled like
/// any other. Nothing is applied or built here, so a point whose settings do not fit the study is
/// found by [`sweep_preflight`]. Either everything is stored or nothing is.
pub fn sweep_create(path: &Path, sweep: &Sweep) -> DocOutcome {
    create_inner(path, sweep).unwrap_or_else(|refusal| *refusal)
}

fn create_inner(path: &Path, sweep: &Sweep) -> Reply {
    let Editing {
        mut document,
        stamp,
        migration,
    } = open_checked(CREATE, path)?;
    let made = expand(CREATE, &document, sweep)?;
    let mut list = document.artifacts.scenarios.clone();
    list.extend(made.iter().map(|point| variant_of(sweep, point)));
    if let Err(error) = check_scenarios(&list) {
        let reason = error.to_string();
        return Err(fail(
            CREATE,
            ("CFD-SCN-002", Line::ScenarioRefused { reason }),
        ));
    }
    document.artifacts.scenarios = list;
    document.artifacts.sweeps.push(sweep.clone());
    save_if_unchanged(&document, path, stamp).map_err(|e| fail(CREATE, not_written(path, e)))?;

    let line = Line::SweepCreated {
        sweep: sweep.id.clone(),
        points: made.len().to_string(),
    };
    Ok(DocOutcome {
        report: sweep_report(CREATE, &document, true, sweep, made).with_migration(migration),
        line: Some(line),
    })
}

/// The messages of the errors among `found`, or one general message when there are none.
fn messages(found: &[Diagnostic]) -> Vec<String> {
    let reasons: Vec<String> = found
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    if reasons.is_empty() {
        vec!["the case could not be prepared".to_owned()]
    } else {
        reasons
    }
}

/// A point that passed: its keys, and the warnings its study gave.
struct Passed {
    mesh: String,
    result: String,
    warnings: Vec<Diagnostic>,
}

/// Run the gates a solve runs for one point, short of the solver: its variant must be listed,
/// its changes must apply to the study, and the study must pass the checks, compile to a case
/// that passes the safety scan, and read back. The reasons are the messages of the findings.
fn check_point(
    document: &Document,
    tool: &ToolIdentity,
    point: &Point,
) -> Result<Passed, Vec<String>> {
    if !document
        .artifacts
        .scenarios
        .iter()
        .any(|s| s.id == point.id)
    {
        return Err(vec![MISSING.to_owned()]);
    }
    let (study, _) = effective_of(PREFLIGHT, document, Some(point.id.as_str()))
        .map_err(|outcome| messages(&outcome.report.diagnostics))?;
    let mut study_json = serde_json::to_string_pretty(&study).map_err(|e| vec![e.to_string()])?;
    study_json.push('\n');
    let prepared =
        prepare(&study_json, Dialect::EsiV2512).map_err(|report| messages(&report.diagnostics))?;
    let keys = StageRecipes::from_resolved(&prepared.resolved, tool).keys();
    Ok(Passed {
        mesh: keys.mesh.to_string(),
        result: keys.result.to_string(),
        warnings: prepared.warnings,
    })
}

/// Check every point of the stored sweep `sweep_id` before anything is solved: whether its variant
/// is there, applies to the study, passes the study's checks and makes a case that passes the
/// safety scan and the read-back check, with the mesh key and the result key it would have. Each
/// failing point is one `CFD-SWP-003` finding (exit class 1) with its reasons. The report also
/// tells how many different meshes the passing points need, and which earlier point each shares
/// its mesh with. Nothing is written, run or started; the warnings of the passing points are kept
/// (a warning that several points give is listed once).
pub fn sweep_preflight(path: &Path, sweep_id: &str) -> DocOutcome {
    preflight_inner(path, sweep_id).unwrap_or_else(|refusal| *refusal)
}

fn preflight_inner(path: &Path, sweep_id: &str) -> Reply {
    let Editing { document, .. } = open_checked(PREFLIGHT, path)?;
    let Some(sweep) = document.artifacts.sweeps.iter().find(|s| s.id == sweep_id) else {
        let sweep = shown_id(sweep_id);
        return Err(fail(
            PREFLIGHT,
            ("CFD-SWP-004", Line::SweepUnknown { sweep }),
        ));
    };
    let all = points(sweep).map_err(|e| fail(PREFLIGHT, sweep_refusal(sweep_id, &e)))?;
    let tool = tool_identity(Dialect::EsiV2512);

    let mut checks: Vec<PointCheck> = Vec::with_capacity(all.len());
    let mut findings: Vec<Diagnostic> = Vec::new();
    let mut warnings: Vec<Diagnostic> = Vec::new();
    // The first point that made each mesh.
    let mut first_mesh: HashMap<String, String> = HashMap::new();
    for point in &all {
        let check = match check_point(&document, &tool, point) {
            Err(reasons) => {
                let line = Line::PreflightPointFailed {
                    point: point.id.clone(),
                    reason: reasons.join("; "),
                };
                findings.push(problem(("CFD-SWP-003", line)));
                PointCheck {
                    id: point.id.clone(),
                    label: point.label.clone(),
                    ok: false,
                    reasons,
                    mesh_key: None,
                    result_key: None,
                    mesh_shared_with: None,
                }
            }
            Ok(passed) => {
                let shared = first_mesh.get(&passed.mesh).cloned();
                if shared.is_none() {
                    first_mesh.insert(passed.mesh.clone(), point.id.clone());
                }
                for warning in passed.warnings {
                    if !warnings.contains(&warning) {
                        warnings.push(warning);
                    }
                }
                PointCheck {
                    id: point.id.clone(),
                    label: point.label.clone(),
                    ok: true,
                    reasons: Vec::new(),
                    mesh_key: Some(passed.mesh),
                    result_key: Some(passed.result),
                    mesh_shared_with: shared,
                }
            }
        };
        checks.push(check);
    }

    let failed = checks.iter().filter(|c| !c.ok).count();
    let line = if failed == 0 {
        Line::PreflightPassed {
            sweep: sweep.id.clone(),
            points: checks.len().to_string(),
            meshes: first_mesh.len().to_string(),
        }
    } else {
        Line::PreflightFailed {
            sweep: sweep.id.clone(),
            failed: failed.to_string(),
            points: checks.len().to_string(),
        }
    };
    findings.extend(warnings);
    let mut report = Report::from_diagnostics(PREFLIGHT, findings, study_hash(&document.study))
        .with_artifact("sweep", sweep.id.clone())
        .with_repro(Repro::Preflight(Box::new(PreflightData {
            sweep: sweep.id.clone(),
            points: checks,
            distinct_meshes: first_mesh.len(),
            failed,
        })));
    if failed > 0 {
        report.set_class(ExitClass::GateFailed);
    }
    Ok(DocOutcome {
        report,
        line: Some(line),
    })
}
