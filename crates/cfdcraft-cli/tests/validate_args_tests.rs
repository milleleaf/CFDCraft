use std::process::Command;
fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cfdcraft"))
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn missing_file_is_setup_error() {
    let o = run(&[
        "validate",
        "/nonexistent/study.json",
        "--key",
        "/nonexistent/key",
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cannot read study"));
}
#[test]
fn missing_key_is_usage_error() {
    let o = run(&["validate", "anything.json"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--key"));
}
#[test]
fn help_lists_supported_verbs() {
    let o = run(&["--help"]);
    assert!(o.status.success());
    let s = String::from_utf8_lossy(&o.stdout);
    for v in ["compile", "run", "validate"] {
        assert!(s.contains(v));
    }
}

const CHANNEL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cfdcraft-model/tests/fixtures/channel_v1.json"
);
const CHANNEL_GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cfdcraft-backend-esi/tests/golden/channel-v2512"
);

fn files(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            all.extend(files(&path));
        } else {
            all.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
    all.sort();
    all
}

#[test]
fn validate_refuses_a_channel_before_any_host_work() {
    // The key and host file do not exist: reaching either would name them in the message.
    let o = run(&[
        "validate",
        CHANNEL,
        "--key",
        "/nonexistent/key",
        "--hosts",
        "/nonexistent/hosts.toml",
    ]);
    assert_eq!(o.status.code(), Some(2));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("lid-driven cavity"), "{err}");
    assert!(!err.contains("hosts.toml"), "{err}");
}

#[test]
fn compile_writes_the_channel_golden() {
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("channel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let o = run(&["compile", CHANNEL, out.to_str().unwrap()]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let golden = std::path::Path::new(CHANNEL_GOLDEN);
    let strip = |v: Vec<(std::path::PathBuf, Vec<u8>)>, base: &std::path::Path| {
        v.into_iter()
            .map(|(p, b)| (p.strip_prefix(base).unwrap().to_owned(), b))
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(files(&out), &out), strip(files(golden), golden));
    std::fs::remove_dir_all(out).unwrap();
}
