//! The shared job lock on the compute host: one directory taken with one atomic
//! remote `mkdir`. Whoever takes it records a small holder file inside, so a
//! second client can say who it is waiting for. A busy lock is "try later", not
//! a failure, and a stale lock is only ever reported, never broken.

use crate::{
    hosts::HostProfile,
    run::{RunError, Stage, checked, output_bounded},
    tunnel::ssh_command,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Remote exit status: the lock directory exists, someone else holds it.
pub const BUSY_EXIT: i32 = 75;
/// Remote exit status: the lock was taken but the holder file could not be written.
pub const HOLDER_WRITE_FAILED_EXIT: i32 = 74;
/// ssh's own exit status when the connection or login failed.
pub const SSH_FAILED_EXIT: i32 = 255;
/// How long past its wall-clock cap a lock may sit before it is called stale.
pub const STALE_MARGIN_SECONDS: u64 = 300;
/// Label written into the holder file.
pub const CLIENT: &str = concat!("cfdcraft-executor-", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
#[error("invalid lock holder: {0}")]
pub struct InvalidHolder(&'static str);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    pub run_id: String,
    pub container: String,
    pub started_epoch: u64,
    pub cap_seconds: u64,
    pub client: String,
}

fn all(s: &str, ok: impl Fn(char) -> bool) -> bool {
    !s.is_empty() && s.len() <= 128 && s.chars().all(ok)
}

impl Holder {
    /// These strings end up inside a remote shell command, so the character
    /// sets are narrow on purpose.
    pub fn new(
        run_id: &str,
        container: &str,
        started_epoch: u64,
        cap_seconds: u64,
        client: &str,
    ) -> Result<Self, InvalidHolder> {
        let h = Self {
            run_id: run_id.into(),
            container: container.into(),
            started_epoch,
            cap_seconds,
            client: client.into(),
        };
        h.validate()?;
        Ok(h)
    }

    fn validate(&self) -> Result<(), InvalidHolder> {
        let id = |c: char| c.is_ascii_alphanumeric() || c == '-';
        if !all(&self.run_id, id) {
            return Err(InvalidHolder("run id"));
        }
        if !all(&self.container, id) {
            return Err(InvalidHolder("container name"));
        }
        if !all(&self.client, |c| {
            c.is_ascii_alphanumeric() || "._-".contains(c)
        }) {
            return Err(InvalidHolder("client label"));
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("holder fields serialise")
    }

    /// Seconds until the holder's own cap runs out; never negative.
    pub fn remaining_seconds(&self, now: u64) -> u64 {
        self.started_epoch
            .saturating_add(self.cap_seconds)
            .saturating_sub(now)
    }

    /// True once the cap plus `margin` is strictly in the past.
    pub fn is_stale(&self, now: u64, margin: u64) -> bool {
        now > self
            .started_epoch
            .saturating_add(self.cap_seconds)
            .saturating_add(margin)
    }
}

/// The holder printed by a refused acquire. A lock without a holder file, or
/// anything that is not a valid holder, gives `None`: the lock is still busy.
pub fn parse_busy_output(stdout: &str) -> Option<Holder> {
    let h: Holder = serde_json::from_str(stdout.trim()).ok()?;
    h.validate().ok()?;
    Some(h)
}

fn plain_path(path: &str) -> bool {
    all(path, |c| c.is_ascii_alphanumeric() || "/._-".contains(c))
}

/// One remote command: take the directory atomically, then write the holder
/// inside it. If the holder cannot be written the directory is removed again
/// (exit 74). If the directory could not be made because it exists, print the
/// holder and exit 75. Any other `mkdir` failure (missing parent, permissions)
/// is exit 73 and is a setup problem, not a busy lock.
pub fn acquire_command(path: &str, holder_json: &str) -> Result<String, InvalidHolder> {
    if !plain_path(path) {
        return Err(InvalidHolder("lock path"));
    }
    if holder_json.contains('\'') {
        return Err(InvalidHolder("holder text"));
    }
    // Re-validate what will be sent, whoever built the JSON.
    if parse_busy_output(holder_json).is_none() {
        return Err(InvalidHolder("holder text"));
    }
    Ok(format!(
        "d='{path}'; if mkdir \"$d\" 2>/dev/null; then \
         if printf '%s\\n' '{holder_json}' > \"$d/holder.json\"; then exit 0; fi; \
         rm -f \"$d/holder.json\"; rmdir \"$d\"; exit {HOLDER_WRITE_FAILED_EXIT}; \
         elif [ -d \"$d\" ]; then cat \"$d/holder.json\" 2>/dev/null; exit {BUSY_EXIT}; \
         else exit 73; fi"
    ))
}

pub fn release_command(path: &str) -> Result<String, InvalidHolder> {
    if !plain_path(path) {
        return Err(InvalidHolder("lock path"));
    }
    Ok(format!("rm -f '{path}/holder.json'; rmdir '{path}'"))
}

/// Holds the remote lock. Dropping it releases the directory unless `release`
/// was cleared, which is done on purpose when the container could not be
/// confirmed stopped.
pub struct Guard {
    p: HostProfile,
    key: PathBuf,
    path: String,
    pub release: bool,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.release {
            return;
        }
        if let Ok(cmd) = release_command(&self.path) {
            let _ = checked(
                ssh_command(&self.p, &self.key)
                    .arg(cmd)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null()),
                Stage::Ssh,
            );
        }
    }
}

pub fn now_epoch() -> Result<u64, RunError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| RunError::Setup(e.to_string()))
}

/// Take the lock or say why not: busy (with the holder when there is one),
/// ssh failed, or a plain setup error.
pub fn acquire(
    p: &HostProfile,
    key: &Path,
    path: &str,
    holder: &Holder,
) -> Result<Guard, RunError> {
    let cmd =
        acquire_command(path, &holder.to_json()).map_err(|e| RunError::Setup(e.to_string()))?;
    let (status, out, err) = output_bounded(ssh_command(p, key).arg(cmd), Duration::from_secs(30))?;
    match status.code() {
        Some(0) => Ok(Guard {
            p: p.clone(),
            key: key.to_path_buf(),
            path: path.to_string(),
            release: true,
        }),
        Some(BUSY_EXIT) => {
            let holder = parse_busy_output(&String::from_utf8_lossy(&out));
            let now = now_epoch()?;
            let stale = holder
                .as_ref()
                .is_some_and(|h| h.is_stale(now, STALE_MARGIN_SECONDS));
            Err(RunError::Busy { holder, stale })
        }
        Some(SSH_FAILED_EXIT) => Err(RunError::Ssh {
            detail: String::from_utf8_lossy(&err).trim().to_string(),
        }),
        other => Err(RunError::Setup(format!(
            "could not take the job lock (exit {}): {}",
            other.map_or("signal".to_string(), |c| c.to_string()),
            String::from_utf8_lossy(&err).trim()
        ))),
    }
}
