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
