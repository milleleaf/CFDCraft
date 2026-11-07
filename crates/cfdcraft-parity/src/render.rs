//! The fixed-format plain-text table. It holds titles and numbers only; sentences about a table
//! are written elsewhere.

use std::collections::BTreeSet;

use crate::ParityReport;

/// Pad every cell to its column width, two spaces between columns, no trailing blanks.
fn table(rows: &[Vec<String>]) -> Vec<String> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..cols)
        .map(|i| {
            rows.iter()
                .filter_map(|r| r.get(i))
                .map(|c| c.chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let w = widths.get(i).copied().unwrap_or(0);
                    format!("{c:<w$}")
                })
                .collect();
            cells.join("  ").trim_end().to_owned()
        })
        .collect()
}

/// The table: a header, one line per column with its gate, convergence and levels, then one line
/// per quantity with the baseline value and, for each column, its value, difference and verdict.
/// Numbers are written as `{:.6}` and differences as `{:.3e}`; a cell with no value is `-`.
pub fn render_table(report: &ParityReport) -> String {
    let mut lines = vec![
        format!("benchmark: {}", report.benchmark),
        format!("case: {}", report.case),
        format!(
            "baseline: {} {}",
            report.baseline.backend,
            report.baseline.gate.as_str()
        ),
        String::new(),
    ];

    let mut summary = vec![
        [
            "backend",
            "gate",
            "converged",
            "preview",
            "golden",
            "oracle",
            "parity",
        ]
        .map(str::to_owned)
        .to_vec(),
    ];
    for c in &report.columns {
        summary.push(vec![
            c.backend.clone(),
            c.gate.as_str().to_owned(),
            c.converged.to_string(),
            c.preview.to_string(),
            c.levels.golden.as_str().to_owned(),
            c.levels.oracle.as_str().to_owned(),
            c.levels.parity.as_str().to_owned(),
        ]);
    }
    lines.extend(table(&summary));
    lines.push(String::new());

    let quantities: BTreeSet<&str> = report
        .columns
        .iter()
        .flat_map(|c| c.rows.iter().map(|r| r.quantity.as_str()))
        .collect();
    let mut title = vec!["quantity".to_owned(), "baseline".to_owned()];
    for c in &report.columns {
        title.push(format!("{} value", c.backend));
        title.push(format!("{} abs_diff", c.backend));
        title.push(format!("{} verdict", c.backend));
    }
    let mut body = vec![title];
    for q in quantities {
        let found: Vec<_> = report
            .columns
            .iter()
            .map(|c| c.rows.iter().find(|r| r.quantity == q))
            .collect();
        let baseline = found
            .iter()
            .flatten()
            .next()
            .map_or_else(|| "-".to_owned(), |r| format!("{:.6}", r.baseline));
        let mut row = vec![q.to_owned(), baseline];
        for r in found {
            match r {
                Some(r) => {
                    row.push(format!("{:.6}", r.value));
                    row.push(format!("{:.3e}", r.abs_diff));
                    row.push(r.verdict.as_str().to_owned());
                }
                None => row.extend(["-", "-", "-"].map(str::to_owned)),
            }
        }
        body.push(row);
    }
    lines.extend(table(&body));
    let mut out = lines.join("\n");
    out.push('\n');
    out
}
