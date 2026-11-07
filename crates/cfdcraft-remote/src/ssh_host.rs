//! The real compute host: commands over ssh, containers through the forwarded
//! docker socket. Every string comes from `commands`; nothing here builds a shell
//! command of its own.

use crate::{
    commands::{
        CommandError, MAX_TEXT, check_text, df_command, download_command, files_command,
        inspect_args, parse_df, parse_du_lines, parse_inspect, parse_wc_lines, ps_label_args,
        read_text_command, remove_run_dir_command, rm_args, stop_args, tail_command,
        time_dirs_command, write_text_command,
    },
    host::{ContainerState, Host, HostError, RemoteFile, TimeDir, is_owned, valid_run_id},
};
use cfdcraft_executor::{
    Holder, HostProfile, Job, ReleaseOutcome, RunConfig, RunError, Stage, checked,
    docker::{RunSpec, docker_cmd},
    lock::{self, SSH_FAILED_EXIT},
    output_bounded,
    run::wait_bounded,
    tunnel::{Tunnel, ssh_command},
};
use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

/// The most time one ordinary remote command may take.
const COMMAND_LIMIT: Duration = Duration::from_secs(30);
/// The most time a results download may take.
const DOWNLOAD_LIMIT: Duration = Duration::from_secs(600);
/// The failure class of a host problem that is not a connection or docker problem.
const INFRA: &str = "CFD-INFRA-005";
/// The failure class of the container service.
const DOCKER: &str = "CFD-INFRA-003";

static NEXT_TUNNEL: AtomicU32 = AtomicU32::new(0);

pub struct SshHost {
    profile: HostProfile,
    key: PathBuf,
    socket_dir: PathBuf,
    scratch: PathBuf,
    image: String,
    tag: String,
    tunnel: Option<Tunnel>,
}

fn refused(reason: &'static str) -> HostError {
    HostError::Refused { reason }
}

fn failed() -> HostError {
    HostError::Failed { class: INFRA }
}

fn command_error(e: CommandError) -> HostError {
    match e {
        CommandError::Invalid { what }
        | CommandError::Bounds { what }
        | CommandError::Unreadable { what } => refused(what),
        CommandError::TextTooLarge { .. } => refused("text too large"),
    }
}

/// A connection problem is `Unreachable`, never a failed run.
fn run_error(e: RunError) -> HostError {
    let class = e.infra_code();
    match e {
        RunError::Ssh { .. } | RunError::TimedOut => HostError::Unreachable,
        RunError::Busy { stale, .. } => HostError::Busy { stale },
        other if other.is_busy() => HostError::Busy { stale: false },
        _ => HostError::Failed { class },
    }
}

/// The standard output of a remote command that must succeed.
fn success_output(status: ExitStatus, out: Vec<u8>) -> Result<Vec<u8>, HostError> {
    match status.code() {
        Some(0) => Ok(out),
        Some(SSH_FAILED_EXIT) => Err(HostError::Unreachable),
        _ => Err(failed()),
    }
}

impl SshHost {
    /// Check the profile, the key file and the image; open nothing yet.
    pub fn connect(
        profile: HostProfile,
        key: impl Into<PathBuf>,
        socket_dir: impl Into<PathBuf>,
        scratch: impl Into<PathBuf>,
        image: &str,
    ) -> Result<Self, HostError> {
        let key = key.into();
        profile
            .validate()
            .map_err(|_| refused("invalid host profile"))?;
        if !key.is_file() {
            return Err(refused("key file missing"));
        }
        RunSpec::allrun_timed(image, profile.wall_clock_seconds)
            .map_err(|_| refused("invalid image"))?;
        Ok(Self {
            profile,
            key,
            socket_dir: socket_dir.into(),
            scratch: scratch.into(),
            image: image.to_string(),
            tag: format!("h{}", NEXT_TUNNEL.fetch_add(1, Ordering::Relaxed)),
            tunnel: None,
        })
    }

    fn lock_path(&self) -> String {
        format!("{}/.cfdcraft-job-lock", self.profile.work_dir)
    }

    /// One remote shell command, its standard input from `stdin` when given.
    fn ssh(
        &self,
        command: &str,
        limit: Duration,
        stdin: Option<&Path>,
    ) -> Result<(ExitStatus, Vec<u8>), HostError> {
        let mut c = ssh_command(&self.profile, &self.key);
        c.arg(command);
        if let Some(path) = stdin {
            c.stdin(File::open(path).map_err(|_| failed())?);
        }
        let (status, out, _) = output_bounded(&mut c, limit).map_err(run_error)?;
        Ok((status, out))
    }

    /// The socket of the docker tunnel, opened on first use and again when the
    /// socket is gone.
    fn socket(&mut self) -> Result<PathBuf, HostError> {
        let open = self
            .tunnel
            .as_ref()
            .map(|t| t.socket_path.clone())
            .filter(|p| p.exists());
        if let Some(path) = open {
            return Ok(path);
        }
        self.tunnel = None;
        let tunnel = Tunnel::open(&self.profile, &self.key, &self.socket_dir, &self.tag)
            .map_err(run_error)?;
        let path = tunnel.socket_path.clone();
        self.tunnel = Some(tunnel);
        Ok(path)
    }

    /// One docker command: its standard output, or `None` when the container is
    /// not there. A failure other than that closes the tunnel, so the next call
    /// opens a fresh one.
    fn docker(&mut self, args: &[String], limit: Duration) -> Result<Option<Vec<u8>>, HostError> {
        let socket = self.socket()?;
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let result = output_bounded(&mut docker_cmd(&self.profile, &socket, &refs), limit);
        match result {
            Ok((status, out, _)) if status.success() => Ok(Some(out)),
            Ok((_, _, err)) => {
                let text = String::from_utf8_lossy(&err);
                if text.contains("No such") {
                    return Ok(None);
                }
                self.tunnel = None;
                if text.contains("Cannot connect") {
                    Err(HostError::Unreachable)
                } else {
                    Err(HostError::Failed { class: DOCKER })
                }
            }
            Err(e) => {
                self.tunnel = None;
                Err(run_error(e))
            }
        }
    }

    /// `Some(true)` when a container of this name carries this run's label,
    /// `Some(false)` when one exists without it, `None` when there is none.
    fn ownership(&mut self, run_id: &str) -> Result<Option<bool>, HostError> {
        let args = ps_label_args(run_id).map_err(command_error)?;
        let listed = self.docker(&args, COMMAND_LIMIT)?.unwrap_or_default();
        if String::from_utf8_lossy(&listed)
            .lines()
            .any(|l| l.trim() == run_id)
        {
            return Ok(Some(true));
        }
        Ok(match self.inspect(run_id)? {
            ContainerState::None => None,
            _ => Some(false),
        })
    }
}

impl Host for SshHost {
    fn use_image(&mut self, image: &str) -> Result<(), HostError> {
        RunSpec::allrun_timed(image, self.profile.wall_clock_seconds)
            .map_err(|_| refused("invalid image"))?;
        self.image = image.to_string();
        Ok(())
    }

    fn start(&mut self, run_id: &str, case: &Path, doc: &str, np: u32) -> Result<(), HostError> {
        if !is_owned(run_id) || !run_id.starts_with(&self.profile.container_prefix) {
            return Err(refused("invalid run id"));
        }
        if doc.is_empty()
            || doc.len() > 40
            || !doc.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(refused("invalid document label"));
        }
        if np == 0 || np > self.profile.core_cap {
            return Err(refused("invalid process count"));
        }
        let spec = RunSpec::allrun_timed(&self.image, self.profile.wall_clock_seconds)
            .map_err(|_| refused("invalid image"))?;
        let config = RunConfig::new(self.socket_dir.clone());
        Job::begin_as(&self.profile, &self.key, config, run_id)
            .map_err(run_error)?
            .start_detached(case, &self.scratch, &spec, doc, np)
            .map(|_| ())
            .map_err(run_error)
    }

    fn inspect(&mut self, run_id: &str) -> Result<ContainerState, HostError> {
        let args = inspect_args(run_id).map_err(command_error)?;
        match self.docker(&args, COMMAND_LIMIT)? {
            None => Ok(ContainerState::None),
            Some(out) => parse_inspect(&String::from_utf8_lossy(&out))
                .map_err(|_| HostError::Failed { class: DOCKER }),
        }
    }

    fn holder(&mut self) -> Result<Option<Holder>, HostError> {
        lock::read_holder(&self.profile, &self.key, &self.lock_path()).map_err(run_error)
    }

    fn tail(
        &mut self,
        run_id: &str,
        rel: &str,
        offset: u64,
        max: u64,
    ) -> Result<Vec<u8>, HostError> {
        let cmd = tail_command(&self.profile.work_dir, run_id, rel, offset, max)
            .map_err(command_error)?;
        let (status, out) = self.ssh(&cmd, COMMAND_LIMIT, None)?;
        success_output(status, out)
    }

    fn files(&mut self, run_id: &str, dir: &str) -> Result<Vec<RemoteFile>, HostError> {
        let cmd = files_command(&self.profile.work_dir, run_id, dir).map_err(command_error)?;
        let (status, out) = self.ssh(&cmd, COMMAND_LIMIT, None)?;
        Ok(parse_wc_lines(&String::from_utf8_lossy(&success_output(
            status, out,
        )?)))
    }

    fn time_dirs(&mut self, run_id: &str) -> Result<Vec<TimeDir>, HostError> {
        let cmd = time_dirs_command(&self.profile.work_dir, run_id).map_err(command_error)?;
        let (status, out) = self.ssh(&cmd, COMMAND_LIMIT, None)?;
        Ok(parse_du_lines(&String::from_utf8_lossy(&success_output(
            status, out,
        )?)))
    }

    fn free_kb(&mut self) -> Result<u64, HostError> {
        let cmd = df_command(&self.profile.work_dir).map_err(command_error)?;
        let (status, out) = self.ssh(&cmd, COMMAND_LIMIT, None)?;
        parse_df(&String::from_utf8_lossy(&success_output(status, out)?)).ok_or_else(failed)
    }

    fn read_text(&mut self, run_id: &str, rel: &str) -> Result<String, HostError> {
        let cmd = read_text_command(&self.profile.work_dir, run_id, rel).map_err(command_error)?;
        let (status, out) = self.ssh(&cmd, COMMAND_LIMIT, None)?;
        match status.code() {
            Some(0) if out.len() > MAX_TEXT => Err(refused("text too large")),
            Some(0) => Ok(String::from_utf8_lossy(&out).into_owned()),
            Some(SSH_FAILED_EXIT) => Err(HostError::Unreachable),
            _ => Err(refused("file not found")),
        }
    }

    fn write_text(&mut self, run_id: &str, rel: &str, text: &str) -> Result<(), HostError> {
        check_text(text).map_err(command_error)?;
        let cmd = write_text_command(&self.profile.work_dir, run_id, rel).map_err(command_error)?;
        // The text goes to the remote command on standard input, from a file of
        // our own, so it is never part of the command.
        std::fs::create_dir_all(&self.scratch).map_err(|_| failed())?;
        let input = self
            .scratch
            .join(format!("stdin-{}.txt", std::process::id()));
        std::fs::write(&input, text).map_err(|_| failed())?;
        let sent = self.ssh(&cmd, COMMAND_LIMIT, Some(&input));
        let _ = std::fs::remove_file(&input);
        success_output(sent?.0, Vec::new()).map(|_| ())
    }

    fn stop_container(&mut self, run_id: &str, grace_seconds: u32) -> Result<(), HostError> {
        let args = stop_args(run_id, grace_seconds).map_err(command_error)?;
        match self.ownership(run_id)? {
            None => Ok(()),
            Some(false) => Err(refused("not an owned container")),
            Some(true) => {
                let limit = COMMAND_LIMIT + Duration::from_secs(u64::from(grace_seconds));
                self.docker(&args, limit).map(|_| ())
            }
        }
    }

    fn download(&mut self, run_id: &str, dest: &Path) -> Result<(), HostError> {
        let cmd = download_command(&self.profile.work_dir, run_id).map_err(command_error)?;
        std::fs::create_dir_all(dest).map_err(|_| failed())?;
        let archive = dest.join("results.tar");
        let sink = File::create(&archive).map_err(|_| failed())?;
        let mut c = ssh_command(&self.profile, &self.key);
        c.arg(&cmd).stdout(sink).stderr(Stdio::null());
        let fetched = c
            .spawn()
            .map_err(|_| failed())
            .and_then(|mut child| wait_bounded(&mut child, DOWNLOAD_LIMIT).map_err(run_error));
        let result = fetched.and_then(|status| match status.code() {
            Some(0) => checked(
                Command::new("tar")
                    .arg("-xf")
                    .arg(&archive)
                    .arg("-C")
                    .arg(dest),
                Stage::Local,
            )
            .map_err(run_error),
            Some(SSH_FAILED_EXIT) => Err(HostError::Unreachable),
            _ => Err(failed()),
        });
        let _ = std::fs::remove_file(&archive);
        result
    }

    fn remove_container(&mut self, run_id: &str) -> Result<(), HostError> {
        let args = rm_args(run_id).map_err(command_error)?;
        match self.ownership(run_id)? {
            None => Ok(()),
            Some(false) => Err(refused("not an owned container")),
            Some(true) => self.docker(&args, COMMAND_LIMIT).map(|_| ()),
        }
    }

    fn remove_run_dir(&mut self, run_id: &str) -> Result<(), HostError> {
        if !run_id.starts_with(&self.profile.container_prefix) {
            return Err(refused("invalid run id"));
        }
        let cmd = remove_run_dir_command(&self.profile.work_dir, run_id).map_err(command_error)?;
        let (status, out) = self.ssh(&cmd, DOWNLOAD_LIMIT, None)?;
        success_output(status, out).map(|_| ())
    }

    fn release(&mut self, run_id: &str) -> Result<ReleaseOutcome, HostError> {
        if !valid_run_id(run_id) {
            return Err(refused("invalid run id"));
        }
        lock::release_checked(&self.profile, &self.key, &self.lock_path(), run_id)
            .map_err(run_error)
    }
}
