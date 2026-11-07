use super::Error;
use cfdcraft_ops::{Finished, Report, RunMode, RunRequest, cases::cli_problem, run_study};
use std::path::{Path, PathBuf};

pub struct RunArgs {
    pub mode: RunMode,
    pub study: PathBuf,
    pub key: PathBuf,
    pub profile: String,
    pub hosts: PathBuf,
    pub case_dir: Option<PathBuf>,
    pub tolerance_override: Option<PathBuf>,
    pub json: bool,
}

/// The study file's name as a run label: every character outside `[A-Za-z0-9-]` becomes `-`.
pub(super) fn label(study: &Path) -> String {
    let stem: String = study
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if stem.is_empty() {
        "study".to_owned()
    } else {
        stem
    }
}

pub fn execute(args: &RunArgs) -> Result<i32, Error> {
    let operation = args.mode.operation();
    let finished = match std::fs::read_to_string(&args.study) {
        Err(e) => Finished {
            report: Report::from_diagnostics(
                operation,
                vec![cli_problem(format!(
                    "cannot read study {}: {e}",
                    args.study.display()
                ))],
                None,
            ),
            detail: None,
        },
        Ok(text) => {
            let target = std::env::current_dir()?.join("target");
            let mut request = RunRequest::new(
                args.mode,
                &args.key,
                &args.hosts,
                &args.profile,
                target.join("cfdcraft-runs"),
                target.join("cfdcraft-sockets"),
            );
            request.case_dir = args.case_dir.clone();
            request.tolerance_override = args.tolerance_override.clone();
            request.label = label(&args.study);
            run_study(&text, &request)
        }
    };
    Ok(emit(&finished, args.json))
}

/// Prints the outcome and returns the exit status. Stdout carries only the public result: the
/// report as JSON, or the summary. The migration note, findings and the executor's full error text
/// (which can name the key, the host and remote directories) go to stderr.
fn emit(finished: &Finished, json: bool) -> i32 {
    let report = &finished.report;
    if json {
        println!("{}", report.to_json());
    } else {
        if let Some(hash) = &report.study_hash {
            println!("study hash: {hash}");
        }
        if let Some(run) = &report.run {
            println!("run: {}", run.run_id);
            println!("duration: {:.3}s", run.duration_seconds);
            if let Some(code) = run.process_exit {
                println!("solver exit status: {code}");
            }
        }
        if let Some(evidence) = &report.evidence {
            print!("{}", evidence.render_text(report.operation == "smoke"));
        }
        if let Some(gate) = &report.gate {
            print!("{}", gate.render_text());
        }
        if let Some(note) = &report.migration {
            eprintln!("note: {}", note.summary);
        }
        eprint!("{}", cfdcraft_model::render_text(&report.diagnostics));
    }
    if let Some(detail) = &finished.detail {
        eprintln!("{detail}");
    }
    i32::from(report.exit_class)
}
