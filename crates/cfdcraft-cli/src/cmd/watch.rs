//! The verbs that watch a job: its monitored quantities, its balances and its verdict, and a tail
//! of its solver log; and the same table for a run that is already recorded.
//!
//! These verbs read only the files the runner has already copied (or the stored result of a
//! recorded run). They never open a connection to the compute host, so they work whether or not
//! the runner is alive. A table is a reading: it exits 0 whatever the state of the quantities.

use std::path::Path;
use std::time::Duration;

use cfdcraft_drain::watch::{MonitorsView, RowView, WatchError, log, monitors, monitors_of_run};
use cfdcraft_ops::Report;
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_queue::active;
use cfdcraft_settle::{BalanceStatus, Status, Verdict, VerdictState};
use cfdcraft_text::{Line, Text};

use super::{Error, json_with};

/// One monitored quantity as it is shown: its name, last value, tail mean, spread and state.
type Shown = (String, f64, f64, f64, Status);
/// One balance as it is shown: its state and its imbalance as a share of the reference.
type Balance = (BalanceStatus, Option<f64>);

/// A number for a table: six significant digits, in exponent form so small values stay visible.
fn num(value: f64) -> String {
    format!("{value:.5e}")
}

/// The share of the reference a balance misses, as a percentage; a dash when there is none.
fn percent(imbalance: Option<f64>) -> String {
    imbalance.map_or_else(|| "-".to_owned(), |share| format!("{:.3} %", share * 100.0))
}

/// The word for the state of a quantity, from the sentences catalog.
fn status_word(text: &Text, status: Status) -> String {
    let line = match status {
        Status::Settled => Line::MonitorSettled,
        Status::Drifting => Line::MonitorDrifting,
        Status::Oscillating => Line::MonitorOscillating,
        Status::TooEarly => Line::MonitorTooEarly,
        Status::TooFewSamples => Line::MonitorTooFewSamples,
        Status::Error => return "cannot be read".to_owned(),
    };
    text.render(&line)
}

fn balance_word(status: BalanceStatus) -> &'static str {
    match status {
        BalanceStatus::Ok => "closes",
        BalanceStatus::Failed => "does not close",
        BalanceStatus::NotApplicable => "not applicable",
    }
}

/// The sentence for a verdict.
fn verdict_line(verdict: &Verdict) -> Line {
    let reasons = verdict.reasons.join(", ");
    match verdict.state {
        VerdictState::Converged => Line::VerdictConverged,
        VerdictState::NotConverged => Line::VerdictNotConverged { reasons },
        VerdictState::NotJudged => Line::VerdictNotJudged { reason: reasons },
    }
}

/// Print one line for each quantity, then the two balances.
fn print_table(rows: &[Shown], mass: Balance, energy: Balance) {
    let text = Text::english();
    for (name, last, mean, spread, status) in rows {
        let line = Line::MonitorLine {
            name: name.clone(),
            last: num(*last),
            mean: num(*mean),
            spread: num(*spread),
            state: status_word(&text, *status),
        };
        println!("{}", text.render(&line));
    }
    let state = balance_word(mass.0).to_owned();
    let imbalance = percent(mass.1);
    println!(
        "{}",
        text.render(&Line::MassBalanceLine { state, imbalance })
    );
    let state = balance_word(energy.0).to_owned();
    let imbalance = percent(energy.1);
    println!(
        "{}",
        text.render(&Line::EnergyBalanceLine { state, imbalance })
    );
}

/// The rows of a table: the study's own name for a quantity when it has one.
fn shown(rows: &[RowView]) -> Vec<Shown> {
    rows.iter()
        .map(|r| {
            let name = r.label.clone().unwrap_or_else(|| r.name.clone());
            (name, r.last, r.mean, r.spread, r.state)
        })
        .collect()
}

/// What a job's table shows right now.
fn show(view: &MonitorsView, json: bool) -> Result<(), Error> {
    if json {
        let data = serde_json::to_value(view)?;
        println!(
            "{}",
            json_with(&Report::new("queue-watch"), "monitors", data)?
        );
        return Ok(());
    }
    println!("{}  {}  {}", view.job, view.state.as_str(), view.iterations);
    print_table(
        &shown(&view.rows),
        (view.mass.status, view.mass.imbalance),
        (view.energy.status, view.energy.imbalance),
    );
    println!("{}", Text::english().render(&verdict_line(&view.verdict)));
    Ok(())
}

/// The job to watch when none was named: the one that is active, else the newest.
fn default_job(doc: &Path) -> Result<String, Error> {
    let text = Text::english();
    let queue = QueueFile::new(doc).read().map_err(|e| {
        text.render(&Line::QueueDamaged {
            reason: e.to_string(),
        })
    })?;
    let number = |id: &str| id.strip_prefix("q-").and_then(|n| n.parse::<u64>().ok());
    let found = active(&queue).or_else(|| queue.jobs.iter().max_by_key(|job| number(&job.id)));
    found
        .map(|job| job.id.clone())
        .ok_or_else(|| "there are no jobs in the queue".into())
}

/// Whether the queue says the job has ended. A job it does not list has not ended.
fn has_ended(doc: &Path, job: &str) -> bool {
    QueueFile::new(doc).read().is_ok_and(|queue| {
        queue
            .jobs
            .iter()
            .any(|j| j.id == job && j.state.is_terminal())
    })
}

/// Print the table of a job; with `follow`, again every `poll` seconds until the job has ended.
pub fn watch(
    doc: &Path,
    job: Option<&str>,
    follow: bool,
    poll: u64,
    json: bool,
) -> Result<i32, Error> {
    let job = match job {
        Some(job) => job.to_owned(),
        None => default_job(doc)?,
    };
    loop {
        match monitors(doc, &job) {
            Ok(view) => {
                show(&view, json)?;
                if !follow || view.state.is_terminal() {
                    return Ok(0);
                }
            }
            // A job that is still to start has no files yet; following waits for them.
            Err(WatchError::NoMirror { .. }) if follow && !has_ended(doc, &job) => {}
            Err(error) => return Err(error.to_string().into()),
        }
        std::thread::sleep(Duration::from_secs(poll));
    }
}

/// Print a piece of the solver log of a job, with host paths hidden, and where the next piece
/// begins. In text the piece is on standard output and the offset on standard error.
pub fn log_of(doc: &Path, job: &str, offset: u64, lines: usize, json: bool) -> Result<i32, Error> {
    let view = log(doc, job, offset, lines).map_err(|e| e.to_string())?;
    if json {
        let data = serde_json::to_value(&view)?;
        println!("{}", json_with(&Report::new("queue-log"), "log", data)?);
    } else {
        if !view.text.is_empty() {
            println!("{}", view.text);
        }
        eprintln!("next offset: {}", view.next_offset);
    }
    Ok(0)
}

/// The table of a recorded run, made again from its stored result under the settings it was made
/// with, and shown next to the verdict that was recorded with it.
fn recorded(doc: &Path, run: &str, json: bool) -> Result<i32, Error> {
    let view = monitors_of_run(doc, run).map_err(|e| e.to_string())?;
    if json {
        let data = serde_json::to_value(&view)?;
        println!("{}", json_with(&Report::new("monitors"), "monitors", data)?);
        return Ok(0);
    }
    print_table(
        &shown(&view.rows),
        (view.mass.status, view.mass.imbalance),
        (view.energy.status, view.energy.imbalance),
    );
    let text = Text::english();
    println!("recomputed: {}", text.render(&verdict_line(&view.verdict)));
    match &view.recorded {
        Some(verdict) => println!("recorded: {}", text.render(&verdict_line(verdict))),
        None => println!("recorded: none was recorded with this run"),
    }
    Ok(0)
}

/// The `monitors` verb: the table of a recorded run, or of a job as `watch` shows it.
pub fn table(doc: &Path, run: Option<&str>, job: Option<&str>, json: bool) -> Result<i32, Error> {
    match (run, job) {
        (Some(_), Some(_)) => Err("ask for a run or for a job, not for both at once".into()),
        (Some(run), None) => recorded(doc, run, json),
        (None, job) => watch(doc, job, false, 0, json),
    }
}
