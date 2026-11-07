//! A registry of runs for callers that must not wait: start a run and get its id at once, poll it,
//! and stop it when the server shuts down. One run is active at a time; a second start is told the
//! host is busy, never queued.

use crate::{
    report::{HolderInfo, Report},
    run::{self, Finished, RunMode, RunRequest, infra_finished, retry_after},
};
use cfdcraft_executor::{RunError, lock};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// What a job does with the end of its run: it turns the finished run into the report that is kept
/// for polling. It runs on the job's own thread, after the case has ended.
pub type FinishStep = Box<dyn FnOnce(Finished) -> Report + Send + 'static>;

/// What `start` answers.
#[derive(Debug)]
pub enum StartOutcome {
    /// The job holds the lock and is running in the background.
    Started { run_id: String },
    /// Another job holds the host. Try again after `retry_after_seconds`.
    Busy {
        holder: Option<HolderInfo>,
        retry_after_seconds: u64,
        stale: bool,
    },
    /// The study or the setup was refused; nothing is running.
    Refused(Box<Report>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    NotFound,
    Running {
        mode: RunMode,
        elapsed_seconds: u64,
    },
    Finished {
        mode: RunMode,
        elapsed_seconds: u64,
        exit_class: u8,
    },
}

#[derive(Debug)]
pub enum ResultOutcome {
    NotFound,
    Running,
    Finished(Box<Report>),
}

/// One run the registry knows about.
struct Entry {
    mode: RunMode,
    started: Instant,
    /// The report and the seconds the run took, once it has ended.
    done: Option<(Report, u64)>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// The one job this process may have in flight. `holder` is filled once the job holds the lock.
struct Active {
    holder: Option<HolderInfo>,
}

#[derive(Default)]
struct Inner {
    active: Option<Active>,
    // Ceiling: finished reports stay in memory for the life of the server; evict old ones if a
    // server ever runs thousands of jobs.
    runs: BTreeMap<String, Entry>,
}

/// Runs started through one server. There is no queue and no waiting list: busy is an answer, not
/// a state.
#[derive(Clone, Default)]
pub struct RunRegistry {
    inner: Arc<Mutex<Inner>>,
}

fn locked(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

impl RunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the in-process guard, run the quick part of the job (seconds: connect, preflight,
    /// lock) and then the long part on a thread. Returns without waiting for the long part.
    pub fn start(&self, study_json: &str, request: RunRequest) -> StartOutcome {
        self.start_job(study_json, request, Box::new(|done| done.report))
    }

    /// As [`RunRegistry::start`], and when the case has ended `finish` makes the report that
    /// polling returns. It is not called when the job is busy or refused. A panic in it gives the
    /// same fixed report as a panic in the run.
    pub fn start_job(
        &self,
        study_json: &str,
        request: RunRequest,
        finish: FinishStep,
    ) -> StartOutcome {
        {
            let mut g = locked(&self.inner);
            if let Some(active) = &g.active {
                let holder = active.holder.clone();
                let now = lock::now_epoch().unwrap_or(0);
                let left = holder
                    .as_ref()
                    .map(|h| (h.started_epoch.saturating_add(h.cap_seconds)).saturating_sub(now));
                let retry = retry_after(left, false);
                return StartOutcome::Busy {
                    holder,
                    retry_after_seconds: retry,
                    stale: false,
                };
            }
            g.active = Some(Active { holder: None });
        }
        let begun = match run::begin(study_json, &request) {
            Ok(begun) => begun,
            Err(done) => {
                locked(&self.inner).active = None;
                let report = done.report;
                return match &report.infra {
                    Some(i) if i.code == "CFD-INFRA-001" => StartOutcome::Busy {
                        holder: i.holder.clone(),
                        retry_after_seconds: i.retry_after_seconds.unwrap_or(60),
                        stale: i.stale,
                    },
                    _ => StartOutcome::Refused(Box::new(report)),
                };
            }
        };
        let run_id = begun.run_id().to_owned();
        let mode = begun.mode();
        let holder = HolderInfo {
            run_id: run_id.clone(),
            container: run_id.clone(),
            started_epoch: lock::now_epoch().unwrap_or(0),
            cap_seconds: begun.cap_seconds(),
        };
        let started = Instant::now();
        {
            let mut g = locked(&self.inner);
            g.active = Some(Active {
                holder: Some(holder),
            });
            g.runs.insert(
                run_id.clone(),
                Entry {
                    mode,
                    started,
                    done: None,
                    cancel: request.cancel.clone(),
                    thread: None,
                },
            );
        }
        let inner = self.inner.clone();
        let id = run_id.clone();
        let handle = std::thread::spawn(move || {
            let report = catch_unwind(AssertUnwindSafe(|| finish(run::execute(begun))))
                .unwrap_or_else(|_| {
                    let stopped = RunError::Setup("the run stopped unexpectedly".into());
                    infra_finished(mode.operation(), &stopped, None, Some(&id)).report
                });
            let mut g = locked(&inner);
            if let Some(entry) = g.runs.get_mut(&id) {
                entry.done = Some((report, started.elapsed().as_secs()));
            }
            g.active = None;
        });
        if let Some(entry) = locked(&self.inner).runs.get_mut(&run_id) {
            entry.thread = Some(handle);
        }
        StartOutcome::Started { run_id }
    }

    pub fn status(&self, run_id: &str) -> Status {
        let g = locked(&self.inner);
        match g.runs.get(run_id) {
            None => Status::NotFound,
            Some(e) => match &e.done {
                None => Status::Running {
                    mode: e.mode,
                    elapsed_seconds: e.started.elapsed().as_secs(),
                },
                Some((report, seconds)) => Status::Finished {
                    mode: e.mode,
                    elapsed_seconds: *seconds,
                    exit_class: report.exit_class,
                },
            },
        }
    }

    pub fn result(&self, run_id: &str) -> ResultOutcome {
        match locked(&self.inner).runs.get(run_id) {
            None => ResultOutcome::NotFound,
            Some(Entry { done: None, .. }) => ResultOutcome::Running,
            Some(Entry {
                done: Some((report, _)),
                ..
            }) => ResultOutcome::Finished(Box::new(report.clone())),
        }
    }

    /// Stop every running job and wait up to `wait` for the threads. True when all have ended. A job
    /// that does not end in time is left to finish on its own; its lock and container are released
    /// by the job itself.
    pub fn shutdown(&self, wait: Duration) -> bool {
        let handles: Vec<JoinHandle<()>> = {
            let mut g = locked(&self.inner);
            g.runs
                .values_mut()
                .filter(|e| e.done.is_none())
                .filter_map(|e| {
                    e.cancel.store(true, Ordering::SeqCst);
                    e.thread.take()
                })
                .collect()
        };
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline && handles.iter().any(|h| !h.is_finished()) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut all = true;
        for h in handles {
            if h.is_finished() {
                let _ = h.join();
            } else {
                all = false;
            }
        }
        all
    }
}
