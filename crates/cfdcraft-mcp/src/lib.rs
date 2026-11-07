//! An MCP server over stdio that offers forty-three typed operations and nothing else: create and
//! validate a study, compile and export it, save, open, pack and unpack a document, start, poll
//! and collect a run, start a job on a document (solve it or one of its variants, rerun a recorded
//! result, or update its mesh or its result), and read what a document records: whether it is up
//! to date and why not, its runs, the checks and key numbers of a run, what a run was made from,
//! how two runs compare and what an update would do. Variants of a document are listed, shown,
//! diffed, created, changed and labelled; sweeps of a document are planned, stored and checked
//! before anything is solved; benchmark studies are listed and made as text, a recorded run is checked
//! against the published reference of its benchmark, and a mesh independence sweep is stored in a
//! document and its grid convergence index read from the recorded results; disk use is read per
//! variant, and stored results are pruned only by the one tool that does it, which shows a plan
//! unless it is told to apply it. The solver backends are listed, a case folder of the workspace is
//! imported as a read-only copy, and the parity table of a benchmark is read from the recorded
//! runs of a document; compile, run and queue jobs may name the backend to use. A solve is put in
//! the document's queue and a background runner, which outlives the server, advances it; the queue
//! is listed, added to, reordered and cancelled, a running job is asked to stop, and its monitors
//! and log are read from the copies the runner keeps on this machine. The reading operations write
//! nothing and start nothing. There is no shell and no free-form dictionary
//! access. Every argument is a name, a mode, a job word, a run id, a flag, a list of changes to
//! settings, a sweep or a study; none is a path, a host, a key or a command. Those are settings of
//! the server itself, given once on the command line.
//!
//! Standard output carries protocol messages only; anything else goes to standard error.

use cfdcraft_drain::{
    finalize::read_report,
    spawn_detached,
    watch::{self, BalanceView, RowView, WatchError},
};
use cfdcraft_model::Diagnostic;
use cfdcraft_ops::{
    AddRequest, BenchmarkOptions, DocJobKind, ExitClass, HolderInfo, JobView, Report, Repro,
    ResultOutcome, RunMode, RunRegistry, ServerSettings, Stage, StartOutcome, Status, Workspace,
    cases::cli_problem, queue_file::QueueFile, scenario_ops::LabelUpdate,
};
use cfdcraft_text::{Line, Text};
use rmcp::{
    ErrorData, Json, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, OnceLock},
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, ReadBuf};

/// How long shutdown waits for the server's own job to stop its container and give up the lock.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(60);

/// The process exit status when the server stopped because standard input could not be read. It is
/// apart from the operation exit classes 0 to 4 and from the status 2 that any other server error
/// ends with.
pub const INPUT_FAILURE_EXIT: i32 = 5;

/// Standard input that remembers whether a read failed. The protocol library reports a failed read
/// and a clean end of file the same way (the input just ends), so this is the one place to tell
/// them apart.
struct WatchedInput {
    inner: tokio::io::Stdin,
    failed: Arc<OnceLock<io::ErrorKind>>,
}

impl AsyncRead for WatchedInput {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Err(e)) = &r {
            let _ = self.failed.set(e.kind());
        }
        r
    }
}

/// A unix socket path must stay under this many bytes (the same bound the tunnel enforces).
const SOCKET_PATH_LIMIT: usize = 100;

/// Where the server lives and which compute host it uses. Built by the command line once.
#[derive(Debug, Clone)]
pub struct McpConfig {
    pub workspace: PathBuf,
    pub key: PathBuf,
    pub hosts: PathBuf,
    pub profile: String,
    /// Whether a solve is put in the document's queue for a background runner, which goes on when
    /// this server stops (true), or stays attached to this server and stops with it (false).
    pub queued_jobs: bool,
}

struct State {
    workspace: Workspace,
    registry: RunRegistry,
    settings: ServerSettings,
}

impl State {
    /// Every failure names the missing thing and never prints the key's location.
    fn open(config: &McpConfig) -> Result<Self, String> {
        if !config.hosts.is_file() {
            return Err("the hosts file is missing".into());
        }
        // The loader's own error can quote the file, which names the host, so it is not shown.
        cfdcraft_executor::HostProfile::load(&config.hosts, &config.profile)
            .map_err(|_| "the host profile cannot be loaded".to_owned())?;
        let workspace = Workspace::open(&config.workspace)?;
        let root = config
            .workspace
            .canonicalize()
            .map_err(|e| format!("the workspace cannot be opened: {e}"))?;
        let socket_dir = root.join("sockets");
        // The longest socket this server will ask for: its directory, a name with a 7-digit
        // process id and an 8-digit tag, and the socket file.
        let longest = socket_dir.join("socket-9999999-ffffffff").join("d.sock");
        if longest.as_os_str().len() >= SOCKET_PATH_LIMIT {
            return Err(format!(
                "the workspace path is too long for the tunnel socket: use a path under {} characters",
                SOCKET_PATH_LIMIT - 40
            ));
        }
        let settings = ServerSettings::new(
            config.key.clone(),
            config.hosts.clone(),
            &config.profile,
            socket_dir,
        )?
        .with_queue(config.queued_jobs);
        Ok(Self {
            workspace,
            registry: RunRegistry::new(),
            settings,
        })
    }
}

// ---- tool arguments: closed, and none of them a path, host, key or command ----

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateStudyArgs {
    /// The name to store the study under: 1 to 40 lower-case letters, digits or hyphens.
    name: String,
    /// The study, as the published study schema describes it. It is checked before it is stored.
    #[schemars(with = "cfdcraft_model::Study")]
    study: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NameArgs {
    /// The name of a study in the workspace. For `open_document`, `pack_document` and
    /// `unpack_document` it is the name of a document or pack held by the server.
    name: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CompileArgs {
    /// The name of a study in the workspace.
    name: String,
    /// The id of the backend to compile for, from `backend_list`. Left out, the default backend.
    #[serde(default)]
    backend: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ImportCaseArgs {
    /// The name of a case folder in the workspace, below its top folder: a plain relative name
    /// with no `..` and no link. It is read and never changed.
    source: String,
    /// The name to import it under: 1 to 40 lower-case letters, digits or hyphens. The case is
    /// kept as `<name>.cfdcase` and an existing one is never replaced.
    name: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ParityTableArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The benchmark to compare the backends on, from `benchmark_list`.
    benchmark: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StatusArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of a variant of the document, from `scenario_list`. Without it, or with `base`, the
    /// status is that of the study itself.
    #[serde(default)]
    scenario: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScenarioArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of a variant of the document, from `scenario_list`, or `base` for the study itself.
    scenario: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DiffArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of the variant to start from, or `base` for the study itself.
    a: String,
    /// The id of the variant to compare with it, or `base` for the study itself.
    b: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScenarioSetArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of the variant to create or change: a plain name (1 to 40 lower-case letters, digits
    /// or hyphens), never `base`, which is the study itself.
    scenario: String,
    /// For a new variant: the id of the variant it descends from. Without it the variant descends
    /// from the study itself (`base`). An existing variant keeps its parent.
    #[serde(default)]
    parent: Option<String>,
    /// The changes to make, at most 200 in all for a variant. A `set` puts a value at a `setting`
    /// and a `remove` takes the member away. A `setting` is a JSON pointer into the study such as
    /// `/boundary_conditions/lid/velocity/value`; a raw override of a solver entry is set at
    /// `/raw_overrides/<label>`. A change to a setting the variant already changes replaces it.
    overrides: Vec<cfdcraft_scenarios::Override>,
    /// A display title for the variant. An empty title is ignored here; clear one with `scenario_label`.
    #[serde(default)]
    title: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScenarioLabelArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of the variant to label, or `base` for the study itself.
    scenario: String,
    /// The new display title. An empty title clears it; left out, the title is kept.
    #[serde(default)]
    title: Option<String>,
    /// Whether pruning keeps the stored results of the variant. Left out, the pin is kept.
    #[serde(default)]
    pinned: Option<bool>,
    /// The new notes. Empty notes clear them; left out, the notes are kept.
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SweepArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The sweep: an id, the variant it starts from (`base` for the study itself) and the
    /// parameters it varies. Every combination of the parameters is one point, at most 200 points.
    sweep: cfdcraft_sweep::Sweep,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SweepOfArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of a sweep stored in the document, from `sweep_create`.
    sweep: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BenchmarkCreateArgs {
    /// The benchmark template to make a study of, from `benchmark_list`: `lid-cavity`,
    /// `backward-step`, `backward-step-laminar`, `cylinder` or `heated-cavity`.
    benchmark: String,
    /// The mesh preset: `coarse`, `medium` (the default) or `fine`.
    #[serde(default)]
    mesh: Option<String>,
    /// The turbulence model: `laminar`, `k-omega-sst`, `realizable-ke` or `rng-k-epsilon`. Left
    /// out, the template's own model is used.
    #[serde(default)]
    turbulence: Option<String>,
    /// `steady` or `transient`. Left out, the template's own choice is used.
    #[serde(default)]
    time: Option<String>,
    /// The time step at the coarse mesh, in seconds, above zero. For a transient study only.
    #[serde(default)]
    delta_t: Option<f64>,
    /// How the fields start: `uniform` or `potential`.
    #[serde(default)]
    init: Option<String>,
    /// The Rayleigh number, above zero. The heated cavity only.
    #[serde(default)]
    rayleigh: Option<f64>,
    /// The Reynolds number, above zero. The laminar step only.
    #[serde(default)]
    reynolds: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BenchmarkCheckArgs {
    /// The name of a document in the workspace.
    name: String,
    /// A run id from `list_runs`. Without it the newest run that recorded a result and is not a
    /// rerun is used.
    #[serde(default)]
    run: Option<String>,
    /// The benchmark the study must be, from `benchmark_list`. Without it the study selects its own.
    #[serde(default)]
    benchmark: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GciCreateArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of the sweep to store: a plain name, 1 to 40 lower-case letters, digits or hyphens.
    /// Its points are called `<sweep>-001`, `<sweep>-002`, and so on.
    sweep: String,
    /// The id of the variant the points descend from, or `base` (the default) for the study itself.
    #[serde(default)]
    scenario: Option<String>,
    /// The mesh levels, three to five, each above the one before and at most 4. Left out, 0, 1
    /// and 2.
    #[serde(default)]
    levels: Option<Vec<u32>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct GciReportArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of a mesh independence sweep stored in the document, from `gci_create`.
    sweep: String,
    /// The name of the key number to analyse, as `get_run_result` lists it, such as
    /// `bench.cylinder.st`.
    quantity: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PruneArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The ids of the variants to prune, from `scenario_list`; `base` is the study itself. Without
    /// it every variant is considered. A result a variant outside the list also needs is kept.
    #[serde(default)]
    scenarios: Option<Vec<String>>,
    /// Without it the call only shows the plan and removes nothing. With it the stored results in
    /// the plan are removed, and that cannot be undone.
    #[serde(default)]
    apply: bool,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Mode {
    /// Run the case and judge the evidence.
    Run,
    /// Run the case, judge the evidence, then compare it with the reference.
    Validate,
    /// A few iterations that show the case loads and iterates; never a validated result.
    Smoke,
}

impl From<Mode> for RunMode {
    fn from(m: Mode) -> Self {
        match m {
            Mode::Run => Self::Run,
            Mode::Validate => Self::Validate,
            Mode::Smoke => Self::Smoke,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StartRunArgs {
    /// The name of a study in the workspace.
    name: String,
    /// What the run is for.
    mode: Mode,
    /// Run the stored export of the study instead of a fresh one; it must match the study exactly.
    #[serde(default)]
    from_export: bool,
    /// The id of the backend to run on, from `backend_list`. Left out, the default backend. A
    /// backend whose image is not pinned yet is refused before anything starts.
    #[serde(default)]
    backend: Option<String>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum JobWord {
    /// Make whatever mesh and result of the document is out of date and record the run.
    Solve,
    /// Run the settings a recorded run was made from again and compare with that run. Needs `run`.
    Rerun,
    /// Make the mesh of the document, which must be out of date.
    UpdateMesh,
    /// Make the result of the document, which must be out of date and stand on an up to date mesh.
    UpdateResult,
}

impl JobWord {
    /// The name of the operation the job reports as.
    fn operation(self) -> &'static str {
        match self {
            Self::Solve => "solve",
            Self::Rerun => "rerun",
            Self::UpdateMesh | Self::UpdateResult => "update",
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StartDocumentJobArgs {
    /// The name of a document in the workspace.
    name: String,
    /// What to do with the document.
    job: JobWord,
    /// For `rerun` only: the id of the recorded run to repeat, from `list_runs`. Any other job
    /// takes no run.
    #[serde(default)]
    run: Option<String>,
    /// For `solve` only: the id of the variant to solve, from `scenario_list`. Without it the study
    /// itself is solved.
    #[serde(default)]
    scenario: Option<String>,
    /// For `solve` only: make a result again that was pruned on purpose. Left off, a pruned result
    /// is not out of date and nothing is run for it.
    #[serde(default)]
    remake_pruned: bool,
}

impl StartDocumentJobArgs {
    /// What the words ask for, or the report that says the combination is not one.
    fn kind(&self) -> Result<DocJobKind, Box<Report>> {
        let refused = |message: &str| {
            let found = cli_problem(message.to_owned());
            Box::new(Report::from_diagnostics(
                self.job.operation(),
                vec![found],
                None,
            ))
        };
        let run_text = "A rerun needs the run to repeat; the other jobs take no run.";
        let solve_text = "A variant and remake_pruned belong to the solve job only.";
        if (self.scenario.is_some() || self.remake_pruned) && !matches!(self.job, JobWord::Solve) {
            return Err(refused(solve_text));
        }
        match (self.job, &self.run) {
            (JobWord::Rerun, Some(run)) => Ok(DocJobKind::Rerun { run: run.clone() }),
            (JobWord::Rerun, None) | (_, Some(_)) => Err(refused(run_text)),
            (JobWord::Solve, None) if self.scenario.is_some() || self.remake_pruned => {
                Ok(DocJobKind::SolveScenario {
                    scenario: self.scenario.clone(),
                    remake_pruned: self.remake_pruned,
                })
            }
            (JobWord::Solve, None) => Ok(DocJobKind::Solve),
            (JobWord::UpdateMesh, None) => Ok(DocJobKind::Item(Stage::Mesh)),
            (JobWord::UpdateResult, None) => Ok(DocJobKind::Item(Stage::Result)),
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RunIdArgs {
    /// The run id that `start_run` or `start_document_job` returned. A queued solve returns a job
    /// id such as `q-001`.
    run_id: String,
    /// For a job id such as `q-001`: the name of its document. Every document numbers its jobs from
    /// 1, so without it the id is looked for in every document and counts only when exactly one has
    /// it. A run id from `start_run` takes no document.
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RunOfArgs {
    /// The name of a document in the workspace.
    name: String,
    /// A run id from `list_runs`. Without it the newest suitable run is used.
    #[serde(default)]
    run: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CompareArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The run id of the original.
    a: String,
    /// The run id of the run compared with it.
    b: String,
    /// How far two numbers may differ in absolute terms and still count as the same. Not negative.
    /// The default is tight, for a rerun in the same image.
    #[serde(default)]
    abs_tolerance: Option<f64>,
    /// How far two numbers may differ relative to their size and still count as the same. Not
    /// negative. The default is tight, for a rerun in the same image.
    #[serde(default)]
    rel_tolerance: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlanUpdateArgs {
    /// The names of the documents to look at: at most 16.
    names: Vec<String>,
    /// The items to update, each written `name:mesh` or `name:result`. Without it every item that
    /// is out of date is chosen.
    #[serde(default)]
    items: Option<Vec<String>>,
    /// The order to update the chosen items in, each written `name:mesh` or `name:result`. It must
    /// list each chosen item once and keep every mesh before the result made from it. Without it
    /// the items go document by document, the mesh first.
    #[serde(default)]
    order: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueueAddArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The id of a variant to solve, from `scenario_list`, or `base` for the study itself. Give at
    /// most one of `scenario`, `sweep` and `run`; with none, the study itself is queued.
    #[serde(default)]
    scenario: Option<String>,
    /// The id of a sweep stored in the document, from `sweep_create`: every point of it is queued,
    /// in point order.
    #[serde(default)]
    sweep: Option<String>,
    /// The id of a stopped run, from `list_runs`: its saved state is continued.
    #[serde(default)]
    run: Option<String>,
    /// Make a result again that was pruned on purpose. Left off, a pruned result is not out of date
    /// and nothing is queued for it.
    #[serde(default)]
    remake_pruned: bool,
    /// The id of the backend the jobs run on, from `backend_list`. Left out, the default backend.
    #[serde(default)]
    backend: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct JobArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The job id from `queue_list`, such as `q-001`.
    job: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct QueueReorderArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The job id from `queue_list`, such as `q-001`. It must be waiting.
    job: String,
    /// The place among the waiting jobs to move it to, counted from 1; a number past the ends is
    /// the nearest end.
    position: u32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MonitorsArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The job id from `queue_list`, such as `q-001`. Without `job` and `run`, the active job is
    /// shown, or the newest when none is active.
    #[serde(default)]
    job: Option<String>,
    /// The id of a recorded run, from `list_runs`: its table is made again from its stored result.
    /// Give at most one of `job` and `run`.
    #[serde(default)]
    run: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LogArgs {
    /// The name of a document in the workspace.
    name: String,
    /// The job id from `queue_list`, such as `q-001`.
    job: String,
    /// The byte to read from, as `next_offset` of the earlier call said. Without it, the start.
    #[serde(default)]
    offset: Option<u64>,
    /// How many lines to return: 1 to 200. Without it, 40.
    #[serde(default)]
    lines: Option<u32>,
}

// ---- tool results ----

/// A benchmark study made as text, with the report of the checks it passed.
#[derive(Serialize, JsonSchema)]
struct BenchmarkStudy {
    /// The study, as the text of a study file. Absent when the request was refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    study: Option<String>,
    /// The findings; a refusal is exit class 2.
    report: Report,
}

#[derive(Serialize, JsonSchema)]
struct StartRunOutput {
    /// `started`, `busy`, `refused` or `nothing_to_run`. A start never waits for the job. A solve
    /// is put in the document's queue and the answer is its job id; the other jobs and `start_run`
    /// run at once on the compute host and are never queued. `nothing_to_run` is a start of a
    /// document job that found nothing out of date: the report holds the data, `message` says so
    /// in words, and nothing started.
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    /// Who holds the compute host, when busy.
    #[serde(skip_serializing_if = "Option::is_none")]
    holder: Option<HolderInfo>,
    /// When busy: seconds to wait before asking again.
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_seconds: Option<u64>,
    /// When busy: the holder is far past its own time cap. Reported only; nothing is broken.
    stale: bool,
    /// When refused: the reasons, in the same structure every operation returns. Also present on a
    /// started solve whose job is queued but whose background runner could not be started.
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Report>,
    /// The sentence for a person: present when the status is `nothing_to_run`, absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct RunStatusOutput {
    /// `running`, `finished` or `not_found`. For a queue job id it is the state of the job:
    /// `queued`, `starting`, `running`, `stopping`, `finishing`, `unreachable`, `finished`,
    /// `stopped`, `failed`, `cancelled` or `interrupted`.
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    elapsed_seconds: Option<u64>,
    /// Once finished: the class of the finished report. Judge a run by the report's
    /// `evidence.verdict`, not by this alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_class: Option<u8>,
}

#[derive(Serialize, JsonSchema)]
struct GetResultOutput {
    /// `running`, `finished` or `not_found`. A queue job that has not ended is `running`, whatever
    /// its state; use `run_status` for the state.
    status: String,
    /// Once a run started by `start_run` or by an attached job has finished: the full report.
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Report>,
    /// Once a queue job has ended: how it ended. The run it recorded is read with `get_run_result`.
    #[serde(skip_serializing_if = "Option::is_none")]
    job: Option<JobResult>,
}

#[derive(Serialize, JsonSchema)]
struct JobResult {
    /// The job id.
    job: String,
    /// `finished`, `stopped`, `failed`, `cancelled` or `interrupted`.
    outcome: String,
    /// The run the document recorded, for a finished or stopped job.
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<String>,
    /// The stable code of a failed or interrupted job.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    /// The sentences that say how it ended, with host paths and run names hidden.
    lines: Vec<String>,
}

/// One monitored quantity of a job or a recorded run.
#[derive(Serialize, JsonSchema)]
struct MonitorRow {
    /// The name in the case, such as `probe_1.Ux`.
    name: String,
    /// The study's own name for the monitor, with the same ending, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    last: f64,
    tail_mean: f64,
    spread: f64,
    /// `settled`, `drifting`, `oscillating`, `too_early`, `too_few_samples` or `error`.
    state: String,
}

#[derive(Serialize, JsonSchema)]
struct BalanceOut {
    /// `ok`, `failed` or `not_applicable`.
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    imbalance: Option<f64>,
}

#[derive(Serialize, JsonSchema)]
struct VerdictOut {
    /// `converged`, `not_converged` or `not_judged`.
    state: String,
    /// Why the verdict is not converged, as stable words such as `not_settled:probe_1.Ux`.
    reasons: Vec<String>,
}

#[derive(Serialize, JsonSchema, Default)]
struct MonitorsOutput {
    /// `ok`, or `refused` with a `code` and a `message`.
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    job: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<String>,
    /// For a job: its state.
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iterations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_time: Option<f64>,
    /// Whether the solver log has its end line.
    #[serde(skip_serializing_if = "Option::is_none")]
    ended: Option<bool>,
    rows: Vec<MonitorRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mass_balance: Option<BalanceOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    energy_balance: Option<BalanceOut>,
    /// The verdict made from the files as they stand. It is a reading, never a failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    verdict: Option<VerdictOut>,
    /// For a recorded run: the verdict recorded with it, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    recorded_verdict: Option<VerdictOut>,
}

#[derive(Serialize, JsonSchema, Default)]
struct LogOutput {
    /// `ok`, or `refused` with a `code` and a `message`.
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// The lines, with host paths and run names hidden.
    text: String,
    /// Where the next piece begins: give it as `offset` to read on.
    next_offset: u64,
}

/// The answer to a start: the run id, or who holds the host, or the report that refused it. A
/// refusal whose report passed is a document job that had nothing to do.
fn start_output(outcome: StartOutcome) -> StartRunOutput {
    let mut out = StartRunOutput {
        status: String::new(),
        run_id: None,
        holder: None,
        retry_after_seconds: None,
        stale: false,
        report: None,
        message: None,
    };
    match outcome {
        StartOutcome::Started { run_id } => {
            out.status = "started".into();
            out.run_id = Some(run_id);
        }
        StartOutcome::Busy {
            holder,
            retry_after_seconds,
            stale,
        } => {
            out.status = "busy".into();
            out.holder = holder;
            out.retry_after_seconds = Some(retry_after_seconds);
            out.stale = stale;
        }
        StartOutcome::Refused(report) => {
            if report.exit_class == 0 {
                out.status = "nothing_to_run".into();
                out.message = Some(Text::english().render(&Line::NothingToRun));
            } else {
                out.status = "refused".into();
            }
            out.report = Some(*report);
        }
    }
    out
}

/// The ids of the jobs a queue report lists, in queue order.
fn job_ids(report: &Report) -> Vec<String> {
    match &report.repro {
        Some(Repro::Queue(data)) => data.jobs.iter().map(|job| job.id.clone()).collect(),
        _ => Vec::new(),
    }
}

/// The finding for a queued job whose runner could not be started: a start the system refused.
fn runner_failed() -> Diagnostic {
    let line = Line::QueueRefused {
        reason: "the job is queued, but the queue runner could not be started".into(),
    };
    Diagnostic::error("CFD-INFRA-007", "queue", "", Text::english().render(&line))
}

/// Make sure a runner is advancing the queue of the document `name`: start one, in a process of its
/// own that outlives this server, unless one is going. `Some` when it could not be started.
fn ensure_runner(s: &State, name: &str) -> Option<Diagnostic> {
    let doc = s.workspace.document_path(name)?;
    if QueueFile::new(&doc).drain_running() {
        return None;
    }
    let started =
        std::env::current_exe().and_then(|exe| spawn_detached(&exe, &s.settings.drain_args(&doc)));
    started.is_err().then(runner_failed)
}

/// Put `request` in the queue of the document `name` and make sure a runner is going when it added
/// a job. Returns the queue report and the ids of the jobs it added. A runner that could not be
/// started is a finding of class 4 on the report; the jobs stay queued.
fn add_to_queue(s: &State, name: &str, request: &AddRequest) -> (Report, Vec<String>) {
    let before = job_ids(&s.workspace.queue_list(name));
    let mut report = s.workspace.queue_add(name, request);
    let added: Vec<String> = job_ids(&report)
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect();
    if !added.is_empty()
        && let Some(found) = ensure_runner(s, name)
    {
        report.diagnostics.push(found);
        report.set_class(ExitClass::Infrastructure);
    }
    (report, added)
}

/// A solve of the study or of a variant, queued: the answer carries the job id of the first job
/// added, which `run_status` and `get_result` accept.
fn queue_start(
    s: &State,
    name: &str,
    scenario: Option<String>,
    remake_pruned: bool,
) -> StartRunOutput {
    let request = AddRequest {
        scenario,
        remake_pruned,
        ..AddRequest::default()
    };
    let (report, added) = add_to_queue(s, name, &request);
    let Some(run_id) = added.into_iter().next() else {
        let nothing = report.exit_class == 0;
        let mut out = start_output(StartOutcome::Refused(Box::new(report)));
        if nothing {
            out.message = Some(Text::english().render(&Line::QueueNothingToAdd));
        }
        return out;
    };
    let mut out = start_output(StartOutcome::Started { run_id });
    if report.exit_class != 0 {
        out.report = Some(report);
    }
    out
}

/// How a queue job ended, for `get_result`; a job that has not ended is still running.
fn job_result(doc: &Path, job: JobView) -> GetResultOutput {
    let ended = matches!(
        job.state.as_str(),
        "finished" | "stopped" | "failed" | "cancelled" | "interrupted"
    );
    if !ended {
        return GetResultOutput {
            status: "running".into(),
            report: None,
            job: None,
        };
    }
    let lines = QueueFile::new(doc)
        .live_dir(&job.id)
        .and_then(|live| read_report(&live))
        .map(|report| report.lines)
        .unwrap_or_default();
    GetResultOutput {
        status: "finished".into(),
        report: None,
        job: Some(JobResult {
            job: job.id,
            outcome: job.state,
            run: job.run,
            code: job.code,
            lines,
        }),
    }
}

/// The most lines one `run_log` call returns.
const MAX_LOG_LINES: u32 = 200;
/// The lines `run_log` returns when the caller does not say.
const DEFAULT_LOG_LINES: u32 = 40;

/// The word a plain enum is written as in JSON, such as `settled` or `not_converged`.
fn word<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A refusal with its stable code and the sentence from the text crate.
fn refusal(code: &str, line: &Line) -> (String, String) {
    (code.to_owned(), Text::english().render(line))
}

fn refused_name() -> (String, String) {
    let reason = "a document name is 1 to 40 characters: lower-case letters, digits and hyphens";
    refusal(
        "CFD-WS-001",
        &Line::QueueRefused {
            reason: reason.into(),
        },
    )
}

/// What a failed look at a job or a recorded run says. Nothing here quotes the operating system.
fn watch_refusal(error: WatchError) -> (String, String) {
    match error {
        WatchError::BadJob { job } | WatchError::UnknownJob { job } => {
            refusal("CFD-QUEUE-002", &Line::QueueUnknownJob { job })
        }
        WatchError::UnknownRun { run } => refusal("CFD-REPRO-004", &Line::UnknownRun { run }),
        WatchError::NoResult { run } => refusal("CFD-REPRO-005", &Line::RunHasNoResult { run }),
        WatchError::NoMirror { job } => {
            let line = Line::QueueNotAllowed {
                job,
                state: "not started".into(),
                action: "watched".into(),
            };
            refusal("CFD-QUEUE-003", &line)
        }
        WatchError::Queue { .. } | WatchError::Io { .. } => {
            let reason = "the live view could not be read".into();
            refusal("CFD-QUEUE-004", &Line::QueueDamaged { reason })
        }
    }
}

fn monitors_refused((code, message): (String, String)) -> MonitorsOutput {
    MonitorsOutput {
        status: "refused".into(),
        code: Some(code),
        message: Some(message),
        ..MonitorsOutput::default()
    }
}

fn rows_out(rows: &[RowView]) -> Vec<MonitorRow> {
    rows.iter()
        .map(|row| MonitorRow {
            name: row.name.clone(),
            label: row.label.clone(),
            last: row.last,
            tail_mean: row.mean,
            spread: row.spread,
            state: word(&row.state),
        })
        .collect()
}

fn balance_out(balance: &BalanceView) -> BalanceOut {
    BalanceOut {
        state: word(&balance.status),
        imbalance: balance.imbalance,
    }
}

/// The table of a job or of a recorded run, read from the copies on this machine.
fn monitors_output(s: &State, a: &MonitorsArgs) -> MonitorsOutput {
    let Some(doc) = s.workspace.document_path(&a.name) else {
        return monitors_refused(refused_name());
    };
    if let Some(run) = &a.run {
        if a.job.is_some() {
            let reason = "ask for a job or for a run, not for both at once".into();
            return monitors_refused(refusal("CFD-QUEUE-001", &Line::QueueRefused { reason }));
        }
        return match watch::monitors_of_run(&doc, run) {
            Ok(view) => MonitorsOutput {
                status: "ok".into(),
                run: Some(view.run),
                iterations: Some(view.iterations),
                last_time: view.last_time,
                ended: Some(view.ended),
                rows: rows_out(&view.rows),
                mass_balance: Some(balance_out(&view.mass)),
                energy_balance: Some(balance_out(&view.energy)),
                verdict: Some(VerdictOut {
                    state: word(&view.verdict.state),
                    reasons: view.verdict.reasons.clone(),
                }),
                recorded_verdict: view.recorded.as_ref().map(|v| VerdictOut {
                    state: word(&v.state),
                    reasons: v.reasons.clone(),
                }),
                ..MonitorsOutput::default()
            },
            Err(error) => monitors_refused(watch_refusal(error)),
        };
    }
    let job = a.job.clone().or_else(|| {
        let report = s.workspace.queue_list(&a.name);
        match report.repro {
            Some(Repro::Queue(data)) => data
                .active
                .clone()
                .or_else(|| data.jobs.last().map(|job| job.id.clone())),
            _ => None,
        }
    });
    let Some(job) = job else {
        let reason = "the queue holds no job to show".into();
        return monitors_refused(refusal("CFD-QUEUE-002", &Line::QueueRefused { reason }));
    };
    match watch::monitors(&doc, &job) {
        Ok(view) => MonitorsOutput {
            status: "ok".into(),
            job: Some(view.job),
            state: Some(word(&view.state)),
            iterations: Some(view.iterations),
            last_time: view.last_time,
            ended: Some(view.ended),
            rows: rows_out(&view.rows),
            mass_balance: Some(balance_out(&view.mass)),
            energy_balance: Some(balance_out(&view.energy)),
            verdict: Some(VerdictOut {
                state: word(&view.verdict.state),
                reasons: view.verdict.reasons.clone(),
            }),
            ..MonitorsOutput::default()
        },
        Err(error) => monitors_refused(watch_refusal(error)),
    }
}

/// A piece of the solver log of a job, from the copy on this machine.
fn log_output(s: &State, a: &LogArgs) -> LogOutput {
    let refused = |(code, message): (String, String)| LogOutput {
        status: "refused".into(),
        code: Some(code),
        message: Some(message),
        ..LogOutput::default()
    };
    let lines = a.lines.unwrap_or(DEFAULT_LOG_LINES);
    if !(1..=MAX_LOG_LINES).contains(&lines) {
        let reason = format!("lines is 1 to {MAX_LOG_LINES}");
        return refused(refusal("CFD-QUEUE-001", &Line::QueueRefused { reason }));
    }
    let Some(doc) = s.workspace.document_path(&a.name) else {
        return refused(refused_name());
    };
    let wanted = usize::try_from(lines).unwrap_or(usize::MAX);
    match watch::log(&doc, &a.job, a.offset.unwrap_or(0), wanted) {
        Ok(view) => LogOutput {
            status: "ok".into(),
            text: view.text,
            next_offset: view.next_offset,
            ..LogOutput::default()
        },
        Err(error) => refused(watch_refusal(error)),
    }
}

/// Run a blocking operation off the protocol loop. A panic becomes a protocol error with a fixed
/// sentence.
async fn blocking<T>(f: impl FnOnce() -> T + Send + 'static) -> Result<Json<T>, ErrorData>
where
    T: Serialize + JsonSchema + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map(Json)
        .map_err(|_| ErrorData::internal_error("The operation stopped unexpectedly.", None))
}

#[derive(Clone)]
struct CfdServer {
    state: Arc<State>,
    tool_router: ToolRouter<Self>,
}

#[tool_router(router = tool_router)]
impl CfdServer {
    fn new(state: Arc<State>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "create_study",
        description = "Store a study under a name if it passes every check. Returns the findings; nothing is stored when there is an error."
    )]
    async fn create_study(
        &self,
        Parameters(a): Parameters<CreateStudyArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.create_study(&a.name, &a.study)).await
    }

    #[tool(
        name = "validate_study",
        description = "Check a stored study and report the findings. Writes nothing."
    )]
    async fn validate_study(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.validate_study(&a.name)).await
    }

    #[tool(
        name = "compile",
        description = "Compile a stored study into a solver case held by the server, for the default backend or the one named. Returns the findings and the files written. A setting the backend does not offer is a finding and nothing is written."
    )]
    async fn compile(
        &self,
        Parameters(a): Parameters<CompileArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.compile_on(&a.name, a.backend.as_deref())).await
    }

    #[tool(
        name = "export",
        description = "Export a stored study as a runnable case with an Allrun script and a README, held by the server."
    )]
    async fn export(&self, Parameters(a): Parameters<NameArgs>) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.export(&a.name)).await
    }

    #[tool(
        name = "save_document",
        description = "Save a stored study as a document held by the server. Returns the findings."
    )]
    async fn save_document(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.save_document(&a.name)).await
    }

    #[tool(
        name = "open_document",
        description = "Open a stored document and report its study hash, any migration note and any missing results or links. Writes nothing."
    )]
    async fn open_document(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.open_document(&a.name)).await
    }

    #[tool(
        name = "pack_document",
        description = "Pack a stored document and its stored results into one file held by the server."
    )]
    async fn pack_document(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.pack_document(&a.name)).await
    }

    #[tool(
        name = "unpack_document",
        description = "Unpack a pack that the operator placed in the server's pack folder into a document, checking every stored result. Never overwrites."
    )]
    async fn unpack_document(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.unpack_document(&a.name)).await
    }

    #[tool(
        name = "start_run",
        description = "Start a run of a stored study on the compute host and return at once with a run id; poll run_status and get_result. If the host is busy the answer is busy with a retry time; starts are never queued."
    )]
    async fn start_run(
        &self,
        Parameters(a): Parameters<StartRunArgs>,
    ) -> Result<Json<StartRunOutput>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            let outcome = s.workspace.start_run_on(
                &s.registry,
                &s.settings,
                &a.name,
                a.mode.into(),
                a.from_export,
                a.backend,
            );
            start_output(outcome)
        })
        .await
    }

    #[tool(
        name = "start_document_job",
        description = "Start a job on a stored document and return at once: solve it (or one of its variants), rerun one of its recorded results, or update its mesh or its result. A solve is put in the document's queue and answered with a job id such as q-001; a background runner on this machine advances the queue and goes on when this server stops, so leaving a solve does not stop it. Watch it with run_status, run_monitors and run_log, and read how it ended with get_result. A rerun or an update runs at once and stops with this server; if the host is busy the answer is busy with a retry time. When nothing is out of date the answer is nothing_to_run and nothing starts."
    )]
    async fn start_document_job(
        &self,
        Parameters(a): Parameters<StartDocumentJobArgs>,
    ) -> Result<Json<StartRunOutput>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            let kind = match a.kind() {
                Ok(kind) => kind,
                Err(report) => return start_output(StartOutcome::Refused(report)),
            };
            match kind {
                DocJobKind::Solve if s.settings.queued() => queue_start(&s, &a.name, None, false),
                DocJobKind::SolveScenario {
                    scenario,
                    remake_pruned,
                } if s.settings.queued() => queue_start(&s, &a.name, scenario, remake_pruned),
                kind => start_output(s.workspace.start_document_job(
                    &s.registry,
                    &s.settings,
                    &a.name,
                    kind,
                )),
            }
        })
        .await
    }

    #[tool(
        name = "run_status",
        description = "Say whether a run is still going. Runs started by this server are known, and so are the queue jobs of the documents of its workspace (give the document's name with a job id such as q-001)."
    )]
    async fn run_status(
        &self,
        Parameters(a): Parameters<RunIdArgs>,
    ) -> Result<Json<RunStatusOutput>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            if let Some((_, job)) = s.workspace.find_job(a.name.as_deref(), &a.run_id) {
                return RunStatusOutput {
                    status: job.state,
                    mode: Some(job.kind),
                    elapsed_seconds: None,
                    exit_class: None,
                };
            }
            let (status, mode, elapsed, class) =
                match s.workspace.run_status(&s.registry, &a.run_id) {
                    Status::NotFound => ("not_found", None, None, None),
                    Status::Running {
                        mode,
                        elapsed_seconds,
                    } => ("running", Some(mode), Some(elapsed_seconds), None),
                    Status::Finished {
                        mode,
                        elapsed_seconds,
                        exit_class,
                    } => (
                        "finished",
                        Some(mode),
                        Some(elapsed_seconds),
                        Some(exit_class),
                    ),
                };
            RunStatusOutput {
                status: status.into(),
                mode: mode.map(|m| m.operation().to_owned()),
                elapsed_seconds: elapsed,
                exit_class: class,
            }
        })
        .await
    }

    #[tool(
        name = "get_result",
        description = "Return the report of a finished run. Judge it by evidence.verdict: a smoke pass shows only that the case loads and iterates. For a queue job id such as q-001 (with the document's name) it says how the job ended and names the run it recorded."
    )]
    async fn get_result(
        &self,
        Parameters(a): Parameters<RunIdArgs>,
    ) -> Result<Json<GetResultOutput>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            if let Some((doc, job)) = s.workspace.find_job(a.name.as_deref(), &a.run_id) {
                return job_result(&doc, job);
            }
            let (status, report) = match s.workspace.run_result(&s.registry, &a.run_id) {
                ResultOutcome::NotFound => ("not_found", None),
                ResultOutcome::Running => ("running", None),
                ResultOutcome::Finished(report) => ("finished", Some(*report)),
            };
            GetResultOutput {
                status: status.into(),
                report,
                job: None,
            }
        })
        .await
    }

    #[tool(
        name = "document_status",
        description = "Say whether a stored document, or one of its variants, is up to date, and for each stage why not, in plain words. Writes nothing and starts nothing."
    )]
    async fn document_status(
        &self,
        Parameters(a): Parameters<StatusArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.scenario_status(&a.name, a.scenario.as_deref())).await
    }

    #[tool(
        name = "scenario_list",
        description = "List the variants of a stored document, the study itself (`base`) first, with each one's kind, its changes counted and its keys. Writes nothing and starts nothing."
    )]
    async fn scenario_list(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.scenario_list(&a.name)).await
    }

    #[tool(
        name = "scenario_show",
        description = "Show one variant of a stored document in full: the changes it stores, the raw entries of its study and the findings of its study. Writes nothing and starts nothing."
    )]
    async fn scenario_show(
        &self,
        Parameters(a): Parameters<ScenarioArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.scenario_show(&a.name, &a.scenario)).await
    }

    #[tool(
        name = "scenario_diff",
        description = "Say how one variant of a stored document differs from another, in the words a status uses. Either may be `base`. Writes nothing and starts nothing."
    )]
    async fn scenario_diff(
        &self,
        Parameters(a): Parameters<DiffArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.scenario_diff(&a.name, &a.a, &a.b)).await
    }

    #[tool(
        name = "scenario_set",
        description = "Create a variant of a stored document, or change the one that exists, with a list of changes to settings. Changes only the document named and starts nothing. Returns the variant, or the reason a change does not apply."
    )]
    async fn scenario_set(
        &self,
        Parameters(a): Parameters<ScenarioSetArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            s.workspace.scenario_set(
                &a.name,
                &a.scenario,
                a.parent.as_deref(),
                &a.overrides,
                a.title.as_deref(),
            )
        })
        .await
    }

    #[tool(
        name = "scenario_label",
        description = "Set or clear the title, the pin and the notes of a variant of a stored document, or of the study itself (`base`). Changes only the document named and starts nothing."
    )]
    async fn scenario_label(
        &self,
        Parameters(a): Parameters<ScenarioLabelArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            let update = LabelUpdate {
                name: a.title,
                pinned: a.pinned,
                notes: a.notes,
            };
            s.workspace.scenario_label(&a.name, &a.scenario, &update)
        })
        .await
    }

    #[tool(
        name = "sweep_plan",
        description = "Expand a sweep into its points: each point with its label and the settings it changes. Stores nothing and starts nothing."
    )]
    async fn sweep_plan(
        &self,
        Parameters(a): Parameters<SweepArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.sweep_plan(&a.name, &a.sweep)).await
    }

    #[tool(
        name = "sweep_create",
        description = "Store a sweep and one variant for each of its points in the named document. Changes only the document named and starts nothing."
    )]
    async fn sweep_create(
        &self,
        Parameters(a): Parameters<SweepArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.sweep_create(&a.name, &a.sweep)).await
    }

    #[tool(
        name = "sweep_preflight",
        description = "Check every point of a stored sweep without solving anything: its changes apply, its study passes the checks and its case is built and scanned. A failing point is exit class 1 with its reason. Writes nothing and starts nothing."
    )]
    async fn sweep_preflight(
        &self,
        Parameters(a): Parameters<SweepOfArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.sweep_preflight(&a.name, &a.sweep)).await
    }

    #[tool(
        name = "benchmark_list",
        description = "List the benchmark templates a study can be made from, each with its reference, its defaults and the quantities a run of it is compared on. Writes nothing and starts nothing."
    )]
    async fn benchmark_list(
        &self,
        Parameters(_): Parameters<NoArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.benchmark_list()).await
    }

    #[tool(
        name = "benchmark_create",
        description = "Make the study of a benchmark template with a mesh preset and physics options and return it as text; the study is checked first. Stores nothing, writes nothing and starts nothing: save it with create_study."
    )]
    async fn benchmark_create(
        &self,
        Parameters(a): Parameters<BenchmarkCreateArgs>,
    ) -> Result<Json<BenchmarkStudy>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            let options = BenchmarkOptions {
                mesh: a.mesh,
                turbulence: a.turbulence,
                time: a.time,
                delta_t: a.delta_t,
                init: a.init,
                rayleigh: a.rayleigh,
                reynolds: a.reynolds,
            };
            let (report, study) = s.workspace.benchmark_create(&a.benchmark, &options);
            BenchmarkStudy { study, report }
        })
        .await
    }

    #[tool(
        name = "benchmark_check",
        description = "Check the stored result of a recorded run of a stored document against the published reference of its benchmark, metric by metric with the tolerance used. A metric outside its tolerance is exit class 1. The gate also says whether the run had converged (gate.convergence.converged and its reason; unknown when the run recorded no assessment): a flag beside the verdict, which it never changes. Writes nothing and starts nothing."
    )]
    async fn benchmark_check(
        &self,
        Parameters(a): Parameters<BenchmarkCheckArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            s.workspace
                .benchmark_check(&a.name, a.run.as_deref(), a.benchmark.as_deref())
        })
        .await
    }

    #[tool(
        name = "gci_create",
        description = "Store a mesh independence sweep in the named document: one variant for each of three to five mesh levels. Changes only the document named and starts nothing; solve the points with queue_add."
    )]
    async fn gci_create(
        &self,
        Parameters(a): Parameters<GciCreateArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            s.workspace.gci_create(
                &a.name,
                &a.sweep,
                a.scenario.as_deref(),
                a.levels.as_deref(),
            )
        })
        .await
    }

    #[tool(
        name = "gci_report",
        description = "Report the observed order of convergence and the grid convergence index of a key number over the points of a stored mesh independence sweep, from the results already recorded. Reads recorded key numbers, writes nothing and starts nothing."
    )]
    async fn gci_report(
        &self,
        Parameters(a): Parameters<GciReportArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.gci_report(&a.name, &a.sweep, &a.quantity)).await
    }

    #[tool(
        name = "backend_list",
        description = "List the solver backends: id, family, admission state, image and whether its results are a preview, with the candidate and image notices that apply. Choose one by id for compile, start_run and queue_add. Writes nothing and starts nothing."
    )]
    async fn backend_list(
        &self,
        Parameters(_): Parameters<NoArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        blocking(cfdcraft_ops::backend_list).await
    }

    #[tool(
        name = "import_case",
        description = "Import an existing OpenFOAM case folder of the workspace as a read-only copy with an inventory of what its dictionaries say and the dialect it was written for. A case that holds a construct that could run code is refused whole, with one finding for each entry, and nothing is written. The source is only read."
    )]
    async fn import_case(
        &self,
        Parameters(a): Parameters<ImportCaseArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.import_case(&a.source, &a.name)).await
    }

    #[tool(
        name = "parity_table",
        description = "Show the parity table of a benchmark over the runs a stored document has recorded: one column for each backend that has a recorded run, against the default backend, with the gate, convergence and preview label of each column. A column that failed or did not converge is shown and marked, never hidden. Writes nothing and starts nothing."
    )]
    async fn parity_table(
        &self,
        Parameters(a): Parameters<ParityTableArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.parity_table(&a.name, &a.benchmark)).await
    }

    #[tool(
        name = "disk_use",
        description = "Say how much stored result each variant of a stored document holds, in bytes: its mesh, its result, what it shares with another variant and what was pruned. Writes nothing and starts nothing."
    )]
    async fn disk_use(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.disk_use(&a.name)).await
    }

    #[tool(
        name = "prune",
        description = "Show which stored results of a document can be removed, and how many bytes that frees. By default this only shows the plan and removes nothing. With apply true it removes the stored results that no pinned variant needs, in the named variants or in all, and it cannot be undone. Every run record and its key numbers are kept."
    )]
    async fn prune(&self, Parameters(a): Parameters<PruneArgs>) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.prune(&a.name, a.scenarios.as_deref(), a.apply)).await
    }

    #[tool(
        name = "list_runs",
        description = "List the runs a stored document records, oldest first, with what each recorded and its checks. Writes nothing and starts nothing."
    )]
    async fn list_runs(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.list_runs(&a.name)).await
    }

    #[tool(
        name = "get_run_result",
        description = "Return the check verdicts and key numbers of a recorded run of a stored document: the run named, or the newest that recorded a result. Writes nothing and starts nothing."
    )]
    async fn get_run_result(
        &self,
        Parameters(a): Parameters<RunOfArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.document_result(&a.name, a.run.as_deref())).await
    }

    #[tool(
        name = "get_provenance",
        description = "Return what a recorded run of a stored document was made from: the run named, or the newest. Writes nothing and starts nothing."
    )]
    async fn get_provenance(
        &self,
        Parameters(a): Parameters<RunOfArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.document_provenance(&a.name, a.run.as_deref())).await
    }

    #[tool(
        name = "compare_runs",
        description = "Compare the key numbers of two recorded runs of a stored document under a tolerance. The verdict is reproduced, within_tolerance or differs; a difference is exit class 1. Writes nothing and starts nothing."
    )]
    async fn compare_runs(
        &self,
        Parameters(a): Parameters<CompareArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            s.workspace
                .compare_runs(&a.name, &a.a, &a.b, a.abs_tolerance, a.rel_tolerance)
        })
        .await
    }

    #[tool(
        name = "plan_update",
        description = "Show what an update of stored documents would do: the items that are out of date and the order they would be made in, refusing what an update would refuse. Writes nothing and starts nothing."
    )]
    async fn plan_update(
        &self,
        Parameters(a): Parameters<PlanUpdateArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            s.workspace
                .plan_update(&a.names, a.items.as_deref(), a.order.as_deref())
        })
        .await
    }

    #[tool(
        name = "queue_list",
        description = "List the jobs in the queue of a stored document, in the order they start, with each one's state, variant, kind, recorded run, code and progress line. Reads the document's queue only: it touches no compute host and starts nothing."
    )]
    async fn queue_list(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.queue_list(&a.name)).await
    }

    #[tool(
        name = "queue_add",
        description = "Put more jobs in the queue of a stored document: the study itself (no other input), one variant, every point of a stored sweep, or the continuation of a stopped run. Give at most one of scenario, sweep and run; backend names the backend the jobs run on. A background runner on this machine then advances the queue, one job at a time, and goes on when this server stops. Returns the queue. A request with nothing out of date adds nothing."
    )]
    async fn queue_add(
        &self,
        Parameters(a): Parameters<QueueAddArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            let request = AddRequest {
                scenario: a.scenario,
                sweep: a.sweep,
                continue_run: a.run,
                remake_pruned: a.remake_pruned,
                backend: a.backend,
            };
            add_to_queue(&s, &a.name, &request).0
        })
        .await
    }

    #[tool(
        name = "queue_reorder",
        description = "Move a waiting job to a place among the waiting jobs, counted from 1. Only a waiting job can be moved. Returns the queue. Touches no compute host."
    )]
    async fn queue_reorder(
        &self,
        Parameters(a): Parameters<QueueReorderArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            let to = usize::try_from(a.position.saturating_sub(1)).unwrap_or(usize::MAX);
            s.workspace.queue_reorder(&a.name, &a.job, to)
        })
        .await
    }

    #[tool(
        name = "queue_cancel",
        description = "Cancel a waiting job at once, or ask a running job to write its latest results and stop; it then ends as stopped and can be continued. A job that has ended cannot be cancelled. Returns the queue. The stop is carried out by the background runner; this call touches no compute host."
    )]
    async fn queue_cancel(
        &self,
        Parameters(a): Parameters<JobArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.queue_cancel(&a.name, &a.job)).await
    }

    #[tool(
        name = "run_stop",
        description = "Ask a running job to write its latest results and stop. It then ends as stopped, is recorded as a stopped run without a result, and can be continued with queue_add and its run. Only a running job can be asked. The stop is carried out by the background runner; this call touches no compute host."
    )]
    async fn run_stop(
        &self,
        Parameters(a): Parameters<JobArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.queue_stop(&a.name, &a.job)).await
    }

    #[tool(
        name = "run_monitors",
        description = "Show the monitored quantities of a job as the files copied from the compute host stand now: for each its last value, tail mean, spread and whether it is settled or drifting, then the mass and energy balances and the convergence verdict. Give a job (default: the active job, else the newest) or a recorded run of the document, whose table is made again from its stored result. Reads copies already on this machine: it touches no compute host. A verdict that is not converged is a reading, not a failure."
    )]
    async fn run_monitors(
        &self,
        Parameters(a): Parameters<MonitorsArgs>,
    ) -> Result<Json<MonitorsOutput>, ErrorData> {
        let s = self.state.clone();
        blocking(move || monitors_output(&s, &a)).await
    }

    #[tool(
        name = "run_log",
        description = "Return a piece of the solver log of a job from the copy on this machine: at most 200 lines from a byte offset, with host paths and run names hidden, and the offset the next piece begins at. Call it again with next_offset to read on. It touches no compute host."
    )]
    async fn run_log(
        &self,
        Parameters(a): Parameters<LogArgs>,
    ) -> Result<Json<LogOutput>, ErrorData> {
        let s = self.state.clone();
        blocking(move || log_output(&s, &a)).await
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "cfdcraft",
    instructions = "Create, check, export and run CFD studies by name, and save, open, pack and unpack them as documents by name. Start a job on a document (solve it, rerun a recorded result, update its mesh or its result). A solve is queued and answered with a job id that goes on when the server stops; a rerun or an update runs at once, one at a time. Poll a job with run_status, watch it with run_monitors and run_log, reorder or cancel the queue, ask a running job to stop with run_stop, and read how it ended with get_result. Read whether a document is up to date and why not, its runs, the checks and key numbers of a run, its provenance, the comparison of two runs and what an update would do. Document variants (scenarios) are listed, shown, diffed, created, changed, titled, pinned and annotated by name, and a variant is solved and its status read like the study. Sweeps are planned, stored and checked before anything is solved, benchmark studies are listed and made as text, a recorded run is checked against the published reference of its benchmark, a mesh independence sweep is stored and its grid convergence index read from recorded results, disk use is read per variant, and stored results are pruned only when a prune is asked to apply its plan. The solver backends are listed and may be named for a compile, a run or queued jobs, an OpenFOAM case folder of the workspace is imported as a read-only copy, and the parity table of a benchmark is read from the recorded runs of a document. Judge a run by evidence.verdict in its report."
)]
impl ServerHandler for CfdServer {}

/// Resolves when the process is asked to stop. The handlers are installed before serving starts.
#[cfg(unix)]
fn stop_signal() -> impl std::future::Future<Output = ()> {
    use tokio::signal::unix::{SignalKind, signal};
    let term = signal(SignalKind::terminate());
    let int = signal(SignalKind::interrupt());
    async move {
        match (term, int) {
            (Ok(mut t), Ok(mut i)) => {
                tokio::select! { _ = t.recv() => {}, _ = i.recv() => {} }
            }
            _ => std::future::pending().await,
        }
    }
}

#[cfg(not(unix))]
async fn stop_signal() {
    if tokio::signal::ctrl_c().await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Serve the operations on standard input and output until the client closes the pipe or the
/// process is asked to stop. Then the server's own job, if any, is cancelled through the
/// executor's stop-and-remove path, and the lock is released only when removal is confirmed.
/// Returns the exit status: 0, or `INPUT_FAILURE_EXIT` after one line on standard error when
/// standard input failed to read rather than ending.
pub fn serve_stdio_blocking(config: McpConfig) -> Result<i32, String> {
    let state = Arc::new(State::open(&config)?);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("the server cannot start: {e}"))?;
    let failed = Arc::new(OnceLock::new());
    let input = WatchedInput {
        inner: tokio::io::stdin(),
        failed: failed.clone(),
    };
    let served = rt.block_on(async {
        let stop = stop_signal();
        let service = CfdServer::new(state.clone())
            .serve((input, tokio::io::stdout()))
            .await
            .map_err(|e| format!("the protocol handshake failed: {e}"))?;
        let token = service.cancellation_token();
        let mut waiting = Box::pin(service.waiting());
        tokio::select! {
            r = &mut waiting => r.map(|_| ()).map_err(|e| format!("the server stopped: {e}")),
            () = stop => {
                token.cancel();
                let _ = waiting.await;
                Ok(())
            }
        }
    });
    let confirmed = state.registry.shutdown(SHUTDOWN_WAIT);
    // A blocked read of standard input must not hold the process open.
    rt.shutdown_background();
    served?;
    if let Some(kind) = failed.get() {
        eprintln!("error: the server stopped because standard input could not be read ({kind})");
        return if confirmed {
            Ok(INPUT_FAILURE_EXIT)
        } else {
            Err("a running job did not confirm its cleanup in time".into())
        };
    }
    if confirmed {
        Ok(0)
    } else {
        Err("a running job did not confirm its cleanup in time".into())
    }
}
