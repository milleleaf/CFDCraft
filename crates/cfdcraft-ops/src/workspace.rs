//! A server-side workspace: studies, compiled cases, exports and runs addressed by name, never by
//! path. A name is `[a-z0-9][a-z0-9-]{0,39}`, so no operation can reach outside the workspace.
//!
//! Nothing here takes a path, a host or a key from the caller. Whoever hosts the workspace decides
//! where it lives; the callers only supply names, modes and a study.

use crate::{
    cases::{cli_problem, compile_study, export_study},
    prepare::prepare,
    registry::{ResultOutcome, RunRegistry, StartOutcome, Status},
    report::{ExitClass, Report},
    run::{RunMode, RunRequest},
};
use cfdcraft_backend_esi::Dialect;
use cfdcraft_model::{Diagnostic, Study};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const STUDIES: &str = "studies";
const COMPILED: &str = "compiled";
const EXPORTS: &str = "exports";
const RUNS: &str = "runs";

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
    /// Open the workspace at `root`, creating it and its four directories if they are absent.
    /// A directory that is a symbolic link, or not a directory, is refused.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let root = root.as_ref();
        std::fs::create_dir_all(root)
            .map_err(|e| format!("the workspace cannot be opened: {e}"))?;
        let root = root
            .canonicalize()
            .map_err(|e| format!("the workspace cannot be opened: {e}"))?;
        for sub in [STUDIES, COMPILED, EXPORTS, RUNS] {
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

    /// The one place a name is judged: a lower-case letter or digit, then up to 39 more lower-case
    /// letters, digits or hyphens.
    pub fn valid_name(name: &str) -> bool {
        let b = name.as_bytes();
        (1..=40).contains(&b.len())
            && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
            && b.iter()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
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
        match store(&self.dir(STUDIES), &path, &parsed) {
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

/// Pretty JSON into a temporary file in `dir`, then linked into place so an existing study is
/// never replaced, and the temporary name removed.
fn store(dir: &Path, path: &Path, study: &Study) -> std::io::Result<()> {
    let tmp = temp_in(dir);
    let json = serde_json::to_string_pretty(study).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, json + "\n")?;
    let linked = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    linked
}
