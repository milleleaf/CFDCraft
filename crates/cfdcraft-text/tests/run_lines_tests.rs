use cfdcraft_text::{Line, Text};

fn s(v: &str) -> String {
    v.to_string()
}

fn english(line: &Line) -> String {
    Text::english().render(line)
}

#[test]
fn a_job_line_renders_in_english() {
    let out = english(&Line::JobStopped {
        job: s("q-003"),
        run: s("run-4"),
        time: s("1200"),
    });
    assert_eq!(
        out,
        "q-003 stopped at time 1200 and was recorded as run-4; it can be continued."
    );
}

#[test]
fn the_queue_sentences_render() {
    let job = || s("q-003");
    let cases: Vec<(Line, &str)> = vec![
        (
            Line::QueueAdded {
                jobs: s("3"),
                queued: s("3"),
            },
            "Added 3 jobs to the queue; 3 are waiting.",
        ),
        (
            Line::QueueNothingToAdd,
            "Nothing to add: everything asked for is up to date or already queued.",
        ),
        (
            Line::QueueRefused {
                reason: s("the queue is full"),
            },
            "The queue request was refused: the queue is full",
        ),
        (
            Line::QueueUnknownJob { job: s("q-009") },
            "There is no job q-009 in the queue.",
        ),
        (
            Line::QueueNotAllowed {
                job: job(),
                state: s("running"),
                action: s("reordered"),
            },
            "Job q-003 is running, so it cannot be reordered.",
        ),
        (
            Line::QueueReordered {
                job: s("q-004"),
                position: s("1"),
            },
            "Moved q-004 to position 1 among the waiting jobs.",
        ),
        (Line::QueueCancelled { job: s("q-005") }, "Cancelled q-005."),
        (
            Line::QueueStopRequested { job: job() },
            "Asked q-003 to write its latest results and stop.",
        ),
        (
            Line::QueueDrainStarted,
            "Started the queue runner; it keeps running if you close this program.",
        ),
        (
            Line::QueueDrainRunning,
            "The queue runner is already running.",
        ),
        (
            Line::QueueDamaged {
                reason: s("a line is not valid"),
            },
            "The queue file cannot be used: a line is not valid",
        ),
        (Line::JobQueued { job: job() }, "q-003 is waiting."),
        (
            Line::JobStarting { job: job() },
            "q-003 is starting on the compute host.",
        ),
        (
            Line::JobRunning {
                job: job(),
                line: s("Time = 120"),
            },
            "q-003 is running: Time = 120",
        ),
        (
            Line::JobStopping {
                job: job(),
                cause: s("stop requested"),
            },
            "q-003 is stopping (stop requested).",
        ),
        (
            Line::JobFinished {
                job: job(),
                run: s("run-4"),
            },
            "q-003 finished and was recorded as run-4.",
        ),
        (
            Line::JobFailed {
                job: job(),
                reason: s("the solver exited with an error"),
            },
            "q-003 failed: the solver exited with an error",
        ),
        (
            Line::JobCancelled { job: s("q-005") },
            "q-005 was cancelled before it started.",
        ),
        (
            Line::JobInterrupted {
                job: job(),
                reason: s("the queue runner was stopped"),
            },
            "q-003 was interrupted: the queue runner was stopped",
        ),
        (
            Line::JobUnreachable { job: job() },
            "The compute host cannot be reached; q-003 keeps running there and will be picked up again.",
        ),
        (
            Line::JobReattached { job: job() },
            "Picked up q-003 again where it left off.",
        ),
        (
            Line::ContinueFrom {
                run: s("run-4"),
                time: s("1200"),
            },
            "Continuing run-4 from time 1200.",
        ),
        (
            Line::ContinueRefused {
                reason: s("it is not a stopped run"),
            },
            "The run cannot be continued: it is not a stopped run",
        ),
        (
            Line::ParallelRun {
                np: s("4"),
                cells: s("40000"),
            },
            "Running on 4 processes (40000 cells).",
        ),
        (
            Line::StoppedRunNote {
                run: s("run-4"),
                time: s("1200"),
            },
            "run-4 is a stopped run (latest time 1200); it holds no finished result.",
        ),
    ];
    assert_eq!(cases.len(), 25, "JobStopped is covered by its own test");
    for (line, want) in &cases {
        assert_eq!(&english(line), want);
    }
}

#[test]
fn a_value_that_looks_like_a_placeholder_is_inserted_as_written() {
    let out = english(&Line::QueueRefused { reason: s("{job}") });
    assert_eq!(out, "The queue request was refused: {job}");
}

#[test]
fn the_samples_still_cover_every_variant() {
    // The 53 sentences of the run phase are the last samples; each has its own variant and source.
    let samples = Line::samples();
    let new = &samples[samples.len() - 53..];
    let sources: std::collections::BTreeSet<&str> = new.iter().map(|l| l.source()).collect();
    assert_eq!(sources.len(), new.len());
}

#[test]
fn a_divergence_sentence_names_the_field_and_the_limit() {
    let out = english(&Line::RunDiverged {
        code: s("CFD-RUN-004"),
        field: s("Ux"),
        value: s("5200"),
        limit: s("1e4"),
    });
    assert_eq!(
        out,
        "The run diverged (CFD-RUN-004): Ux reached 5200, over the limit 1e4. It was stopped without writing the unstable fields."
    );
}

#[test]
fn each_known_solver_error_has_its_own_sentence_with_a_fix() {
    let code = || s("CFD-SOLVER-001");
    let lines = vec![
        Line::SolverMissingFile { code: code() },
        Line::SolverNumericalBlowup { code: code() },
        Line::SolverUnknownType { code: code() },
        Line::SolverMissingKeyword { code: code() },
        Line::SolverLinearNotConverged { code: code() },
        Line::SolverPoorMesh { code: code() },
        Line::SolverOutOfMemory { code: code() },
        Line::SolverDiskFull { code: code() },
        Line::SolverDecomposition { code: code() },
        Line::SolverNotEnoughSlots { code: code() },
        Line::SolverSegfault { code: code() },
    ];
    let mut seen = std::collections::BTreeSet::new();
    for line in &lines {
        let out = english(line);
        assert!(out.contains("CFD-SOLVER-001"), "{out}");
        assert!(out.ends_with('.'), "{out}");
        // What happened, then at least one more sentence saying what to do.
        assert!(out.matches(". ").count() >= 1, "{out}");
        assert!(seen.insert(out), "two errors share a sentence");
    }
}

#[test]
fn the_unknown_solver_error_shows_the_tail_it_is_given() {
    let out = english(&Line::SolverUnknown {
        tail: s("cannot open file | FOAM exiting"),
    });
    assert_eq!(
        out,
        "The solver stopped with an error this program does not recognise (CFD-SOLVER-099). The last lines of its log:\ncannot open file | FOAM exiting\nCheck the setup and the mesh, then run it again."
    );
}

#[test]
fn a_monitor_row_reads_with_its_numbers() {
    let out = english(&Line::MonitorLine {
        name: s("probe_1.Ux"),
        last: s("0.0123"),
        mean: s("0.0121"),
        spread: s("0.0004"),
        state: s("settled"),
    });
    assert_eq!(
        out,
        "probe_1.Ux: last 0.0123, tail mean 0.0121, spread 0.0004: settled."
    );
}

#[test]
fn the_balance_and_verdict_sentences_render() {
    let cases: Vec<(Line, &str)> = vec![
        (
            Line::DiskBelowFloor {
                free: s("8"),
                floor: s("20"),
            },
            "The compute host has 8 GB free, below its floor of 20 GB, so the run was not started.",
        ),
        (
            Line::DiskStopped {
                free: s("19"),
                floor: s("20"),
            },
            "The run was stopped because free disk on the compute host fell to 19 GB, the floor being 20 GB. Its latest results were kept.",
        ),
        (Line::MonitorSettled, "settled"),
        (Line::MonitorDrifting, "drifting"),
        (Line::MonitorOscillating, "oscillating"),
        (Line::MonitorTooEarly, "too early to tell"),
        (Line::MonitorTooFewSamples, "too few samples to tell"),
        (
            Line::MassBalanceLine {
                state: s("closes"),
                imbalance: s("0.2 %"),
            },
            "Mass balance: closes (imbalance 0.2 % of the inlet flux).",
        ),
        (
            Line::EnergyBalanceLine {
                state: s("does not close"),
                imbalance: s("7 %"),
            },
            "Energy balance: does not close (imbalance 7 % of the reference).",
        ),
        (
            Line::VerdictConverged,
            "Converged: every monitored quantity has settled and the balances close.",
        ),
        (
            Line::VerdictNotConverged {
                reasons: s("monitor probe_1 drifting"),
            },
            "Not converged: monitor probe_1 drifting.",
        ),
        (
            Line::VerdictNotJudged {
                reason: s("no monitors are defined"),
            },
            "Convergence cannot be judged: no monitors are defined.",
        ),
        (
            Line::ReasonMonitor {
                id: s("probe_1"),
                from: s("none"),
                to: s("x 0.05 m, y 0.05 m, velocity"),
            },
            "monitor probe_1 none → x 0.05 m, y 0.05 m, velocity",
        ),
    ];
    for (line, want) in &cases {
        assert_eq!(&english(line), want);
    }
}

#[test]
fn no_sentence_names_a_path_or_a_host() {
    let samples = Line::samples();
    for line in &samples[samples.len() - 53..] {
        let out = english(line);
        for bad in ["/data", "ssh", ".ssh", "@"] {
            assert!(!out.contains(bad), "{out}");
        }
    }
}
