use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Passed,
    Failed(Vec<String>),
}
impl Verdict {
    pub fn passed(&self) -> bool {
        matches!(self, Self::Passed)
    }
}

pub struct RunEvidence<'a> {
    pub block_mesh_log: &'a str,
    pub check_mesh_log: &'a str,
    pub solver_log: &'a str,
    pub time_directories: &'a [f64],
}
/// What a smoke run must have produced. There is deliberately no residual threshold here.
pub struct SmokeExpectations {
    pub end_time: f64,
    pub cell_count: u64,
    pub domain_volume: f64,
}
pub struct EvidenceExpectations {
    pub end_time: f64,
    pub cell_count: u64,
    pub domain_volume: f64,
    pub p_threshold: f64,
    pub u_threshold: f64,
}

fn verdict(reasons: Vec<String>) -> Verdict {
    if reasons.is_empty() {
        Verdict::Passed
    } else {
        Verdict::Failed(reasons)
    }
}
fn close(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-6 * b.abs().max(1e-12)
}
fn fatal(log: &str, label: &str, reasons: &mut Vec<String>) {
    for marker in [
        "FOAM FATAL",
        "Floating point exception",
        "Segmentation fault",
        "signal SIGFPE",
        "foamConfigureFvSchemes",
        "Unknown function type",
        "Unknown functionObject type",
        "cannot find functionObject file",
    ] {
        if log.lines().any(|line| {
            line.contains(marker)
                && !(marker == "Floating point exception"
                    && line.contains("Floating point exception trapping enabled (FOAM_SIGFPE)"))
        }) {
            reasons.push(format!("{label}: fatal signal {marker}"));
        }
    }
    if log.contains("Foam::sigFpe::sigHandler") {
        reasons.push(format!("{label}: SIGFPE stack trace"));
    }
    if log
        .split(|c: char| c.is_whitespace() || ",=();[]".contains(c))
        .any(|x| {
            matches!(
                x.to_ascii_lowercase().as_str(),
                "nan" | "-nan" | "inf" | "+inf" | "-inf" | "infinity" | "-infinity"
            )
        })
    {
        reasons.push(format!("{label}: non-finite number in log"));
    }
}
/// The last non-empty line must be `End`. A parallel run prints one `Finalising parallel run`
/// line after it, which is skipped.
fn ends(log: &str, label: &str, reasons: &mut Vec<String>) {
    let mut lines = log.lines().rev().map(str::trim).filter(|s| !s.is_empty());
    let mut last = lines.next();
    if last == Some("Finalising parallel run") {
        last = lines.next();
    }
    if last != Some("End") {
        reasons.push(format!("{label}: missing terminal End marker"));
    }
}
fn parse_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_once(key).and_then(|(_, s)| {
        s.trim()
            .split(|c: char| c.is_whitespace() || c == ',')
            .next()
    })
}
fn final_time(log: &str) -> Option<f64> {
    log.lines()
        .filter_map(|l| l.trim().strip_prefix("Time = "))
        .filter_map(|s| s.trim().parse::<f64>().ok())
        .next_back()
}

fn mesh(log: &str, cells: Option<u64>, volume: Option<f64>, reasons: &mut Vec<String>) {
    if !log.lines().any(|l| l.trim() == "Mesh OK.") && !log.lines().any(|l| l.trim() == "Mesh OK") {
        reasons.push("checkMesh: missing Mesh OK marker".into());
    }
    if log.contains("Failed ") || log.contains("***") {
        reasons.push("checkMesh: mesh checks failed".into());
    }
    let found = log
        .lines()
        .filter_map(|l| l.trim().strip_prefix("cells:"))
        .find_map(|s| s.trim().parse::<u64>().ok());
    if let Some(expected) = cells
        && found != Some(expected)
    {
        reasons.push(format!(
            "checkMesh: expected {expected} cells, found {found:?}"
        ));
    }
    let found_volume = log.lines().find_map(|l| {
        parse_after(l, "Total volume =").and_then(|s| s.trim_end_matches('.').parse::<f64>().ok())
    });
    if !found_volume.is_some_and(|v| v.is_finite() && v > 0.0) {
        reasons.push("checkMesh: missing or invalid positive total volume".into());
    }
    if let Some(expected) = volume
        && !found_volume.is_some_and(|v| close(v, expected))
    {
        reasons.push(format!(
            "checkMesh: volume {found_volume:?} differs from expected {expected}"
        ));
    }
}

/// `limits` is `Some((p, U))` for the convergence judge; `None` only checks that residuals are well formed.
fn residuals(log: &str, limits: Option<(f64, f64)>, reasons: &mut Vec<String>) {
    let mut last = BTreeMap::new();
    let mut seen_time = false;
    for line in log.lines() {
        if line.trim().starts_with("Time = ") {
            last.clear();
            seen_time = true;
        }
        if let Some((_, tail)) = line.split_once("Solving for ") {
            let field = tail.split(',').next().unwrap_or("").trim();
            let initial =
                parse_after(line, "Initial residual =").and_then(|s| s.parse::<f64>().ok());
            let final_r = parse_after(line, "Final residual =").and_then(|s| s.parse::<f64>().ok());
            if !initial.is_some_and(|x| x.is_finite() && x >= 0.0)
                || !final_r.is_some_and(|x| x.is_finite() && x >= 0.0)
            {
                reasons.push(format!("solver: invalid residual for {field}"));
            }
            last.insert(field.to_string(), initial);
        }
    }
    if !seen_time {
        reasons.push("solver: no iteration blocks".into());
    }
    let Some((p_threshold, u_threshold)) = limits else {
        // Smoke: no limits, but a run that solved nothing in its final iteration never iterated.
        if seen_time && last.is_empty() {
            reasons.push("solver: no solved field in the final iteration".into());
        }
        return;
    };
    for (field, threshold) in [("p", p_threshold), ("Ux", u_threshold), ("Uy", u_threshold)] {
        if !threshold.is_finite() || threshold <= 0.0 {
            reasons.push(format!("solver: invalid {field} convergence threshold"));
            continue;
        }
        match last.get(field).copied().flatten() {
            Some(value) if value.is_finite() && value >= 0.0 && value <= threshold => {}
            Some(value) => reasons.push(format!(
                "solver: final iteration {field} initial residual {value} exceeds {threshold}"
            )),
            None => reasons.push(format!(
                "solver: missing final iteration residual for {field}"
            )),
        }
    }
}

fn judge_evidence(
    e: &RunEvidence<'_>,
    end_time: f64,
    cell_count: u64,
    domain_volume: f64,
    limits: Option<(f64, f64)>,
) -> Verdict {
    let mut reasons = Vec::new();
    for (name, log) in [
        ("blockMesh", e.block_mesh_log),
        ("checkMesh", e.check_mesh_log),
        ("simpleFoam", e.solver_log),
    ] {
        ends(log, name, &mut reasons);
        fatal(log, name, &mut reasons);
    }
    if !final_time(e.solver_log).is_some_and(|t| close(t, end_time)) {
        reasons.push(format!("solver: final time differs from {end_time}"));
    }
    let final_dir = e
        .time_directories
        .iter()
        .copied()
        .filter(|t| t.is_finite())
        .max_by(f64::total_cmp);
    if !final_dir.is_some_and(|t| close(t, end_time)) {
        reasons.push(format!(
            "outputs: final time directory {final_dir:?} differs from {end_time}"
        ));
    }
    mesh(
        e.check_mesh_log,
        Some(cell_count),
        Some(domain_volume),
        &mut reasons,
    );
    residuals(e.solver_log, limits, &mut reasons);
    verdict(reasons)
}

/// Judge each step and actual output-directory evidence; the process exit code is deliberately absent.
pub fn judge_run(e: &RunEvidence<'_>, expected: &EvidenceExpectations) -> Verdict {
    judge_evidence(
        e,
        expected.end_time,
        expected.cell_count,
        expected.domain_volume,
        Some((expected.p_threshold, expected.u_threshold)),
    )
}

/// Smoke verdict: the case loaded and iterated to its end time. It proves nothing about convergence or
/// correctness, so callers must always label its result as a smoke result and never as a validated one.
/// Every rule of `judge_run` applies except the residual limits.
pub fn judge_smoke(e: &RunEvidence<'_>, expected: &SmokeExpectations) -> Verdict {
    judge_evidence(
        e,
        expected.end_time,
        expected.cell_count,
        expected.domain_volume,
        None,
    )
}

/// What a run that was stopped on request must show. There is no residual limit and no end time to
/// reach: the run was cut short on purpose.
pub struct PartialExpectations {
    pub end_time: f64,
    pub cell_count: u64,
    pub domain_volume: f64,
}

/// Judges a run that was stopped on request. It proves the stop was clean (every log ends, nothing
/// fatal, a good mesh, well formed residuals, and the last logged time is the newest time folder
/// and lies in `(0, end_time]`). It proves nothing about convergence, so its result must never be
/// stored or shown as a finished run.
pub fn judge_partial(e: &RunEvidence<'_>, expected: &PartialExpectations) -> Verdict {
    let mut reasons = Vec::new();
    for (name, log) in [
        ("blockMesh", e.block_mesh_log),
        ("checkMesh", e.check_mesh_log),
        ("simpleFoam", e.solver_log),
    ] {
        ends(log, name, &mut reasons);
        fatal(log, name, &mut reasons);
    }
    let newest = e
        .time_directories
        .iter()
        .copied()
        .filter(|t| t.is_finite())
        .max_by(f64::total_cmp);
    match final_time(e.solver_log) {
        Some(t)
            if t.is_finite()
                && t > 0.0
                && (t <= expected.end_time || close(t, expected.end_time)) =>
        {
            if !newest.is_some_and(|d| close(d, t)) {
                reasons.push(format!(
                    "outputs: newest time directory {newest:?} differs from the last logged time {t}"
                ));
            }
        }
        Some(t) => reasons.push(format!(
            "solver: last logged time {t} is not above zero and within {}",
            expected.end_time
        )),
        None => reasons.push("solver: no logged time".into()),
    }
    mesh(
        e.check_mesh_log,
        Some(expected.cell_count),
        Some(expected.domain_volume),
        &mut reasons,
    );
    residuals(e.solver_log, None, &mut reasons);
    verdict(reasons)
}

/// Combined-log compatibility entry point. Real executions must use `judge_run` to prove every step and output directory.
pub fn judge(log_text: &str, expected_end_time: f64, expected_cell_count: Option<u64>) -> Verdict {
    let mut reasons = Vec::new();
    ends(log_text, "combined log", &mut reasons);
    fatal(log_text, "combined log", &mut reasons);
    if !final_time(log_text).is_some_and(|t| close(t, expected_end_time)) {
        reasons.push("solver: final time mismatch".into());
    }
    mesh(log_text, expected_cell_count, None, &mut reasons);
    residuals(log_text, Some((1e-6, 1e-6)), &mut reasons);
    verdict(reasons)
}
