use crate::hosts::HostProfile;
use std::{
    collections::HashMap,
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
};

/// What the container runs. A closed set: no constructor takes a command string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCommand {
    /// `sh ./Allrun` from the uploaded case. The `cd /w` is required: the image
    /// entrypoint resets the working directory to the user's home.
    Allrun,
}

/// An image reference pinned by digest plus one allowlisted command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    image: String,
    command: RunCommand,
}

/// `<repository>@sha256:<64 lowercase hex>`; the repository is lowercase path
/// components of `[a-z0-9._-]` and carries no tag, port or whitespace.
fn valid_digest_reference(image: &str) -> bool {
    let Some((repo, hex)) = image.split_once("@sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && !repo.is_empty()
        && repo.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with(['.', '-'])
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        })
}

impl RunSpec {
    pub fn allrun(image: &str) -> Result<Self, String> {
        if !valid_digest_reference(image) {
            return Err("image must be <repository>@sha256:<64 lowercase hex>".into());
        }
        Ok(Self {
            image: image.into(),
            command: RunCommand::Allrun,
        })
    }

    pub fn image(&self) -> &str {
        &self.image
    }

    pub fn command(&self) -> RunCommand {
        self.command
    }

    fn script(&self) -> &'static str {
        match self.command {
            RunCommand::Allrun => "cd /w && sh ./Allrun",
        }
    }
}

pub fn scrubbed_env() -> HashMap<OsString, OsString> {
    std::env::vars_os()
        .filter(|(k, _)| {
            !["DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_CONFIG"]
                .iter()
                .any(|v| k == v)
        })
        .collect()
}

pub fn docker_cmd(_p: &HostProfile, socket: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("docker");
    // Shares none of the caller's pipes (see `ssh_command`); a step that needs one sets its own.
    c.stdin(Stdio::null()).stdout(Stdio::null());
    c.env_clear()
        .envs(scrubbed_env())
        .arg("-H")
        .arg(format!("unix://{}", socket.display()))
        .args(args);
    c
}

pub fn run_args(
    p: &HostProfile,
    remote: &str,
    name: &str,
    spec: &RunSpec,
) -> Result<Vec<String>, String> {
    p.validate().map_err(|e| e.to_string())?;
    if !name.starts_with(&p.container_prefix)
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("invalid owned container name".into());
    }
    if !remote.starts_with(&(p.work_dir.clone() + "/"))
        || !remote
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/-_".contains(c))
    {
        return Err("invalid project work directory".into());
    }
    Ok(vec![
        "run".into(),
        "--rm".into(),
        "--pull=never".into(),
        "--name".into(),
        name.into(),
        "--cpus".into(),
        p.cpus.to_string(),
        "--memory".into(),
        format!("{}g", p.memory_gb),
        "--memory-swap".into(),
        format!("{}g", p.memory_gb),
        "--pids-limit".into(),
        "256".into(),
        "--network".into(),
        "none".into(),
        "--user".into(),
        format!("{0}:{0}", p.remote_user_uid),
        "-v".into(),
        format!("{remote}:/w"),
        spec.image().into(),
        "bash".into(),
        "-lc".into(),
        spec.script().into(),
    ])
}
