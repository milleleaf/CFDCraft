//! Judging a finished lattice run from its own evidence: one reply to every request, a solve reply
//! that reports its steps, why it stopped, how the fluid mass changed and whether the flow became
//! unstable, and a solver log that holds text. The exit status of the process takes no part: the
//! solver prints almost nothing, so the replies are the evidence, and a missing reply fails the run.
//!
//! The convergence verdict is here too. It reads the same replies through the same reader, and the
//! monitored series through the result view, never through a second reader of the replies.

use super::results::{FromRun, View};
use crate::{
    markers::{
        ENVELOPE_CONTENT, ENVELOPE_ERROR, ENVELOPE_IS_ERROR, ENVELOPE_RESULT, ENVELOPE_STRUCTURED,
        ENVELOPE_TEXT, EXECUTE_TOOL, INSTABILITY_WORD, LOG_HOLDS_TEXT, MASS_CHANGE_LIMIT,
        NORMAL_STOPS, READ_CAP_BYTES, REPLY_CHANGE, REPLY_MASS, REPLY_MASS_CHANGE, REPLY_STEPS,
        REPLY_STOP, REPLY_WARNINGS, SOLVE_COMMAND, STEADY_STOP,
    },
    plan::{COMMANDS_FILE, LOG_FILE, REPLIES_FILE, run_plan},
};
use cfdcraft_backend::{ConvergenceOutcome, JudgeMode, JudgeOutcome, RunFiles};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::ResolvedStudy;
use cfdcraft_settle::{PeriodicOptions, PeriodicStatus, Samples, assess_periodic};
use serde_json::Value;

const CUT_SENTENCE: &str = "A file of the run was longer than the read limit of 8 MiB, so it was read only up to that size.";

/// One request of the stream: its number, the tool it calls and the arguments of the call.
pub(super) struct Request {
    pub id: u64,
    pub tool: String,
    pub arguments: Value,
}

impl Request {
    /// The command and its parameters, for a call that runs one command.
    pub fn command(&self) -> Option<(&str, &Value)> {
        if self.tool != EXECUTE_TOOL {
            return None;
        }
        let name = self.arguments.get("command")?.as_str()?;
        Some((name, self.arguments.get("params")?))
    }
}

/// One reply: the number of the request it answers and its answer, which is `None` for an error.
pub(super) struct Reply {
    pub id: Option<u64>,
    pub payload: Option<Value>,
}

pub(super) fn parse_requests(text: &str) -> Vec<Request> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|message| {
            let id = message.get("id")?.as_u64()?;
            let params = message.get("params")?;
            Some(Request {
                id,
                tool: params.get("name")?.as_str()?.to_owned(),
                arguments: params.get("arguments")?.clone(),
            })
        })
        .collect()
}

/// The answer a reply carries, or `None` when the reply is an error or holds no result.
fn payload_of(message: &Value) -> Option<Value> {
    if message.get(ENVELOPE_ERROR).is_some() {
        return None;
    }
    let result = message.get(ENVELOPE_RESULT)?;
    if result.get(ENVELOPE_IS_ERROR).and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if let Some(structured) = result.get(ENVELOPE_STRUCTURED) {
        return Some(structured.clone());
    }
    let text = result
        .get(ENVELOPE_CONTENT)
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get(ENVELOPE_TEXT))
        .and_then(Value::as_str);
    match text {
        Some(text) => Some(serde_json::from_str(text).unwrap_or_else(|_| Value::from(text))),
        None => Some(result.clone()),
    }
}

/// Every line that is JSON gives a reply; a line that is not gives an error reply without a number.
pub(super) fn parse_replies(text: &str) -> Vec<Reply> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(message) => Reply {
                id: message.get("id").and_then(Value::as_u64),
                payload: payload_of(&message),
            },
            Err(_) => Reply {
                id: None,
                payload: None,
            },
        })
        .collect()
}

pub(super) fn reply_to(replies: &[Reply], id: u64) -> Option<&Reply> {
    replies.iter().find(|reply| reply.id == Some(id))
}

/// The end of a text that is longer than the read limit, from the start of a whole line.
fn tail(text: String) -> String {
    let cap = usize::try_from(READ_CAP_BYTES).unwrap_or(usize::MAX);
    if text.len() <= cap {
        return text;
    }
    let mut start = text.len() - cap;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if text.as_bytes()[start - 1] == b'\n' {
        return text[start..].to_owned();
    }
    match text[start..].find('\n') {
        Some(at) => text[start + at + 1..].to_owned(),
        None => String::new(),
    }
}

/// A file's text, read up to the limit (its end when it is longer), and whether it was cut.
fn take(files: &dyn RunFiles, rel: &str, cut: &mut bool) -> Option<String> {
    let text = files.read_text(rel)?;
    let longer = files.size(rel).is_some_and(|size| size > READ_CAP_BYTES)
        || u64::try_from(text.len()).is_ok_and(|len| len > READ_CAP_BYTES);
    *cut |= longer;
    Some(if longer { tail(text) } else { text })
}

/// The step count the stream asks for, when a step names one.
fn requested_steps(requests: &[Request]) -> Option<u64> {
    requests
        .iter()
        .filter_map(|request| request.arguments.get("commands")?.as_array())
        .flatten()
        .filter(|entry| entry.get("command").and_then(Value::as_str) == Some("model.step.create"))
        .find_map(|entry| entry.pointer("/params/procedure/steps")?.as_u64())
}

/// A word of the solver's own text for a report: letters, digits and a few signs, and short.
fn word(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
        .take(32)
        .collect()
}

fn warned_unstable(payload: &Value) -> bool {
    payload
        .get(REPLY_WARNINGS)
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                let text = item
                    .as_str()
                    .map_or_else(|| item.to_string(), str::to_owned);
                text.to_lowercase().contains(INSTABILITY_WORD)
            })
        })
}

fn check_solve(payload: &Value, requested: Option<u64>, reasons: &mut Vec<String>) {
    let stop = payload.get(REPLY_STOP).and_then(Value::as_str);
    match stop {
        None => reasons.push("The solve reply does not say why the run stopped.".to_owned()),
        Some(why) if !NORMAL_STOPS.contains(&why) => reasons.push(format!(
            "The run stopped for the reason {}, which is not an ordinary end.",
            word(why)
        )),
        Some(_) => {}
    }
    match payload.get(REPLY_STEPS).and_then(Value::as_u64) {
        None => {
            reasons.push("The solve reply does not say how many time steps were taken.".to_owned())
        }
        Some(0) => reasons.push("The solve reply reports no time steps.".to_owned()),
        Some(steps) => {
            if let Some(want) = requested
                && steps != want
                && stop != Some(STEADY_STOP)
            {
                reasons.push(format!(
                    "The solve reply reports {steps} time steps and the request asked for {want}."
                ));
            }
        }
    }
    match payload
        .get(REPLY_MASS)
        .and_then(|mass| mass.get(REPLY_MASS_CHANGE))
        .and_then(Value::as_f64)
    {
        None => {
            reasons.push("The solve reply does not report the change of the fluid mass.".to_owned())
        }
        Some(change) if !change.is_finite() || change.abs() > MASS_CHANGE_LIMIT => {
            reasons.push(format!(
                "The fluid mass changed by {change:.3e} of itself, above the limit of {MASS_CHANGE_LIMIT:.3e}."
            ));
        }
        Some(_) => {}
    }
    if warned_unstable(payload) {
        reasons.push("The solver warned that the flow became unstable.".to_owned());
    }
}

fn check_replies(requests: &[Request], replies: &[Reply], reasons: &mut Vec<String>) {
    if requests.is_empty() {
        reasons.push("The request file holds no request.".to_owned());
        return;
    }
    for request in requests {
        match reply_to(replies, request.id) {
            None => reasons.push(format!("No reply came back for request {}.", request.id)),
            Some(reply) if reply.payload.is_none() => {
                reasons.push(format!("The reply to request {} is an error.", request.id));
            }
            Some(_) => {}
        }
    }
    let Some(solve) = solve_reply(requests, replies) else {
        reasons.push("The request file holds no solve request.".to_owned());
        return;
    };
    // A missing or error reply to the solve request is already a reason above.
    if let Some(payload) = solve {
        check_solve(payload, requested_steps(requests), reasons);
    }
}

/// The answer to the solve request: `None` when the stream holds no solve request, and
/// `Some(None)` when the reply is missing or an error.
fn solve_reply<'a>(requests: &[Request], replies: &'a [Reply]) -> Option<Option<&'a Value>> {
    let request = requests.iter().find(|request| {
        request
            .command()
            .is_some_and(|(name, _)| name == SOLVE_COMMAND)
    })?;
    Some(reply_to(replies, request.id).and_then(|reply| reply.payload.as_ref()))
}

fn check_log(log: Option<&str>, reasons: &mut Vec<String>) {
    match log {
        None => reasons.push("The solver log is missing.".to_owned()),
        Some(text) if LOG_HOLDS_TEXT && text.trim().is_empty() => {
            reasons.push("The solver log is empty.".to_owned());
        }
        Some(_) => {}
    }
}

/// Whether a run is a good one, from the files it left. `Stopped` has no meaning for a lattice
/// run: the solver cannot be stopped at a time, so a run without its end is never a good one.
pub(super) fn judge(files: &dyn RunFiles, mode: &JudgeMode) -> JudgeOutcome {
    if matches!(mode, JudgeMode::Stopped { .. }) {
        return JudgeOutcome::failed(
            "The lattice solver cannot be stopped at a time, so a stopped run is not judged as a good one.",
        );
    }
    let mut cut = false;
    let requests = take(files, COMMANDS_FILE, &mut cut).map(|text| parse_requests(&text));
    let replies = take(files, REPLIES_FILE, &mut cut).map(|text| parse_replies(&text));
    let log = take(files, LOG_FILE, &mut cut);
    let mut reasons = Vec::new();
    match (&requests, &replies) {
        (None, _) => reasons.push(
            "The request file is missing, so the replies cannot be matched to requests.".to_owned(),
        ),
        (_, None) => reasons.push("The reply file is missing.".to_owned()),
        (Some(requests), Some(replies)) => check_replies(requests, replies, &mut reasons),
    }
    check_log(log.as_deref(), &mut reasons);
    let verdict = if !reasons.is_empty() {
        "failed"
    } else if matches!(mode, JudgeMode::Smoke) {
        "smoke_passed"
    } else {
        "passed"
    };
    if cut {
        reasons.push(CUT_SENTENCE.to_owned());
    }
    JudgeOutcome {
        verdict: verdict.to_owned(),
        reasons,
    }
}

/// The values of the change measure of the steady rule, oldest first.
fn change_series(payload: &Value) -> Option<Vec<f64>> {
    match payload.get(REPLY_CHANGE)? {
        Value::Array(items) => items.iter().map(Value::as_f64).collect(),
        single => single.as_f64().map(|value| vec![value]),
    }
}

/// A steady run is converged when the last change is within the study's limits (the smaller of the
/// pressure and velocity limits, as the solver has one change measure) and the change did not rise
/// over the last tenth of the run.
fn steady_verdict(files: &dyn RunFiles, study: &ResolvedStudy) -> ConvergenceOutcome {
    let mut cut = false;
    let requests = take(files, COMMANDS_FILE, &mut cut).map(|text| parse_requests(&text));
    let replies = take(files, REPLIES_FILE, &mut cut).map(|text| parse_replies(&text));
    let payload = match (&requests, &replies) {
        (Some(requests), Some(replies)) => solve_reply(requests, replies).flatten(),
        _ => None,
    };
    let Some(payload) = payload else {
        return not_converged(
            "The solve reply is missing, so the change of the flow is not known.",
        );
    };
    let Some(series) = change_series(payload).filter(|s| !s.is_empty()) else {
        return not_converged("The solve reply carries no change measure.");
    };
    let convergence = &study.solver.convergence;
    let limit = convergence.p.min(convergence.u);
    let mut reasons = Vec::new();
    let last = series[series.len() - 1];
    if !last.is_finite() || last > limit {
        reasons.push(format!(
            "The last change of the flow is {last:.3e}, above the limit of {limit:.3e}."
        ));
    }
    // The last tenth of the run, at least two values; a series of one has no tail.
    let tenth = series.len().div_ceil(10).max(2);
    if series.len() >= 2 && last > series[series.len() - tenth] {
        reasons.push(
            "The change of the flow was still rising over the last tenth of the run.".to_owned(),
        );
    }
    ConvergenceOutcome {
        converged: reasons.is_empty(),
        reasons,
    }
}

fn not_converged(reason: &str) -> ConvergenceOutcome {
    ConvergenceOutcome {
        converged: false,
        reasons: vec![reason.to_owned()],
    }
}

/// The times and the lift coefficients of a force coefficient file, read from the view's text.
fn lift_series(text: &str) -> Option<(Vec<f64>, Vec<f64>)> {
    let mut column = None;
    let (mut times, mut lift) = (Vec::new(), Vec::new());
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(comment) = line.strip_prefix('#') {
            let mut words = comment.split_whitespace();
            if words.next() == Some("Time") {
                column = words.position(|name| name == "Cl");
            }
            continue;
        }
        let mut cells = line.split_whitespace().map(str::parse::<f64>);
        let time = cells.next()?.ok()?;
        let value = cells.nth(column?)?.ok()?;
        times.push(time);
        lift.push(value);
    }
    (!times.is_empty()).then_some((times, lift))
}

/// A transient run is converged when the lift of the first force monitor, read through the result
/// view, shows a settled shedding. The estimator is `assess_periodic` of `cfdcraft-settle`, the one
/// the benchmark comparer uses on the same series. The view feeds it, not the raw replies, so that
/// the verdict and the comparer judge one series with one reader.
fn transient_verdict(files: &dyn RunFiles, study: &ResolvedStudy) -> ConvergenceOutcome {
    let Some(force) = study.monitor_plan().forces.into_iter().next() else {
        return not_converged(
            "The study monitors no force, so there is no coefficient series to judge.",
        );
    };
    let view = View::new(Box::new(FromRun(files)), study);
    let path = format!("postProcessing/{}/0/coefficient.dat", force.fo);
    let Some((times, lift)) = view.read(&path).as_deref().and_then(lift_series) else {
        return not_converged(
            "The replies do not hold the coefficient series of the force monitor.",
        );
    };
    let samples = Samples {
        name: "Cl",
        times: &times,
        values: &lift,
    };
    let state = assess_periodic(&samples, &PeriodicOptions::default());
    let reason = match state.status {
        PeriodicStatus::Settled => {
            return ConvergenceOutcome {
                converged: true,
                reasons: Vec::new(),
            };
        }
        PeriodicStatus::Drifting => "The amplitude or the cycle length of the lift still changes.",
        PeriodicStatus::TooEarly | PeriodicStatus::TooFewSamples => {
            "The lift series holds too few whole cycles to be judged."
        }
        PeriodicStatus::Error => "The lift series cannot be read.",
    };
    not_converged(reason)
}

/// Whether the run converged: the steady rule for a run to a steady state, the periodic estimator
/// for a run in time.
pub(super) fn convergence(files: &dyn RunFiles, study: &ResolvedStudy) -> ConvergenceOutcome {
    if run_plan(study).transient {
        transient_verdict(files, study)
    } else {
        steady_verdict(files, study)
    }
}
