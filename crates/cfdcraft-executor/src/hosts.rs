use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostProfile {
    pub ssh_target: String,
    pub docker_socket_remote: String,
    pub work_dir: String,
    pub cpus: u32,
    pub memory_gb: u32,
    pub wall_clock_seconds: u64,
    pub concurrency: u32,
    pub load_threshold: f64,
    pub container_prefix: String,
    pub remote_user_uid: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum PreflightError {
    #[error("host busy: load {load_1min} >= threshold {threshold}")]
    Busy { load_1min: f64, threshold: f64 },
    #[error("existing cfdcraft containers: {names:?}")]
    ExistingCfdcraftContainers { names: Vec<String> },
    #[error("cannot parse host load from uptime")]
    InvalidLoad,
    #[error("invalid host profile: {0}")]
    InvalidProfile(String),
}

impl HostProfile {
    pub fn load(path: &Path, name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        #[derive(Deserialize)]
        struct Profiles {
            profiles: BTreeMap<String, HostProfile>,
        }
        let mut all: Profiles = toml::from_str(&std::fs::read_to_string(path)?)?;
        let p = all
            .profiles
            .remove(name)
            .ok_or_else(|| format!("unknown host profile {name}"))?;
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<(), PreflightError> {
        // The host named in the local hosts.toml is the one you approved; here we
        // pin the project directory, socket and caps, and refuse a target that
        // could be read as an ssh option.
        if self.ssh_target.is_empty()
            || self.ssh_target.starts_with('-')
            || self.ssh_target.contains(char::is_whitespace)
            || self.docker_socket_remote != "/var/run/docker.sock"
            || self.work_dir != "/data/cfdcraft/work"
            || self.container_prefix != "cfdcraft-"
            || self.concurrency != 1
            || self.cpus == 0
            || self.cpus > 2
            || self.memory_gb == 0
            || self.memory_gb > 8
            || self.wall_clock_seconds == 0
            || self.wall_clock_seconds > 3600
            || !self.load_threshold.is_finite()
            || self.load_threshold <= 0.0
        {
            return Err(PreflightError::InvalidProfile(
                "a plain ssh target, the project directory, caps and concurrency required".into(),
            ));
        }
        Ok(())
    }
}

pub fn preflight(p: &HostProfile, uptime: &str, ps: &str) -> Result<(), PreflightError> {
    p.validate()?;
    let load = uptime
        .split("load average:")
        .nth(1)
        .or_else(|| uptime.split("load averages:").nth(1))
        .and_then(|s| s.trim().split([',', ' ']).next())
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|x| x.is_finite() && *x >= 0.0)
        .ok_or(PreflightError::InvalidLoad)?;
    if load >= p.load_threshold {
        return Err(PreflightError::Busy {
            load_1min: load,
            threshold: p.load_threshold,
        });
    }
    let names: Vec<_> = ps
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|s| s.starts_with(&p.container_prefix))
        .map(str::to_string)
        .collect();
    if !names.is_empty() {
        return Err(PreflightError::ExistingCfdcraftContainers { names });
    }
    Ok(())
}
