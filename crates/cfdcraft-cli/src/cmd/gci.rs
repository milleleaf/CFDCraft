//! The verbs for mesh independence: store a sweep over the mesh level, and report the grid
//! convergence index of a key number from the runs of its points.
//!
//! `create` prints a sentence. `report` prints the summary sentence on standard output and each
//! warning about the series on standard error; with `--json` the report is the only output.
//! Nothing here needs a compute host.

use std::path::Path;

use cfdcraft_ops::gci_ops::{GciRequest, gci_create, gci_report, gci_warning_lines};
use cfdcraft_ops::{DocOutcome, ExitClass, Report, Repro};
use cfdcraft_text::Text;

use super::{Error, emit_doc};

/// The levels of a comma separated list such as `0,1,2`.
fn parse_levels(text: &str) -> Option<Vec<u32>> {
    text.split(',')
        .map(|part| part.trim().parse::<u32>().ok())
        .collect()
}

/// A request the command line itself cannot make, as a refused outcome.
fn refusal(operation: &str, message: &str) -> DocOutcome {
    DocOutcome {
        report: Report::from_diagnostics(
            operation,
            vec![cfdcraft_ops::cases::cli_problem(message.to_owned())],
            None,
        ),
        line: None,
    }
}

/// Store a sweep over the mesh level, starting from `scenario`.
pub fn create(
    doc: &Path,
    sweep: &str,
    scenario: &str,
    levels: &str,
    json: bool,
) -> Result<i32, Error> {
    let outcome = match parse_levels(levels) {
        Some(levels) => gci_create(
            doc,
            &GciRequest {
                sweep: sweep.to_owned(),
                scenario: scenario.to_owned(),
                levels,
            },
        ),
        None => refusal(
            "gci-create",
            "The mesh levels must be a comma separated list of whole numbers, such as 0,1,2.",
        ),
    };
    Ok(emit_doc(&outcome, json))
}

/// Report the grid convergence index of `quantity` over the points of `sweep`.
pub fn report(doc: &Path, sweep: &str, quantity: &str, json: bool) -> Result<i32, Error> {
    let outcome = gci_report(doc, sweep, quantity);
    let code = emit_doc(&outcome, json);
    if !json
        && outcome.report.class() == ExitClass::Passed
        && let Some(Repro::Gci(data)) = &outcome.report.repro
    {
        let text = Text::english();
        for line in gci_warning_lines(data) {
            eprintln!("{}", text.render(&line));
        }
    }
    Ok(code)
}
