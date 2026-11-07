use std::path::{Path, PathBuf};
use std::process::Command;

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const HOSTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cfdcraft-executor/tests/hosts.example.toml"
);
const CHANNEL: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .output()
        .unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn channel_without_outlet() -> String {
    let outlet = "    \"outlet\": {\n      \"faces\": [\"XMax\"],\n      \"kind\": \"PressureOutlet\"\n    },\n";
    let broken = CHANNEL.replace(outlet, "");
    assert_ne!(broken, CHANNEL, "the fixture changed shape");
    broken
}

#[test]
fn compile_of_a_study_with_a_missing_boundary_exits_2_and_writes_nothing() {
    let dir = scratch("missing-boundary");
    let study = dir.join("study.json");
    std::fs::write(&study, channel_without_outlet()).unwrap();
    let out = dir.join("case");
    let o = run(&["compile", study.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("CFD-BC-001") && err.contains("XMax"), "{err}");
    assert!(!out.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn warnings_are_printed_to_stderr_and_do_not_stop_the_compile() {
    let dir = scratch("warnings");
    let study = dir.join("study.json");
    let coarse = CAVITY.replace("\"cells_x\": 128", "\"cells_x\": 4");
    assert_ne!(coarse, CAVITY, "the fixture changed shape");
    std::fs::write(&study, coarse).unwrap();
    let out = dir.join("case");
    let o = run(&["compile", study.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(String::from_utf8_lossy(&o.stderr).contains("warning CFD-WARN-002"));
    assert!(out.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn an_invalid_study_never_reaches_ssh_or_docker() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("nothing-runs");
    let study = dir.join("study.json");
    std::fs::write(&study, channel_without_outlet()).unwrap();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.join("calls.log");
    for tool in ["ssh", "docker"] {
        let path = bin.join(tool);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"{tool} $@\" >> \"{}\"\nexit 1\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    for verb in ["validate", "run"] {
        let o = Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
            .env("PATH", &path)
            .args([
                verb,
                study.to_str().unwrap(),
                "--key",
                "/nonexistent/key",
                "--hosts",
                HOSTS,
                "--profile",
                "box",
            ])
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "{verb}");
        assert!(
            String::from_utf8_lossy(&o.stderr).contains("CFD-BC-001"),
            "{verb}"
        );
    }
    assert!(!log.exists() || std::fs::read_to_string(&log).unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}
