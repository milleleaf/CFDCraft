use crate::hosts::HostProfile;
use std::{collections::HashMap, ffi::OsString, path::Path, process::Command};

pub const IMAGE: &str = "opencfd/openfoam-default@sha256:33fb575aa9980d2bc42fd58c75ae698c489293ba30c991380fe3f899c622f319";

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
    c.env_clear()
        .envs(scrubbed_env())
        .arg("-H")
        .arg(format!("unix://{}", socket.display()))
        .args(args);
    c
}

pub fn run_args(p: &HostProfile, remote: &str, name: &str) -> Result<Vec<String>, String> {
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
    Ok(vec!["run".into(),"--rm".into(),"--pull=never".into(),"--name".into(),name.into(),"--cpus".into(),p.cpus.to_string(),"--memory".into(),format!("{}g",p.memory_gb),"--memory-swap".into(),format!("{}g",p.memory_gb),"--pids-limit".into(),"256".into(),"--network".into(),"none".into(),"--user".into(),format!("{0}:{0}",p.remote_user_uid),"-v".into(),format!("{remote}:/w"),IMAGE.into(),"bash".into(),"-lc".into(),"cd /w && blockMesh > log.blockMesh 2>&1 && checkMesh > log.checkMesh 2>&1 && simpleFoam > log.simpleFoam 2>&1".into()])
}
