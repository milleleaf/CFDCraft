pub mod check_case;
pub mod compile;
pub mod document;
pub mod export;
pub mod keys;
pub mod repro;
pub mod run;
pub type Error = Box<dyn std::error::Error>;

/// Reads a study file and runs `operation` on its text. A file that cannot be read is a finished,
/// classified report, so the offline verbs answer it the same way as any other invalid input.
pub fn with_study(
    path: &std::path::Path,
    operation: &str,
    run: impl FnOnce(&str) -> cfdcraft_ops::Report,
) -> cfdcraft_ops::Report {
    match std::fs::read_to_string(path) {
        Ok(text) => run(&text),
        Err(e) => cfdcraft_ops::Report::from_diagnostics(
            operation,
            vec![cfdcraft_ops::cases::cli_problem(format!(
                "cannot read study {}: {e}",
                path.display()
            ))],
            None,
        ),
    }
}

/// Prints a report and returns the process exit status for it.
///
/// With `json` the report is the only thing on stdout and nothing goes to stderr. Otherwise a
/// passing run prints the study hash and `label: path` on stdout, and every note and finding goes
/// to stderr.
pub fn emit(report: &cfdcraft_ops::Report, json: bool, label: &str, path: &std::path::Path) -> i32 {
    if json {
        println!("{}", report.to_json());
    } else {
        if report.class() == cfdcraft_ops::ExitClass::Passed {
            if let Some(hash) = &report.study_hash {
                println!("study hash: {hash}");
            }
            println!("{label}: {}", path.display());
        }
        eprint!("{}", report.render_text());
    }
    i32::from(report.exit_class)
}

/// Prints the outcome of a document operation and returns the process exit status for it.
///
/// With `json` the report is the only thing on stdout and nothing goes to stderr. Otherwise a
/// passing run prints the study hash (when there is one) and the success sentence on stdout, and
/// every note and finding goes to stderr.
pub fn emit_doc(outcome: &cfdcraft_ops::DocOutcome, json: bool) -> i32 {
    let report = &outcome.report;
    if json {
        println!("{}", report.to_json());
    } else {
        if report.class() == cfdcraft_ops::ExitClass::Passed {
            if let Some(hash) = &report.study_hash {
                println!("study hash: {hash}");
            }
            if let Some(line) = &outcome.line {
                println!("{}", cfdcraft_text::Text::english().render(line));
            }
        }
        eprint!("{}", report.render_text());
    }
    i32::from(report.exit_class)
}
