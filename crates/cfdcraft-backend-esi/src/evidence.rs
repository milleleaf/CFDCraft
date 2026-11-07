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

/// What a run of a given solver is judged on: the solver's name for the reasons, the field whose
/// residual gates a steady run, whether a temperature residual exists, whether residual limits
/// apply at all, and the log of the potential-flow step when there was one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidencePlan {
    pub solver: String,
    pub pressure: String,
    pub temperature: bool,
    pub residual_limits: bool,
    pub potential_log: Option<String>,
}

impl EvidencePlan {
    /// The steady laminar cavity solver: exactly what the judge has always assumed.
    pub fn cavity() -> Self {
        Self {
            solver: "simpleFoam".into(),
            pressure: "p".into(),
            temperature: false,
            residual_limits: true,
            potential_log: None,
        }
    }

    /// The fields whose last initial residual must be under a limit, with that limit.
    fn checks(&self, p_threshold: f64, u_threshold: f64) -> Vec<(String, f64)> {
        let mut checks = vec![
            (self.pressure.clone(), p_threshold),
            ("Ux".to_string(), u_threshold),
            ("Uy".to_string(), u_threshold),
        ];
        if self.temperature {
            checks.push(("T".to_string(), u_threshold));
        }
        checks
    }

    /// The checks of the plan's judge: none when no residual limit applies or none was asked for.
    fn limits(&self, limits: Option<(f64, f64)>) -> Option<Vec<(String, f64)>> {
        limits
            .filter(|_| self.residual_limits)
            .map(|(p, u)| self.checks(p, u))
    }

    /// The log of the potential-flow step must end and hold nothing fatal.
    fn check_potential(&self, reasons: &mut Vec<String>) {
        if let Some(text) = &self.potential_log {
            ends(text, "potentialFoam", reasons);
            fatal(text, "potentialFoam", reasons);
        }
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
        .filter_map(|s| {
            // A transient run prints the time with its unit letter.
            let s = s.trim();
            s.strip_suffix('s').unwrap_or(s).trim().parse::<f64>().ok()
        })
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

/// `limits` is the list of fields and limits for the convergence judge; `None` only checks that
/// residuals are well formed.
fn residuals(log: &str, limits: Option<&[(String, f64)]>, reasons: &mut Vec<String>) {
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
    let Some(limits) = limits else {
        // Smoke: no limits, but a run that solved nothing in its final iteration never iterated.
        if seen_time && last.is_empty() {
            reasons.push("solver: no solved field in the final iteration".into());
        }
        return;
    };
    for (field, threshold) in limits {
        let threshold = *threshold;
        if !threshold.is_finite() || threshold <= 0.0 {
            reasons.push(format!("solver: invalid {field} convergence threshold"));
            continue;
        }
        match last.get(field.as_str()).copied().flatten() {
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
    plan: &EvidencePlan,
) -> Verdict {
    let mut reasons = Vec::new();
    for (name, log) in [
        ("blockMesh", e.block_mesh_log),
        ("checkMesh", e.check_mesh_log),
        (plan.solver.as_str(), e.solver_log),
    ] {
        ends(log, name, &mut reasons);
        fatal(log, name, &mut reasons);
    }
    plan.check_potential(&mut reasons);
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
    residuals(e.solver_log, plan.limits(limits).as_deref(), &mut reasons);
    verdict(reasons)
}

/// Judge each step and actual output-directory evidence; the process exit code is deliberately absent.
pub fn judge_run(e: &RunEvidence<'_>, expected: &EvidenceExpectations) -> Verdict {
    judge_run_with(e, expected, &EvidencePlan::cavity())
}

/// `judge_run` for the solver of a plan. A plan without residual limits (a transient run) is
/// judged on its end markers, its end time and its fields, and its residuals must be well formed.
pub fn judge_run_with(
    e: &RunEvidence<'_>,
    expected: &EvidenceExpectations,
    plan: &EvidencePlan,
) -> Verdict {
    judge_evidence(
        e,
        expected.end_time,
        expected.cell_count,
        expected.domain_volume,
        Some((expected.p_threshold, expected.u_threshold)),
        plan,
    )
}

/// Smoke verdict: the case loaded and iterated to its end time. It proves nothing about convergence or
/// correctness, so callers must always label its result as a smoke result and never as a validated one.
/// Every rule of `judge_run` applies except the residual limits.
pub fn judge_smoke(e: &RunEvidence<'_>, expected: &SmokeExpectations) -> Verdict {
    judge_smoke_with(e, expected, &EvidencePlan::cavity())
}

/// `judge_smoke` for the solver of a plan.
pub fn judge_smoke_with(
    e: &RunEvidence<'_>,
    expected: &SmokeExpectations,
    plan: &EvidencePlan,
) -> Verdict {
    judge_evidence(
        e,
        expected.end_time,
        expected.cell_count,
        expected.domain_volume,
        None,
        plan,
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
    judge_partial_with(e, expected, &EvidencePlan::cavity())
}

/// `judge_partial` for the solver of a plan.
pub fn judge_partial_with(
    e: &RunEvidence<'_>,
    expected: &PartialExpectations,
    plan: &EvidencePlan,
) -> Verdict {
    let mut reasons = Vec::new();
    for (name, log) in [
        ("blockMesh", e.block_mesh_log),
        ("checkMesh", e.check_mesh_log),
        (plan.solver.as_str(), e.solver_log),
    ] {
        ends(log, name, &mut reasons);
        fatal(log, name, &mut reasons);
    }
    plan.check_potential(&mut reasons);
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
    let checks = EvidencePlan::cavity().checks(1e-6, 1e-6);
    residuals(log_text, Some(&checks), &mut reasons);
    verdict(reasons)
}
