//! Synthetic monitor files in the layouts the solver writes. Numbers carry twelve decimals.

// Each test crate uses only some of these.
#![allow(dead_code)]

/// One value: a scalar for one number, a parenthesised vector for three.
fn cell(values: &[f64]) -> String {
    let words: Vec<String> = values.iter().map(|v| format!("{v:.12}")).collect();
    if words.len() == 3 {
        format!("({})", words.join(" "))
    } else {
        words.join(" ")
    }
}

pub fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} differs from {b}");
}

/// A probes file: a header per location, the index line, then a time and one cell per probe.
pub fn probes_text(locations: &[[f64; 3]], rows: &[(f64, Vec<Vec<f64>>)]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (i, l) in locations.iter().enumerate() {
        lines.push(format!(
            "# Probe {i} ({:.12} {:.12} {:.12})",
            l[0], l[1], l[2]
        ));
    }
    let indexes: Vec<String> = (0..locations.len()).map(|i| format!("{i:>13}")).collect();
    lines.push(format!("#       Probe {}", indexes.join(" ")));
    lines.push("#        Time".to_owned());
    for (time, cells) in rows {
        let cells: Vec<String> = cells.iter().map(|c| cell(c)).collect();
        lines.push(format!("{time:.12}   {}", cells.join("   ")));
    }
    lines.join("\n") + "\n"
}

/// A surface field value file: the region header, the column line, then a time and the cells separated by tabs.
pub fn surface_text(
    region: &str,
    faces: u64,
    area: f64,
    columns: &[&str],
    rows: &[(f64, Vec<Vec<f64>>)],
) -> String {
    let mut lines = vec![
        format!("# Region type : {region}"),
        format!("# Faces       : {faces}"),
        format!("# Area        : {area:.12}"),
        "# Scale factor : 1".to_owned(),
        "#".to_owned(),
        format!("# Time\t{}", columns.join("\t")),
    ];
    for (time, cells) in rows {
        let cells: Vec<String> = cells.iter().map(|c| cell(c)).collect();
        lines.push(format!("{time:.12}\t{}", cells.join("\t")));
    }
    lines.join("\n") + "\n"
}

/// A sampled line in the plain numeric layout: no header, one row of numbers per point.
pub fn xy_text(rows: &[Vec<f64>]) -> String {
    let lines: Vec<String> = rows
        .iter()
        .map(|row| {
            let words: Vec<String> = row.iter().map(|v| format!("{v:.12}")).collect();
            words.join(" ")
        })
        .collect();
    lines.join("\n") + "\n"
}
