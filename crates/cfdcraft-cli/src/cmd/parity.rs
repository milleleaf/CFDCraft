//! The verb that prints the parity table of a benchmark over the runs a study file has recorded:
//! the header sentence, the fixed-format table, the sentence for the three levels and one sentence
//! for each column. Findings, such as a column that failed its gate, go to standard error; with
//! `--json` the report is the only output. Nothing here needs a compute host.

use std::path::Path;

use cfdcraft_ops::{Repro, parity_table};

use super::Error;

/// Print the parity table of `benchmark` over the runs of `doc`.
pub fn execute(doc: &Path, benchmark: &str, json: bool) -> Result<i32, Error> {
    let report = parity_table(doc, benchmark, None);
    if json {
        println!("{}", report.to_json());
    } else {
        // The table is the answer, so a finding about a column never hides it.
        if let Some(Repro::Parity(data)) = &report.repro {
            print!("{}", data.text);
        }
        eprint!("{}", report.render_text());
    }
    Ok(i32::from(report.exit_class))
}
