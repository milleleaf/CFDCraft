use crate::{
    docker::{docker_cmd, run_args},
    hosts::{HostProfile, preflight},
    tunnel::{Tunnel, ssh_command},
};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("preflight: {0}")]
    Preflight(#[from] crate::hosts::PreflightError),
    #[error("wall-clock cap exceeded")]
    TimedOut,
    #[error("{0}")]
    Setup(String),
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
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait()? {
            return Ok(s);
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn checked(c: &mut Command) -> Result<(), RunError> {
    let mut child = c.spawn()?;
    let s = wait_bounded(&mut child, Duration::from_secs(60))?;
    if !s.success() {
        return Err(RunError::Setup(format!("command failed: {s}")));
    }
    Ok(())
}
fn text(c: &mut Command) -> Result<String, RunError> {
    text_bounded(c, Duration::from_secs(30))
}
fn text_bounded(c: &mut Command, limit: Duration) -> Result<String, RunError> {
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
    if !status?.success() {
        return Err(RunError::Setup(String::from_utf8_lossy(&err).into_owned()));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
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
struct RemoteLock<'a> {
    p: &'a HostProfile,
    key: &'a Path,
    path: String,
    release: bool,
}
impl Drop for RemoteLock<'_> {
    fn drop(&mut self) {
        if !self.release {
            return;
        }
        let _ = checked(
            ssh_command(self.p, self.key)
                .arg(format!("rmdir '{}'", self.path))
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        );
    }
}

pub fn run_case(
    p: &HostProfile,
    key: &Path,
    case: &Path,
    study: &str,
    results: &Path,
) -> Result<RunOutcome, RunError> {
    p.validate()?;
    if !key.is_file() {
        return Err(RunError::Setup(format!(
            "key path does not exist: {}",
            key.display()
        )));
    }
    let id = run_id(study)?;
    std::fs::create_dir_all(results)?;
    let results = results.canonicalize()?;
    let base = std::env::current_dir()?.join("target/cfdcraft-sockets");
    std::fs::create_dir_all(&base)?;
    let t = Tunnel::open(p, key, &base)?;
    let uptime = text(ssh_command(p, key).arg("uptime"))?;
    let ps = text(&mut docker_cmd(
        p,
        &t.socket_path,
        &["ps", "--format", "{{.Names}} {{.Status}} "],
    ))?;
    preflight(p, &uptime, &ps)?;
    let uid = text(ssh_command(p, key).arg("id -u"))?;
    if uid.trim() != p.remote_user_uid.to_string() {
        return Err(RunError::Setup("remote UID differs from profile".into()));
    }
    let lock_path = format!("{}/.cfdcraft-job-lock", p.work_dir);
    checked(ssh_command(p, key).arg(format!("mkdir '{}'", lock_path)))?;
    let mut lock = RemoteLock {
        p,
        key,
        path: lock_path,
        release: true,
    };
    // Recheck after the atomic project lock closes concurrent preflight races.
    let ps = text(&mut docker_cmd(
        p,
        &t.socket_path,
        &["ps", "--format", "{{.Names}}"],
    ))?;
    preflight(p, &uptime, &ps)?;
    let remote = format!("{}/{id}", p.work_dir);
    checked(ssh_command(p, key).arg(format!("mkdir '{}'", remote)))?;
    let archive = results.join("input.tar");
    checked(
        Command::new("tar")
            .arg("-C")
            .arg(case)
            .args(["-cf"])
            .arg(&archive)
            .arg("."),
    )?;
    checked(
        ssh_command(p, key)
            .arg(format!("tar -xf - -C '{}'", remote))
            .stdin(File::open(&archive)?),
    )?;
    let args = run_args(p, &remote, &id).map_err(RunError::Setup)?;
    let refs: Vec<_> = args.iter().map(String::as_str).collect();
    let start = Instant::now();
    let mut child = docker_cmd(p, &t.socket_path, &refs).spawn()?;
    let status = wait_bounded(&mut child, Duration::from_secs(p.wall_clock_seconds));
    if !matches!(&status,Ok(s) if s.success()) {
        // Killing the Docker client alone does not stop the remote container.
        let _ = checked(&mut docker_cmd(
            p,
            &t.socket_path,
            &["stop", "--time", "5", &id],
        ));
        let _ = checked(&mut docker_cmd(p, &t.socket_path, &["rm", "-f", &id]));
        let remaining = text(&mut docker_cmd(
            p,
            &t.socket_path,
            &[
                "ps",
                "--filter",
                &format!("name=^{id}$"),
                "--format",
                "{{.Names}}",
            ],
        ));
        if !matches!(remaining,Ok(ref s) if s.trim().is_empty()) {
            lock.release = false;
            return Err(RunError::Setup(format!(
                "could not confirm owned container {id} stopped; retained project lock for recovery"
            )));
        }
    }
    let archive = results.join("results.tar");
    checked(
        ssh_command(p, key)
            .arg(format!("tar -C '{}' -cf - .", remote))
            .stdout(File::create(&archive)?),
    )?;
    checked(
        Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&results),
    )?;
    let status = status?;
    Ok(RunOutcome {
        results_dir: results,
        remote_dir: remote,
        container_name: id,
        exit_code: status.code(),
        preflight_uptime: uptime,
        duration_seconds: start.elapsed().as_secs_f64(),
    })
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
            text_bounded(&mut command, Duration::from_millis(100)),
            Err(RunError::TimedOut)
        ));
        assert!(start.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    fn failed_client_scenario(mode: &str) {
        use std::os::unix::fs::PermissionsExt;

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/executor-failure-tests")
            .join(format!("{}-{mode}", std::process::id()));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("case")).unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::write(root.join("dummy-key"), "not a credential").unwrap();
        let archive = root.join("empty.tar");
        assert!(
            Command::new("tar")
                .arg("-C")
                .arg(root.join("empty"))
                .arg("-cf")
                .arg(&archive)
                .arg(".")
                .status()
                .unwrap()
                .success()
        );
        let ssh = r#"#!/bin/sh
for arg do
    if [ "$arg" = -N ]; then tunnel=1; fi
    case "$arg" in *:/var/run/docker.sock) socket=${arg%:/var/run/docker.sock};; esac
    last=$arg
done
if [ "${tunnel:-0}" = 1 ]; then
    : > "$socket"
    exec sleep 30
fi
case "$last" in
    uptime) printf 'load average: 0.1, 0.1, 0.1\n';;
    'id -u') printf '1000\n';;
    "mkdir '/data/cfdcraft/work/.cfdcraft-job-lock'") : > "$CFC_TEST_ROOT/lock";;
    "rmdir '/data/cfdcraft/work/.cfdcraft-job-lock'")
        printf 'unlock\n' >> "$CFC_TEST_ROOT/events"
        rm "$CFC_TEST_ROOT/lock";;
    mkdir*) :;;
    'tar -xf'*) cat > /dev/null;;
    'tar -C'*) cat "$CFC_TEST_ROOT/empty.tar";;
    *) printf 'unexpected ssh command: %s\n' "$last" >&2; exit 90;;
esac
"#;
        let docker = r#"#!/bin/sh
shift 2
command=$1
shift
printf '%s\n' "$command" >> "$CFC_TEST_ROOT/events"
case "$command" in
    ps) if [ -f "$CFC_TEST_ROOT/running" ]; then cat "$CFC_TEST_ROOT/running"; fi;;
    run)
        while [ "$#" -gt 0 ]; do
            if [ "$1" = --name ]; then shift; printf '%s\n' "$1" > "$CFC_TEST_ROOT/running"; break; fi
            shift
        done
        exit 1;;
    stop|rm)
        if [ "$CFC_TEST_MODE" = cleanup-fails ]; then exit 1; fi
        rm -f "$CFC_TEST_ROOT/running";;
    *) printf 'unexpected docker command: %s\n' "$command" >&2; exit 91;;
esac
"#;
        for (name, script) in [("ssh", ssh), ("docker", docker)] {
            let path = root.join("bin").join(name);
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Isolate PATH and fixture state in another test process. Concurrent tests
        // never mutate the parent's environment or invoke real SSH/Docker.
        let path = std::env::join_paths(
            std::iter::once(root.join("bin"))
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "run::tests::fake_failed_client_worker",
                "--nocapture",
            ])
            .env("PATH", path)
            .env("CFC_TEST_ROOT", &root)
            .env("CFC_TEST_MODE", mode)
            .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "worker failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let events = std::fs::read_to_string(root.join("events")).unwrap();
        let actions: Vec<_> = events.lines().collect();
        let run = actions.iter().position(|x| *x == "run").unwrap();
        assert_eq!(&actions[run..run + 4], &["run", "stop", "rm", "ps"]);
        if mode == "cleanup-fails" {
            assert!(root.join("lock").exists());
            assert!(root.join("running").exists());
            assert!(!actions.contains(&"unlock"));
        } else {
            assert!(!root.join("running").exists());
            assert!(!root.join("lock").exists());
            assert_eq!(actions.last(), Some(&"unlock"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_docker_client_stops_owned_container_before_unlock() {
        failed_client_scenario("cleanup-succeeds");
    }

    #[cfg(unix)]
    #[test]
    fn unconfirmed_container_cleanup_retains_project_lock() {
        failed_client_scenario("cleanup-fails");
    }

    #[cfg(unix)]
    #[test]
    fn fake_failed_client_worker() {
        let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
            return;
        };
        let profile = HostProfile::load(
            Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/hosts.example.toml"
            )),
            "box",
        )
        .unwrap();
        let outcome = run_case(
            &profile,
            &root.join("dummy-key"),
            &root.join("case"),
            "regression",
            &root.join("results"),
        );
        if std::env::var("CFC_TEST_MODE").unwrap() == "cleanup-fails" {
            assert!(
                matches!(outcome, Err(RunError::Setup(message)) if message.contains("retained project lock"))
            );
        } else {
            assert_eq!(outcome.unwrap().exit_code, Some(1));
        }
    }
}
