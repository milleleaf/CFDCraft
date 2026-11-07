//! An MCP server over stdio that offers eleven typed operations and nothing else: create and
//! validate a study, compile and export it, save, open, pack and unpack a document, and start,
//! poll and collect a run. There is no shell
//! and no free-form dictionary access. Every argument is a name, a mode, a flag or a study;
//! none is a path, a host, a key or a command. Those are settings of the server itself, given
//! once on the command line.
//!
//! Standard output carries protocol messages only; anything else goes to standard error.

use cfdcraft_ops::{
    HolderInfo, Report, ResultOutcome, RunMode, RunRegistry, ServerSettings, StartOutcome, Status,
    Workspace,
};
use rmcp::{
    ErrorData, Json, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::PathBuf,
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
        )?;
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
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RunIdArgs {
    /// The run id that `start_run` returned.
    run_id: String,
}

// ---- tool results ----

#[derive(Serialize, JsonSchema)]
struct StartRunOutput {
    /// `started`, `busy` or `refused`. A start never waits for the job and never queues.
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
    /// When refused: the reasons, in the same structure every operation returns.
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Report>,
}

#[derive(Serialize, JsonSchema)]
struct RunStatusOutput {
    /// `running`, `finished` or `not_found`.
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
    /// `running`, `finished` or `not_found`.
    status: String,
    /// Once finished: the full report.
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<Report>,
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
        description = "Compile a stored study into a solver case held by the server. Returns the findings and the files written."
    )]
    async fn compile(
        &self,
        Parameters(a): Parameters<NameArgs>,
    ) -> Result<Json<Report>, ErrorData> {
        let s = self.state.clone();
        blocking(move || s.workspace.compile(&a.name)).await
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
            let outcome = s.workspace.start_run(
                &s.registry,
                &s.settings,
                &a.name,
                a.mode.into(),
                a.from_export,
            );
            let mut out = StartRunOutput {
                status: String::new(),
                run_id: None,
                holder: None,
                retry_after_seconds: None,
                stale: false,
                report: None,
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
                    out.status = "refused".into();
                    out.report = Some(*report);
                }
            }
            out
        })
        .await
    }

    #[tool(
        name = "run_status",
        description = "Say whether a run is still going. Only runs started by this server are known."
    )]
    async fn run_status(
        &self,
        Parameters(a): Parameters<RunIdArgs>,
    ) -> Result<Json<RunStatusOutput>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
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
        description = "Return the report of a finished run. Judge it by evidence.verdict: a smoke pass shows only that the case loads and iterates."
    )]
    async fn get_result(
        &self,
        Parameters(a): Parameters<RunIdArgs>,
    ) -> Result<Json<GetResultOutput>, ErrorData> {
        let s = self.state.clone();
        blocking(move || {
            let (status, report) = match s.workspace.run_result(&s.registry, &a.run_id) {
                ResultOutcome::NotFound => ("not_found", None),
                ResultOutcome::Running => ("running", None),
                ResultOutcome::Finished(report) => ("finished", Some(*report)),
            };
            GetResultOutput {
                status: status.into(),
                report,
            }
        })
        .await
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "cfdcraft",
    instructions = "Create, check, export and run CFD studies by name, and save, open, pack and unpack them as documents by name. Judge a run by evidence.verdict in its report."
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

/// Serve the eleven operations on standard input and output until the client closes the pipe or the
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
