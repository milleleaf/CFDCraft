//! The verbs for benchmarks: list the templates, write a complete study from one, and check a
//! recorded run against its published reference.
//!
//! The words that choose the options are read by the operations, not here. None of these verbs
//! needs a compute host. `check` prints the table of the gate on standard output, then the
//! sentence of the verdict, and its exit status is the class of the report: 1 for a gate that
//! failed, 2 for a refusal.

use std::path::Path;

use cfdcraft_ops::{
    BenchmarkOptions, DocOutcome, ExitClass, Repro, benchmark_check, benchmark_create,
    benchmark_list, created_line,
};
use cfdcraft_text::Text;

use super::document::write_new_text;
use super::{Error, emit_doc};

/// Print the templates, one sentence each, in the order the operation gives them.
pub fn list(json: bool) -> Result<i32, Error> {
    let report = benchmark_list();
    if json {
        println!("{}", report.to_json());
    } else {
        if let Some(Repro::Benchmarks(data)) = &report.repro {
            let text = Text::english();
            for line in data.lines() {
                println!("{}", text.render(&line));
            }
        }
        eprint!("{}", report.render_text());
    }
    Ok(i32::from(report.exit_class))
}

/// Write the study of template `id`, with the options chosen, to the new file `out`.
pub fn create(id: &str, out: &Path, options: &BenchmarkOptions, json: bool) -> Result<i32, Error> {
    let (report, text) = benchmark_create(id, options);
    let Some(text) = text else {
        return Ok(emit_doc(&DocOutcome { report, line: None }, json));
    };
    if let Some(refused) = write_new_text(&report.operation, out, &text) {
        return Ok(emit_doc(&refused, json));
    }
    let shown = out.display().to_string();
    let outcome = DocOutcome {
        line: Some(created_line(id, options, &shown)),
        report: report.with_artifact("study", shown),
    };
    Ok(emit_doc(&outcome, json))
}

/// Check the stored result of a recorded run of `doc` against the benchmark its study is.
pub fn check(
    doc: &Path,
    run: Option<&str>,
    benchmark: Option<&str>,
    tolerance_override: Option<&Path>,
    json: bool,
) -> Result<i32, Error> {
    let mut outcome = benchmark_check(doc, run, benchmark, tolerance_override);
    if !json {
        // The table is the answer, so it goes to standard output once; the findings stay on
        // standard error. A passing verdict is printed with the rest by `emit_doc`.
        // A run that is not converged, or whose state is unknown, is flagged before the table;
        // the verdict and the exit status are those of the comparison alone.
        if let Some(gate) = outcome.report.gate.take() {
            if let Some(flag) = gate.convergence.as_ref().filter(|c| !c.message.is_empty()) {
                println!("{}", flag.message);
            }
            print!("{}", gate.render_text());
        }
        if outcome.report.class() != ExitClass::Passed
            && let Some(line) = &outcome.line
        {
            println!("{}", Text::english().render(line));
        }
    }
    Ok(emit_doc(&outcome, json))
}
