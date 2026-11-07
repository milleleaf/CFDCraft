//! A server-side workspace: studies, compiled cases, exports, runs, documents and packs addressed by
//! name, never by path. A name is `[a-z0-9][a-z0-9-]{0,39}`, so no operation can reach outside the workspace.
//!
//! Nothing here takes a path, a host or a key from the caller. Whoever hosts the workspace decides
//! where it lives; the callers only supply names, modes and a study.

use crate::{
    cases::{cli_problem, compile_study, export_study},
    host_runner::finish_job,
    jobs::{JobAction, JobOptions, operation_of, plan_scenario_job_with},
    prepare::prepare,
    registry::{ResultOutcome, RunRegistry, StartOutcome, Status},
    report::{ExitClass, Report},
    run::{RunMode, RunRequest},
    scenario_ops::LabelUpdate,
};
use cfdcraft_backend_esi::Dialect;
use cfdcraft_compare::Tolerance;
use cfdcraft_keys::Stage;
use cfdcraft_model::{Diagnostic, Study};
use cfdcraft_scenarios::Override;
use cfdcraft_sweep::Sweep;
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const STUDIES: &str = "studies";
const COMPILED: &str = "compiled";
const EXPORTS: &str = "exports";
const RUNS: &str = "runs";
const DOCUMENTS: &str = "documents";
const PACKS: &str = "packs";

/// The most documents one plan of an update may name.
const MAX_PLAN_DOCUMENTS: usize = 16;

/// The directories under a workspace root, each a real directory.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

fn ws_error(code: &str, message: &str) -> Diagnostic {
    Diagnostic::error(code, "study name", "", message)
}

fn failed(operation: &str, found: Diagnostic) -> Report {
    Report::from_diagnostics(operation, vec![found], None)
}

impl Workspace {
    /// Open the workspace at `root`, creating it and its six directories if they are absent.
    /// A directory that is a symbolic link, or not a directory, is refused.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref();
        std::fs::create_dir_all(root)
            .map_err(|e| format!("the workspace cannot be opened: {e}"))?;
        let root = root
            .canonicalize()
            .map_err(|e| format!("the workspace cannot be opened: {e}"))?;
        for sub in [STUDIES, COMPILED, EXPORTS, RUNS, DOCUMENTS, PACKS] {
            let path = root.join(sub);
            match std::fs::symlink_metadata(&path) {
                Ok(m) if m.is_dir() => {}
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(format!("the workspace directory {sub} is a symbolic link"));
                }
                Ok(_) => return Err(format!("the workspace entry {sub} is not a directory")),
                Err(e) if e.kind() == ErrorKind::NotFound => std::fs::create_dir(&path)
                    .map_err(|e| format!("the workspace directory {sub} cannot be made: {e}"))?,
                Err(e) => return Err(format!("the workspace directory {sub} cannot be used: {e}")),
            }
        }
        Ok(Self { root })
    }

    /// Whether `name` is a plain name: a lower-case letter or digit, then up to 39 more lower-case
    /// letters, digits or hyphens. The rule is [`cfdcraft_runs::is_plain_name`], which run ids,
    /// link ids and the verbs that name a run follow too.
    pub fn valid_name(name: &str) -> bool {
        cfdcraft_runs::is_plain_name(name)
    }

    pub(crate) fn dir(&self, sub: &str) -> PathBuf {
        self.root.join(sub)
    }

    fn study_path(&self, name: &str) -> PathBuf {
        self.dir(STUDIES).join(format!("{name}.json"))
    }

    fn bad_name(operation: &str) -> Report {
        failed(
            operation,
            ws_error(
                "CFD-WS-001",
                "A name is 1 to 40 characters: lower-case letters, digits and hyphens, starting with a letter or digit.",
            ),
        )
    }

    /// The stored study's text, or the finished report that says why there is none.
    // The report is the error on purpose: a failed step hands back the finished result.
    #[allow(clippy::result_large_err)]
    pub(crate) fn load_study(&self, operation: &str, name: &str) -> Result<String, Report> {
        if !Self::valid_name(name) {
            return Err(Self::bad_name(operation));
        }
        std::fs::read_to_string(self.study_path(name)).map_err(|e| {
            let found = if e.kind() == ErrorKind::NotFound {
                ws_error(
                    "CFD-WS-002",
                    "There is no study of that name in the workspace.",
                )
            } else {
                cli_problem("The stored study cannot be read.".into())
            };
            failed(operation, found)
        })
    }

    /// Store a study under `name` if it passes every gate; a schema 0 study is stored migrated.
    /// The report is the one `validate_study` would give.
    pub fn create_study(&self, name: &str, study: &serde_json::Value) -> Report {
        const OP: &str = "create_study";
        if !Self::valid_name(name) {
            return Self::bad_name(OP);
        }
        let path = self.study_path(name);
        if std::fs::symlink_metadata(&path).is_ok() {
            return failed(
                OP,
                ws_error(
                    "CFD-WS-003",
                    "A study of that name already exists in the workspace.",
                ),
            );
        }
        let text = study.to_string();
        let prepared = match prepare(&text, Dialect::EsiV2512) {
            Ok(p) => p,
            Err(report) => return report.for_operation(OP),
        };
        let report = report_of(OP, &prepared);
        // Prepare accepted the text, so it parses; store the parsed, migrated form.
        let Ok((parsed, _)) = Study::from_json_with_note(&text) else {
            return report;
        };
        match store(&path, &parsed) {
            Ok(()) => report,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => failed(
                OP,
                ws_error(
                    "CFD-WS-003",
                    "A study of that name already exists in the workspace.",
                ),
            ),
            Err(_) => failed(OP, cli_problem("The study could not be stored.".into())),
        }
    }

    /// Run the gates over a stored study and report; nothing is written.
    pub fn validate_study(&self, name: &str) -> Report {
        const OP: &str = "validate_study";
        let text = match self.load_study(OP, name) {
            Ok(t) => t,
            Err(report) => return report,
        };
        match prepare(&text, Dialect::EsiV2512) {
            Ok(p) => report_of(OP, &p),
            Err(report) => report.for_operation(OP),
        }
    }
}

impl Workspace {
    /// Compile a stored study into `compiled/<name>`, replacing what was there.
    pub fn compile(&self, name: &str) -> Report {
        self.write_output("compile", COMPILED, name, compile_study)
    }

    /// Export a stored study into `exports/<name>`: the case plus `Allrun` and `README.md`.
    pub fn export(&self, name: &str) -> Report {
        self.write_output("export", EXPORTS, name, export_study)
    }

    /// The output directory belongs to the server: the case is written into a fresh sibling and
    /// swapped in only when every gate passed, so a failure keeps the previous output and a
    /// success keeps no stale file.
    fn write_output(
        &self,
        operation: &str,
        sub: &str,
        name: &str,
        write: fn(&str, &Path) -> Report,
    ) -> Report {
        let text = match self.load_study(operation, name) {
            Ok(t) => t,
            Err(report) => return report,
        };
        let parent = self.dir(sub);
        let fresh = temp_in(&parent);
        let report = write(&text, &fresh);
        let target = parent.join(name);
        let swapped = report.class() == ExitClass::Passed && swap_in(&parent, &fresh, &target);
        if swapped {
            return report;
        }
        let _ = std::fs::remove_dir_all(&fresh);
        if report.class() == ExitClass::Passed {
            return failed(
                operation,
                cli_problem("The output could not be put in place.".into()),
            );
        }
        report
    }
}

impl Workspace {
    /// Save a stored study as the document `documents/<name>.cfdcraft`. An existing document is
    /// never replaced.
    pub fn save_document(&self, name: &str) -> Report {
        const OP: &str = "save_document";
        self.named(OP, name, || match self.load_study(OP, name) {
            Ok(text) => {
                crate::documents::save_document(&text, &self.document_file(name), false).report
            }
            Err(report) => report,
        })
    }

    /// Open `documents/<name>.cfdcraft` and report on it. Nothing is written. A link in the
    /// document that leads outside the workspace is not followed.
    pub fn open_document(&self, name: &str) -> Report {
        self.named("open_document", name, || {
            crate::documents::open_document_in(&self.document_file(name), Some(&self.root)).report
        })
    }

    /// Pack `documents/<name>.cfdcraft` and its stored results into `packs/<name>.cfdpack`. An
    /// existing pack is never replaced.
    pub fn pack_document(&self, name: &str) -> Report {
        self.named("pack_document", name, || {
            crate::documents::pack_document(&self.document_file(name), &self.pack_file(name)).report
        })
    }

    /// Unpack `packs/<name>.cfdpack`, which the operator placed there, into the document
    /// `documents/<name>.cfdcraft`. An existing document is never replaced.
    pub fn unpack_document(&self, name: &str) -> Report {
        self.named("unpack_document", name, || {
            crate::documents::unpack_document(&self.pack_file(name), &self.dir(DOCUMENTS)).report
        })
    }

    fn document_file(&self, name: &str) -> PathBuf {
        self.dir(DOCUMENTS).join(format!("{name}.cfdcraft"))
    }

    /// Whether the mesh and the result of `documents/<name>.cfdcraft` are up to date for the study
    /// it holds, and for each one that is not, why. Nothing is written and nothing runs.
    pub fn document_status(&self, name: &str) -> Report {
        self.named("document_status", name, || {
            crate::inspect::document_status(&self.document_file(name)).report
        })
    }

    /// [`Workspace::document_status`] for the variant `scenario` of the document; `None` and `base`
    /// mean the study itself. Nothing is written and nothing runs.
    pub fn scenario_status(&self, name: &str, scenario: Option<&str>) -> Report {
        self.named("document_status", name, || {
            crate::inspect::scenario_status(&self.document_file(name), scenario).report
        })
    }

    /// The variants of the document, the study itself first, with their kinds and keys. Nothing is
    /// written.
    pub fn scenario_list(&self, name: &str) -> Report {
        self.named("scenario_list", name, || {
            crate::scenario_ops::scenario_list(&self.document_file(name)).report
        })
    }

    /// One variant in full: what it stores and the findings of its study. Nothing is written.
    pub fn scenario_show(&self, name: &str, scenario: &str) -> Report {
        self.named("scenario_show", name, || {
            crate::scenario_ops::scenario_show(&self.document_file(name), scenario).report
        })
    }

    /// How the variant `b` differs from the variant `a`. Either may be `base`. Nothing is written.
    pub fn scenario_diff(&self, name: &str, a: &str, b: &str) -> Report {
        self.named("scenario_diff", name, || {
            crate::scenario_ops::scenario_diff(&self.document_file(name), a, b).report
        })
    }

    /// Create the variant `scenario` of the document, or replace some of the changes of the one that
    /// exists. A new variant descends from `parent` (the study itself when `None`). A non-empty
    /// `title` becomes its name. Only the named document changes.
    pub fn scenario_set(
        &self,
        name: &str,
        scenario: &str,
        parent: Option<&str>,
        overrides: &[Override],
        title: Option<&str>,
    ) -> Report {
        self.named("scenario_set", name, || {
            crate::scenario_ops::scenario_set(
                &self.document_file(name),
                scenario,
                parent,
                overrides,
                title,
            )
            .report
        })
    }

    /// Set or clear the title, the pin and the notes of a variant or of the study (`base`). An empty
    /// title or notes clears it. Only the named document changes.
    pub fn scenario_label(&self, name: &str, scenario: &str, update: &LabelUpdate) -> Report {
        self.named("scenario_label", name, || {
            crate::scenario_ops::scenario_label(&self.document_file(name), scenario, update).report
        })
    }

    /// The points `sweep` would make for the document, each with its label and the settings it
    /// changes. Nothing is stored and nothing is started.
    pub fn sweep_plan(&self, name: &str, sweep: &Sweep) -> Report {
        self.named("sweep_plan", name, || {
            crate::sweep_ops::sweep_plan(&self.document_file(name), sweep).report
        })
    }

    /// Store `sweep` in the document and one variant for each of its points. Only the named
    /// document changes, and nothing is started.
    pub fn sweep_create(&self, name: &str, sweep: &Sweep) -> Report {
        self.named("sweep_create", name, || {
            crate::sweep_ops::sweep_create(&self.document_file(name), sweep).report
        })
    }

    /// Check every point of the stored sweep `sweep_id` without solving anything. A point that
    /// fails is exit class 1 with its reasons. Nothing is written or started.
    pub fn sweep_preflight(&self, name: &str, sweep_id: &str) -> Report {
        self.named("sweep_preflight", name, || {
            crate::sweep_ops::sweep_preflight(&self.document_file(name), sweep_id).report
        })
    }

    /// What each variant of the document holds in the store, in bytes. Nothing is written.
    pub fn disk_use(&self, name: &str) -> Report {
        self.named("disk_use", name, || {
            crate::disk_ops::disk_use(&self.document_file(name)).report
        })
    }

    /// Plan the removal of stored results, in `scope` (the named variants) or in all variants. It
    /// is a dry run unless `apply` is set. Every run record and its key numbers stay.
    pub fn prune(&self, name: &str, scope: Option<&[String]>, apply: bool) -> Report {
        self.named("prune", name, || {
            crate::disk_ops::prune(&self.document_file(name), scope, apply).report
        })
    }

    /// The runs recorded in the document, oldest first.
    pub fn list_runs(&self, name: &str) -> Report {
        self.named("list_runs", name, || {
            crate::inspect::list_runs(&self.document_file(name)).report
        })
    }

    /// The checks and key numbers of the run called `run`, or of the newest run that recorded a
    /// result and is not a rerun. (`run_result` is the registry's answer for a started run.)
    pub fn document_result(&self, name: &str, run: Option<&str>) -> Report {
        self.named("get_run_result", name, || {
            crate::inspect::run_result(&self.document_file(name), run).report
        })
    }

    /// What the run called `run`, or the newest run, was made from.
    pub fn document_provenance(&self, name: &str, run: Option<&str>) -> Report {
        self.named("get_provenance", name, || {
            crate::inspect::run_provenance(&self.document_file(name), run).report
        })
    }

    /// Compare the key numbers of run `b` with those of run `a`. A part of the tolerance that is
    /// not given is the tight default for a rerun in the same image; a part that is negative or not
    /// a finite number is refused as an ordinary report.
    pub fn compare_runs(
        &self,
        name: &str,
        a: &str,
        b: &str,
        abs_tolerance: Option<f64>,
        rel_tolerance: Option<f64>,
    ) -> Report {
        const OP: &str = "compare_runs";
        if !Self::valid_name(name) {
            return Self::bad_name(OP);
        }
        let default = Tolerance::same_image();
        let tolerance = match Tolerance::new(
            abs_tolerance.unwrap_or(default.abs),
            rel_tolerance.unwrap_or(default.rel),
        ) {
            Ok(t) => t,
            Err(e) => return failed(OP, cli_problem(e.to_string())),
        };
        self.named(OP, name, || {
            crate::inspect::compare_runs(&self.document_file(name), a, b, tolerance).report
        })
    }

    /// What an update of the named documents would do: the items that are out of date and the
    /// order they would be made in. It refuses what an update would refuse and never writes or runs
    /// anything. `items` chooses some of the waiting items and `order` sets their order; both are
    /// written `name:mesh` or `name:result`.
    pub fn plan_update(
        &self,
        names: &[String],
        items: Option<&[String]>,
        order: Option<&[String]>,
    ) -> Report {
        const OP: &str = "plan_update";
        if names.len() > MAX_PLAN_DOCUMENTS || !names.iter().all(|n| Self::valid_name(n)) {
            return Self::bad_name(OP);
        }
        let paths: Vec<PathBuf> = names.iter().map(|n| self.document_file(n)).collect();
        let report = crate::update::plan_update_report(&paths, items, order).report;
        self.within(report.for_operation(OP))
    }

    fn pack_file(&self, name: &str) -> PathBuf {
        self.dir(PACKS).join(format!("{name}.cfdpack"))
    }

    /// Run a document operation for a valid name and make its report workspace-relative.
    fn named(&self, operation: &str, name: &str, run: impl FnOnce() -> Report) -> Report {
        let report = if Self::valid_name(name) {
            run().for_operation(operation)
        } else {
            Self::bad_name(operation)
        };
        self.within(report)
    }

    /// Show every path in a report relative to the workspace root, so a finding never names the
    /// server's directories: the root and a separator are removed, a bare root becomes `.`.
    fn within(&self, mut report: Report) -> Report {
        let root = self.root.display().to_string();
        let relative = |text: &str| relative_to(&root, text);
        for found in &mut report.diagnostics {
            found.message = relative(&found.message);
            found.setting = relative(&found.setting);
            found.hint = found.hint.as_deref().map(relative);
            if let Some(at) = &mut found.location {
                at.file = relative(&at.file);
            }
        }
        for artifact in &mut report.artifacts {
            artifact.name = relative(&artifact.name);
        }
        report
    }
}

/// Whether a character can belong to a name inside a path, so that a root met right after it is
/// the tail of some other path and not the start of one.
fn in_a_path_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '+')
}

/// `text` with the folder `root` taken out of every path that starts with it: the root and the
/// separator after it are removed, a bare root becomes `.`. A longer name that merely begins with
/// the root's text (`/srv/ws2` against `/srv/ws`) is left as it is, and so is a path that merely
/// contains the root's text further along (`/data/srv/ws/x` against `/srv/ws`).
fn relative_to(root: &str, text: &str) -> String {
    if root.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(root) {
        out.push_str(&rest[..at]);
        let after = &rest[at + root.len()..];
        if out.chars().next_back().is_some_and(in_a_path_name) {
            out.push_str(root);
            rest = after;
            continue;
        }
        let mut chars = after.chars();
        let (first, second) = (chars.next(), chars.next());
        match first {
            Some(c) if c == '/' || c == std::path::MAIN_SEPARATOR => {
                rest = &after[c.len_utf8()..];
            }
            Some(c) if c.is_alphanumeric() || c == '-' || c == '_' => {
                out.push_str(root);
                rest = after;
            }
            Some('.') if second.is_some_and(char::is_alphanumeric) => {
                out.push_str(root);
                rest = after;
            }
            _ => {
                out.push('.');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// What the host process decides about where runs happen. Built once at start-up and never a
/// parameter of any workspace operation.
#[derive(Debug, Clone)]
pub struct ServerSettings {
    key: PathBuf,
    hosts: PathBuf,
    profile: String,
    socket_dir: PathBuf,
}

impl ServerSettings {
    /// The key file must exist; the message does not say where it was looked for.
    pub fn new(
        key: impl Into<PathBuf>,
        hosts: impl Into<PathBuf>,
        profile: &str,
        socket_dir: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let key = key.into();
        if !key.is_file() {
            return Err("the key file for the compute host is missing".into());
        }
        Ok(Self {
            key,
            hosts: hosts.into(),
            profile: profile.to_owned(),
            socket_dir: socket_dir.into(),
        })
    }
}

/// What a job started on a stored document does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocJobKind {
    /// Make whatever mesh and result is out of date.
    Solve,
    /// Run the settings a recorded run was made from and compare with that run.
    Rerun {
        /// The run to repeat.
        run: String,
    },
    /// Make this one stage, which must be out of date.
    Item(Stage),
    /// Solve the variant `scenario` (the study itself when `None` or `base`). With `remake_pruned`
    /// a result that was pruned on purpose counts as out of date and is made again.
    SolveScenario {
        scenario: Option<String>,
        remake_pruned: bool,
    },
}

/// A run id as the registry hands them out: letters, digits and hyphens.
fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

impl Workspace {
    /// Start a run of a stored study and return at once; the caller polls. With `from_export` the
    /// stored export is what runs, after it has been checked byte for byte against the study.
    /// Busy and refused answers pass through from the registry unchanged.
    pub fn start_run(
        &self,
        registry: &RunRegistry,
        settings: &ServerSettings,
        name: &str,
        mode: RunMode,
        from_export: bool,
    ) -> StartOutcome {
        let text = match self.load_study(mode.operation(), name) {
            Ok(t) => t,
            Err(report) => return StartOutcome::Refused(Box::new(report)),
        };
        let mut request = RunRequest::new(
            mode,
            &settings.key,
            &settings.hosts,
            &settings.profile,
            self.dir(RUNS),
            &settings.socket_dir,
        );
        request.label = name.to_owned();
        if from_export {
            let dir = self.dir(EXPORTS).join(name);
            if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
                let found = ws_error(
                    "CFD-WS-002",
                    "There is no export of that name in the workspace.",
                );
                return StartOutcome::Refused(Box::new(failed(mode.operation(), found)));
            }
            request.case_dir = Some(dir);
        }
        registry.start(&text, request)
    }

    /// Start a job on the stored document `name` and return at once; the caller polls with the id
    /// of the run. What the job does follows from `kind`. Nothing starts when there is nothing to
    /// do or when the request is refused: those answers are a finished report, as is a busy host.
    /// The finished report is the operation's own (solve, update or rerun) and records the run in
    /// the document only if every check held. A rerun is compared under the tight tolerance of a
    /// rerun in the same image and never runs with other tools than the recorded ones.
    pub fn start_document_job(
        &self,
        registry: &RunRegistry,
        settings: &ServerSettings,
        name: &str,
        kind: DocJobKind,
    ) -> StartOutcome {
        let mut scenario = None;
        let mut options = JobOptions::default();
        let action = match kind {
            DocJobKind::Solve => JobAction::Solve,
            DocJobKind::SolveScenario {
                scenario: wanted,
                remake_pruned,
            } => {
                scenario = wanted;
                options = JobOptions { remake_pruned };
                JobAction::Solve
            }
            DocJobKind::Item(stage) => JobAction::Item(stage),
            DocJobKind::Rerun { run } => JobAction::Rerun {
                run,
                tolerance: Tolerance::same_image(),
                allow_tool_change: false,
            },
        };
        if !Self::valid_name(name) {
            return StartOutcome::Refused(Box::new(Self::bad_name(operation_of(&action))));
        }
        let planned = plan_scenario_job_with(
            &self.document_file(name),
            scenario.as_deref(),
            action,
            &options,
        );
        let job = match planned {
            Ok(job) => job,
            Err(outcome) => return StartOutcome::Refused(Box::new(self.within(outcome.report))),
        };
        let mut request = RunRequest::new(
            RunMode::Run,
            &settings.key,
            &settings.hosts,
            &settings.profile,
            self.dir(RUNS),
            &settings.socket_dir,
        );
        request.label = name.to_owned();
        let study = job.study_json().to_owned();
        let runs_dir = self.dir(RUNS);
        let profile = settings.profile.clone();
        let here = self.clone();
        registry.start_job(
            &study,
            request,
            Box::new(move |done| here.within(finish_job(job, &runs_dir, &profile, done))),
        )
    }

    pub fn run_status(&self, registry: &RunRegistry, run_id: &str) -> Status {
        if plain_id(run_id) {
            registry.status(run_id)
        } else {
            Status::NotFound
        }
    }

    pub fn run_result(&self, registry: &RunRegistry, run_id: &str) -> ResultOutcome {
        if plain_id(run_id) {
            registry.result(run_id)
        } else {
            ResultOutcome::NotFound
        }
    }
}

/// Put `fresh` at `target`. The old directory moves aside first and comes back if the move fails.
fn swap_in(parent: &Path, fresh: &Path, target: &Path) -> bool {
    let aside = temp_in(parent);
    let had_old = match std::fs::rename(target, &aside) {
        Ok(()) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(_) => return false,
    };
    if std::fs::rename(fresh, target).is_err() {
        if had_old {
            let _ = std::fs::rename(&aside, target);
        }
        return false;
    }
    let _ = std::fs::remove_dir_all(&aside);
    true
}

/// The passing report of a prepared study: its warnings, hash and migration note.
fn report_of(operation: &str, p: &crate::Prepared) -> Report {
    let report = Report::from_diagnostics(operation, p.warnings.clone(), Some(p.hash.clone()))
        .with_migration(p.migration.clone());
    debug_assert_eq!(report.class(), ExitClass::Passed);
    report
}

/// A name no study can have (it starts with a dot), unique within this process and run.
pub(crate) fn temp_in(dir: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    dir.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Pretty JSON through the shared exclusive create: an existing study is never replaced, and an
/// interrupted write leaves no partial study.
fn store(path: &Path, study: &Study) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(study).map_err(std::io::Error::other)?;
    craft_atomic::write_new(path, (json + "\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::relative_to;

    #[test]
    fn only_paths_under_the_root_are_made_relative() {
        let root = "/srv/ws";
        assert_eq!(
            relative_to(root, "/srv/ws/documents/a.cfdcraft is missing"),
            "documents/a.cfdcraft is missing"
        );
        assert_eq!(relative_to(root, "inside (/srv/ws)"), "inside (.)");
        assert_eq!(relative_to(root, "/srv/ws/a and /srv/ws/b"), "a and b");
        // Names that merely begin with the root's text are somebody else's paths.
        assert_eq!(relative_to(root, "/srv/ws2/x"), "/srv/ws2/x");
        assert_eq!(relative_to(root, "/srv/ws-old/x"), "/srv/ws-old/x");
        assert_eq!(relative_to(root, "/srv/ws.bak/x"), "/srv/ws.bak/x");
        assert_eq!(
            relative_to(root, "/srv/ws2/x then /srv/ws/y"),
            "/srv/ws2/x then y"
        );
    }

    #[test]
    fn a_root_in_the_middle_of_another_path_is_left_alone() {
        let root = "/srv/ws";
        assert_eq!(relative_to(root, "/data/srv/ws/x"), "/data/srv/ws/x");
        assert_eq!(relative_to(root, "/data-1/srv/ws"), "/data-1/srv/ws");
        assert_eq!(
            relative_to(root, "/data/srv/ws/x and /srv/ws/y"),
            "/data/srv/ws/x and y"
        );
        // After a quote, a bracket, an equals sign or a colon it starts a path of its own.
        assert_eq!(relative_to(root, "\"/srv/ws/a\" (/srv/ws)"), "\"a\" (.)");
        assert_eq!(relative_to(root, "dir=/srv/ws/a"), "dir=a");
        assert_eq!(relative_to(root, "at:/srv/ws"), "at:.");
    }
}
