use crate::{
    docker::{RunSpec, docker_cmd, run_args},
    hosts::{HostProfile, PreflightError, preflight},
    lock::{self, Guard, Holder},
    tunnel::{SOCKET_PATH_LIMIT, Tunnel, ssh_command},
};
use std::{
    fs::File,
    hash::{DefaultHasher, Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub enum RunError {
    Io(std::io::Error),
    Preflight(PreflightError),
    TimedOut,
    Setup(String),
    /// The compute host is reachable but another job holds the lock.
    Busy {
        holder: Option<Holder>,
        stale: bool,
    },
    /// ssh itself failed (connection, login): not the same as a busy host.
    Ssh {
        detail: String,
    },
    /// The container service on the host failed a command.
    Docker {
        detail: String,
    },
    /// The caller asked the job to stop; the owned container is gone.
    Cancelled,
    SocketPathTooLong,
}

impl From<std::io::Error> for RunError {
    fn from(e: std::io::Error) -> Self {
        RunError::Io(e)
    }
}

impl From<PreflightError> for RunError {
    fn from(e: PreflightError) -> Self {
        RunError::Preflight(e)
    }
}

fn busy_text(holder: &Option<Holder>, stale: bool) -> String {
    match holder {
        Some(h) => format!(
            "another job holds the lock ({}, {} s left of its cap{})",
            h.container,
            h.remaining_seconds(lock::now_epoch().unwrap_or(0)),
            if stale { ", looks stale" } else { "" }
        ),
        None => "another job holds the lock".into(),
    }
}

impl RunError {
    /// True when the right answer is "try again later": the lock is held, a
    /// CFDCraft container is already running, or the host load is over the line.
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            RunError::Busy { .. }
                | RunError::Preflight(
                    PreflightError::ExistingCfdcraftContainers { .. } | PreflightError::Busy { .. }
                )
        )
    }

    /// A stable code for the failure class, safe to show anywhere.
    pub fn infra_code(&self) -> &'static str {
        match self {
            RunError::Busy { .. } => "CFD-INFRA-001",
            RunError::Preflight(
                PreflightError::ExistingCfdcraftContainers { .. } | PreflightError::Busy { .. },
            ) => "CFD-INFRA-001",
            RunError::Ssh { .. } => "CFD-INFRA-002",
            RunError::Docker { .. } => "CFD-INFRA-003",
            RunError::TimedOut => "CFD-INFRA-004",
            RunError::Cancelled => "CFD-INFRA-006",
            RunError::Io(_)
            | RunError::Preflight(_)
            | RunError::Setup(_)
            | RunError::SocketPathTooLong => "CFD-INFRA-005",
        }
    }

    /// One fixed sentence per failure class. Nothing from the environment (key
    /// path, host, remote directories) can appear in it, so it may go into a
    /// report, a log line or a tool error.
    pub fn public_message(&self) -> String {
        match self {
            RunError::SocketPathTooLong => {
                format!(
                    "the socket directory path is too long (limit {SOCKET_PATH_LIMIT} characters)"
                )
            }
            other => match other.infra_code() {
                "CFD-INFRA-001" => "the compute host is busy; try again later",
                "CFD-INFRA-002" => "the compute host could not be reached over ssh",
                "CFD-INFRA-003" => "the container service on the compute host failed",
                "CFD-INFRA-004" => "the run exceeded its wall-clock cap",
                "CFD-INFRA-006" => "the run was cancelled",
                _ => "the run could not be set up",
            }
            .into(),
        }
    }

    /// The full text, for stderr only: it can name the key path, the host and
    /// remote directories.
    pub fn detail(&self) -> String {
        match self {
            RunError::Io(e) => format!("I/O: {e}"),
            RunError::Preflight(e) => format!("preflight: {e}"),
            RunError::TimedOut => "wall-clock cap exceeded".into(),
            RunError::Setup(s) => s.clone(),
            RunError::Busy { holder, stale } => busy_text(holder, *stale),
            RunError::Ssh { detail } => format!("ssh failed: {detail}"),
            RunError::Docker { detail } => format!("docker failed: {detail}"),
            RunError::Cancelled => "run cancelled".into(),
            RunError::SocketPathTooLong => self.public_message(),
        }
    }
}

/// `Display` is the public message, so `error: {e}`, a JSON report and a tool
/// error cannot leak the environment. Print `detail()` separately to stderr.
impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.public_message())
    }
}

impl std::error::Error for RunError {}

/// Which program a command talks to, so a failure maps to the right error.
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Ssh,
    Docker,
    Local,
}

impl Stage {
    fn failure(self, status: ExitStatus, detail: String) -> RunError {
        match self {
            // 255 is ssh's own failure; any other status came from the remote command.
            Stage::Ssh if status.code() == Some(lock::SSH_FAILED_EXIT) => RunError::Ssh { detail },
            Stage::Docker => RunError::Docker { detail },
            _ => RunError::Setup(detail),
        }
    }
}

/// Where a job puts its tunnel socket and how a caller stops it.
#[derive(Clone)]
pub struct RunConfig {
    pub socket_dir: PathBuf,
    pub cancel: Arc<AtomicBool>,
}

impl RunConfig {
    /// The caller supplies a short directory of its own.
    pub fn new(socket_dir: impl Into<PathBuf>) -> Self {
        Self {
            socket_dir: socket_dir.into(),
            cancel: Arc::default(),
        }
    }

    /// `target/cfdcraft-sockets` under the current directory, read once here.
    pub fn from_cwd() -> std::io::Result<Self> {
        Ok(Self::new(
            std::env::current_dir()?.join("target/cfdcraft-sockets"),
        ))
    }

    /// Set the returned flag from any thread to stop the job.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }
}

#[derive(Debug)]
pub struct RunOutcome {
    pub results_dir: PathBuf,
    pub remote_dir: String,
    pub container_name: String,
    pub exit_code: Option<i32>,
    pub preflight_uptime: String,
    pub duration_seconds: f64,
}

pub fn wait_bounded(child: &mut Child, limit: Duration) -> Result<ExitStatus, RunError> {
    wait_cancellable(child, limit, &AtomicBool::new(false))
}

/// Like `wait_bounded`, and also stops waiting when `cancel` is set: the child
/// is killed and `Cancelled` returned.
pub fn wait_cancellable(
    child: &mut Child,
    limit: Duration,
    cancel: &AtomicBool,
) -> Result<ExitStatus, RunError> {
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait()? {
            return Ok(s);
        }
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::Cancelled);
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn checked(c: &mut Command, stage: Stage) -> Result<(), RunError> {
    let mut child = c.spawn()?;
    let s = wait_bounded(&mut child, Duration::from_secs(60))?;
    if !s.success() {
        return Err(stage.failure(s, format!("command failed: {s}")));
    }
    Ok(())
}
fn text(c: &mut Command, stage: Stage) -> Result<String, RunError> {
    text_bounded(c, Duration::from_secs(30), stage)
}
fn text_bounded(c: &mut Command, limit: Duration, stage: Stage) -> Result<String, RunError> {
    let (status, out, err) = output_bounded(c, limit)?;
    if !status.success() {
        return Err(stage.failure(status, String::from_utf8_lossy(&err).into_owned()));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}
/// Run a command with piped output under a wall-clock limit; the caller decides
/// what the exit status means.
pub(crate) fn output_bounded(
    c: &mut Command,
    limit: Duration,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>), RunError> {
    let mut child = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let mut out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    let stdout = std::thread::spawn(move || {
        let mut b = Vec::new();
        out.read_to_end(&mut b).map(|_| b)
    });
    let stderr = std::thread::spawn(move || {
        let mut b = Vec::new();
        err.read_to_end(&mut b).map(|_| b)
    });
    let status = wait_bounded(&mut child, limit);
    let out = stdout
        .join()
        .map_err(|_| RunError::Setup("stdout reader failed".into()))??;
    let err = stderr
        .join()
        .map_err(|_| RunError::Setup("stderr reader failed".into()))??;
    Ok((status?, out, err))
}
pub fn run_id(study: &str) -> Result<String, RunError> {
    if study.is_empty() || !study.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(RunError::Setup(
            "study name must be alphanumeric or hyphens".into(),
        ));
    }
    Ok(format!(
        "cfdcraft-{study}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| RunError::Setup(e.to_string()))?
            .as_nanos()
    ))
}

/// The run id is far too long for a socket path; eight hex digits of its hash
/// tell concurrent jobs apart.
fn short_tag(id: &str) -> String {
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}

/// Stop the owned container, remove it and confirm it is gone. Killing the
/// docker client does not stop the remote container.
fn stop_and_confirm(p: &HostProfile, socket: &Path, id: &str) -> bool {
    let _ = checked(
        &mut docker_cmd(p, socket, &["stop", "--time", "5", id]),
        Stage::Docker,
    );
    let _ = checked(&mut docker_cmd(p, socket, &["rm", "-f", id]), Stage::Docker);
    let remaining = text(
        &mut docker_cmd(
            p,
            socket,
            &[
                "ps",
                "--filter",
                &format!("name=^{id}$"),
                "--format",
                "{{.Names}}",
            ],
        ),
        Stage::Docker,
    );
    matches!(remaining, Ok(ref s) if s.trim().is_empty())
}

/// A started job: tunnel open, preflight passed, project lock held. `execute`
/// does the long part. Dropping a job that was never executed releases the lock
/// and closes the tunnel.
pub struct Job {
    p: HostProfile,
    key: PathBuf,
    cancel: Arc<AtomicBool>,
    id: String,
    uptime: String,
    lock: Guard,
    tunnel: Tunnel,
}

impl Job {
    /// The quick part: validate, open the tunnel, preflight, take the lock and
    /// preflight again. Returns `Busy` in seconds when the host is taken.
    pub fn begin(
        p: &HostProfile,
        key: &Path,
        config: RunConfig,
        label: &str,
    ) -> Result<Job, RunError> {
        p.validate()?;
        if !key.is_file() {
            return Err(RunError::Setup(format!(
                "key path does not exist: {}",
                key.display()
            )));
        }
        let id = run_id(label)?;
        let tunnel = Tunnel::open(p, key, &config.socket_dir, &short_tag(&id))?;
        let uptime = text(ssh_command(p, key).arg("uptime"), Stage::Ssh)?;
        let ps = text(
            &mut docker_cmd(
                p,
                &tunnel.socket_path,
                &["ps", "--format", "{{.Names}} {{.Status}} "],
            ),
            Stage::Docker,
        )?;
        preflight(p, &uptime, &ps)?;
        let uid = text(ssh_command(p, key).arg("id -u"), Stage::Ssh)?;
        if uid.trim() != p.remote_user_uid.to_string() {
            return Err(RunError::Setup("remote UID differs from profile".into()));
        }
        let lock_path = format!("{}/.cfdcraft-job-lock", p.work_dir);
        let holder = Holder::new(
            &id,
            &id,
            lock::now_epoch()?,
            p.wall_clock_seconds,
            lock::CLIENT,
        )
        .map_err(|e| RunError::Setup(e.to_string()))?;
        let lock = lock::acquire(p, key, &lock_path, &holder)?;
        // Recheck after the atomic project lock closes concurrent preflight races.
        let ps = text(
            &mut docker_cmd(p, &tunnel.socket_path, &["ps", "--format", "{{.Names}}"]),
            Stage::Docker,
        )?;
        preflight(p, &uptime, &ps)?;
        Ok(Job {
            p: p.clone(),
            key: key.to_path_buf(),
            cancel: config.cancel,
            id,
            uptime,
            lock,
            tunnel,
        })
    }

    /// The run id, which is also the name of the container this job starts.
    pub fn run_id(&self) -> &str {
        &self.id
    }

    /// The long part: upload the case, run the container, download the results.
    pub fn execute(
        mut self,
        case: &Path,
        results: &Path,
        spec: &RunSpec,
    ) -> Result<RunOutcome, RunError> {
        let (p, key, id) = (&self.p, &self.key, &self.id);
        let cancelled = || self.cancel.load(Ordering::SeqCst);
        std::fs::create_dir_all(results)?;
        let results = results.canonicalize()?;
        if cancelled() {
            return Err(RunError::Cancelled);
        }
        let remote = format!("{}/{id}", p.work_dir);
        checked(
            ssh_command(p, key).arg(format!("mkdir '{}'", remote)),
            Stage::Ssh,
        )?;
        let archive = results.join("input.tar");
        checked(
            Command::new("tar")
                .arg("-C")
                .arg(case)
                .args(["-cf"])
                .arg(&archive)
                .arg("."),
            Stage::Local,
        )?;
        checked(
            ssh_command(p, key)
                .arg(format!("tar -xf - -C '{}'", remote))
                .stdin(File::open(&archive)?),
            Stage::Ssh,
        )?;
        let args = run_args(p, &remote, id, spec).map_err(RunError::Setup)?;
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        if cancelled() {
            return Err(RunError::Cancelled);
        }
        let start = Instant::now();
        let mut child = docker_cmd(p, &self.tunnel.socket_path, &refs).spawn()?;
        let status = wait_cancellable(
            &mut child,
            Duration::from_secs(p.wall_clock_seconds),
            &self.cancel,
        );
        if !matches!(&status, Ok(s) if s.success()) {
            if !stop_and_confirm(p, &self.tunnel.socket_path, id) {
                self.lock.release = false;
                return Err(RunError::Setup(format!(
                    "could not confirm owned container {id} stopped; retained project lock for recovery"
                )));
            }
            if matches!(status, Err(RunError::Cancelled)) {
                return Err(RunError::Cancelled);
            }
        }
        let archive = results.join("results.tar");
        checked(
            ssh_command(p, key)
                .arg(format!("tar -C '{}' -cf - .", remote))
                .stdout(File::create(&archive)?),
            Stage::Ssh,
        )?;
        checked(
            Command::new("tar")
                .arg("-xf")
                .arg(&archive)
                .arg("-C")
                .arg(&results),
            Stage::Local,
        )?;
        let status = status?;
        Ok(RunOutcome {
            results_dir: results,
            remote_dir: remote,
            container_name: id.clone(),
            exit_code: status.code(),
            preflight_uptime: self.uptime.clone(),
            duration_seconds: start.elapsed().as_secs_f64(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_command_obeys_wall_clock_cap() {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "printf started; exec sleep 5"]);
            command
        };
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("powershell");
            command.args([
                "-NoProfile",
                "-Command",
                "Write-Output started; Start-Sleep -Seconds 5",
            ]);
            command
        };
        let start = Instant::now();
        assert!(matches!(
            text_bounded(&mut command, Duration::from_millis(100), Stage::Local),
            Err(RunError::TimedOut)
        ));
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
