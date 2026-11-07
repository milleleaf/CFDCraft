//! The incremental scanner: solver log text in, events and a running state out.
//!
//! The scanner keeps the text after the last newline and waits for the rest of that line, so any
//! way of cutting a log into pieces gives the same events and the same state as the whole log.
//! A line longer than [`MAX_LINE`] bytes is skipped whole, and the maps in the state have fixed
//! sizes, so a hostile log cannot make the scanner grow.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A line longer than this many bytes is skipped whole.
pub const MAX_LINE: usize = 4096;
/// The most distinct solved fields the state follows.
pub const MAX_FIELDS: usize = 64;
/// The most characters a field name may have.
const MAX_FIELD_NAME: usize = 64;
/// How many continuity values the state keeps.
pub const LOCAL_WINDOW: usize = 64;

/// What a fatal line in the log was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FatalKind {
    /// The solver printed its own fatal error.
    FoamFatal,
    /// A floating-point trap: the solver died on an invalid number.
    FloatingPoint,
    /// The solver crashed.
    SegmentationFault,
}

/// One thing the scanner recognised in a complete line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// A new time step or iteration began.
    Time { time: f64 },
    /// One linear solve.
    Residual {
        field: String,
        initial: f64,
        final_: f64,
        iterations: u32,
    },
    /// The continuity errors of a step.
    Continuity {
        local: f64,
        global: f64,
        cumulative: f64,
    },
    /// The Courant number of a transient step.
    Courant { mean: f64, max: f64 },
    /// The solver's own clock.
    ExecutionTime { seconds: f64 },
    /// The last line of a finished run.
    End,
    /// The line a parallel run prints after `End`.
    Finalising,
    /// A fatal line (reported once).
    Fatal { kind: FatalKind },
    /// A `nan` or `inf` token (reported once).
    NonFinite,
}

/// Everything the scanner knows about the log so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogState {
    /// The newest time or iteration read.
    pub last_time: Option<f64>,
    /// How many `Time =` lines were read.
    pub iterations: u64,
    /// The first initial residual of each field.
    pub first_initial: BTreeMap<String, f64>,
    /// The latest initial residual of each field.
    pub last_initial: BTreeMap<String, f64>,
    /// The last [`LOCAL_WINDOW`] values of `sum local`, oldest first.
    pub local_errors: Vec<f64>,
    /// The smallest `sum local` seen.
    pub min_local: Option<f64>,
    /// The largest Courant maximum seen.
    pub max_courant: Option<f64>,
    /// The last line was `End` at some point.
    pub ended: bool,
    /// `Finalising parallel run` was seen.
    pub finalising: bool,
    /// The first fatal line, if any.
    pub fatal: Option<FatalKind>,
    /// A `nan` or `inf` token was seen.
    pub non_finite: bool,
    /// The latest `ExecutionTime`.
    pub execution_seconds: Option<f64>,
}

/// Reads a solver log in pieces of any size.
#[derive(Debug, Clone, Default)]
pub struct Scanner {
    partial: String,
    skipping: bool,
    state: LogState,
}

impl Scanner {
    /// A scanner that has read nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The state after everything fed so far.
    pub fn state(&self) -> &LogState {
        &self.state
    }

    /// Reads more log text. Text after the last newline waits for its newline; the events of
    /// every completed line come back in order.
    pub fn feed(&mut self, text: &str) -> Vec<Event> {
        let mut events = Vec::new();
        let mut rest = text;
        while let Some(pos) = rest.find('\n') {
            let (segment, tail) = rest.split_at(pos);
            rest = &tail[1..];
            self.finish_line(segment, &mut events);
        }
        self.keep(rest);
        events
    }

    fn finish_line(&mut self, segment: &str, events: &mut Vec<Event>) {
        if self.skipping {
            self.skipping = false;
            self.partial.clear();
            return;
        }
        if self.partial.len() + segment.len() > MAX_LINE {
            self.partial.clear();
            return;
        }
        if self.partial.is_empty() {
            self.line(segment, events);
        } else {
            self.partial.push_str(segment);
            let line = std::mem::take(&mut self.partial);
            self.line(&line, events);
        }
    }

    fn keep(&mut self, rest: &str) {
        if self.skipping {
            return;
        }
        if self.partial.len() + rest.len() > MAX_LINE {
            self.partial.clear();
            self.skipping = true;
        } else {
            self.partial.push_str(rest);
        }
    }

    fn line(&mut self, raw: &str, events: &mut Vec<Event>) {
        let line = raw.trim();
        if line.is_empty() {
            return;
        }
        if let Some(event) = self.parse(line) {
            events.push(event);
        }
        self.judge(line, events);
    }

    fn parse(&mut self, line: &str) -> Option<Event> {
        if let Some(rest) = line.strip_prefix("Time = ") {
            let time = number(rest.trim().trim_end_matches('s').trim())?;
            self.state.last_time = Some(time);
            self.state.iterations += 1;
            return Some(Event::Time { time });
        }
        if let Some((_, tail)) = line.split_once("Solving for ") {
            return self.residual(line, tail);
        }
        if line.starts_with("time step continuity errors") {
            let local = number(value_after(line, "sum local =")?)?;
            let global = number(value_after(line, "global =")?)?;
            let cumulative = number(value_after(line, "cumulative =")?)?;
            let state = &mut self.state;
            if state.local_errors.len() >= LOCAL_WINDOW {
                state.local_errors.remove(0);
            }
            state.local_errors.push(local);
            state.min_local = Some(state.min_local.map_or(local, |m| m.min(local)));
            return Some(Event::Continuity {
                local,
                global,
                cumulative,
            });
        }
        if line.starts_with("Courant Number") {
            let mean = number(value_after(line, "mean:")?)?;
            let max = number(value_after(line, "max:")?)?;
            let seen = self.state.max_courant;
            self.state.max_courant = Some(seen.map_or(max, |m| m.max(max)));
            return Some(Event::Courant { mean, max });
        }
        if line.starts_with("ExecutionTime =") {
            let seconds = number(value_after(line, "ExecutionTime =")?)?;
            self.state.execution_seconds = Some(seconds);
            return Some(Event::ExecutionTime { seconds });
        }
        if line == "End" {
            self.state.ended = true;
            return Some(Event::End);
        }
        if line == "Finalising parallel run" {
            self.state.finalising = true;
            return Some(Event::Finalising);
        }
        None
    }

    fn residual(&mut self, line: &str, tail: &str) -> Option<Event> {
        let field = tail.split(',').next()?.trim();
        if !valid_field(field) {
            return None;
        }
        let initial = number(value_after(line, "Initial residual =")?)?;
        let final_ = number(value_after(line, "Final residual =")?)?;
        let iterations = value_after(line, "No Iterations")?.parse::<u32>().ok()?;
        let state = &mut self.state;
        if !state.last_initial.contains_key(field) && state.last_initial.len() >= MAX_FIELDS {
            return None;
        }
        state
            .first_initial
            .entry(field.to_string())
            .or_insert(initial);
        state.last_initial.insert(field.to_string(), initial);
        Some(Event::Residual {
            field: field.to_string(),
            initial,
            final_,
            iterations,
        })
    }

    fn judge(&mut self, line: &str, events: &mut Vec<Event>) {
        if is_banner(line) {
            return;
        }
        if self.state.fatal.is_none()
            && let Some(kind) = fatal_kind(line)
        {
            self.state.fatal = Some(kind);
            events.push(Event::Fatal { kind });
        }
        if !self.state.non_finite && has_non_finite(line) {
            self.state.non_finite = true;
            events.push(Event::NonFinite);
        }
    }
}

/// The start-up lines that say floating-point trapping is switched on. They name the trap but
/// no trap happened, so nothing reads them as one.
pub(crate) fn is_banner(line: &str) -> bool {
    line.contains("(FOAM_SIGFPE)") && line.contains("trapping")
}

fn fatal_kind(line: &str) -> Option<FatalKind> {
    if line.contains("FOAM FATAL") {
        Some(FatalKind::FoamFatal)
    } else if line.contains("Floating point exception")
        || line.contains("signal SIGFPE")
        || line.contains("Foam::sigFpe::sigHandler")
    {
        Some(FatalKind::FloatingPoint)
    } else if line.contains("Segmentation fault") {
        Some(FatalKind::SegmentationFault)
    } else {
        None
    }
}

fn has_non_finite(line: &str) -> bool {
    const TOKENS: [&str; 8] = [
        "nan",
        "-nan",
        "+nan",
        "inf",
        "+inf",
        "-inf",
        "infinity",
        "-infinity",
    ];
    line.split(|c: char| c.is_whitespace() || ",=();[]".contains(c))
        .any(|token| TOKENS.iter().any(|t| token.eq_ignore_ascii_case(t)))
}

fn valid_field(field: &str) -> bool {
    !field.is_empty()
        && field.len() <= MAX_FIELD_NAME
        && field
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// The first word after `key`, up to a space or a comma.
fn value_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (_, after) = line.split_once(key)?;
    after
        .trim_start()
        .split(|c: char| c.is_whitespace() || c == ',')
        .next()
        .filter(|s| !s.is_empty())
}

fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}
