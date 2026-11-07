use crate::{hosts::HostProfile, run::RunError};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub fn ssh_command(p: &HostProfile, key: &Path) -> Command {
    let mut c = Command::new("ssh");
    c.arg("-i")
        .arg(key)
        .args([
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=5",
            "-o",
            "ServerAliveCountMax=2",
        ])
        .arg(&p.ssh_target);
    c
}

/// Unix socket paths longer than this do not fit `sockaddr_un` on every
/// platform we run on.
pub const SOCKET_PATH_LIMIT: usize = 100;

pub struct Tunnel {
    pub socket_path: PathBuf,
    pub child_pid: u32,
    child: Child,
    dir: PathBuf,
}
impl Tunnel {
    /// Open a tunnel to the host's Docker socket under `base`. The directory is
    /// `socket-<pid>-<tag>`, so tunnels with different tags in one process never
    /// share a socket; the tag is short and plain because the whole path is
    /// limited.
    pub fn open(p: &HostProfile, key: &Path, base: &Path, tag: &str) -> Result<Self, RunError> {
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(RunError::Setup("tunnel tag must be alphanumeric".into()));
        }
        std::fs::create_dir_all(base)?;
        let dir = base
            .canonicalize()?
            .join(format!("socket-{}-{tag}", std::process::id()));
        let socket = dir.join("d.sock");
        if socket.as_os_str().len() >= SOCKET_PATH_LIMIT {
            return Err(RunError::SocketPathTooLong);
        }
        std::fs::create_dir(&dir)?;
        let mut c = Command::new("ssh");
        c.arg("-i")
            .arg(key)
            .args([
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ExitOnForwardFailure=yes",
                "-N",
                "-L",
            ])
            .arg(format!("{}:{}", socket.display(), p.docker_socket_remote))
            .arg(&p.ssh_target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = match c.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir(&dir);
                return Err(error.into());
            }
        };
        let mut t = Self {
            socket_path: socket,
            child_pid: child.id(),
            child,
            dir,
        };
        let start = Instant::now();
        loop {
            if t.child.try_wait()?.is_some() {
                return Err(RunError::Ssh {
                    detail: "tunnel exited before readiness".into(),
                });
            }
            if t.socket_path.exists() {
                return Ok(t);
            }
            if start.elapsed() > Duration::from_secs(10) {
                return Err(RunError::Ssh {
                    detail: "tunnel socket readiness timeout".into(),
                });
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
        let _ = std::fs::remove_dir(&self.dir);
    }
}
pub fn close(t: Tunnel) {
    drop(t);
}
