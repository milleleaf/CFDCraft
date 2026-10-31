use cfdcraft_executor::{
    HostProfile,
    docker::{docker_cmd, scrubbed_env},
};
use std::path::Path;

#[test]
fn docker_environment_and_socket_are_explicit() {
    let p = HostProfile::load(
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/hosts.example.toml"
        )),
        "box",
    )
    .unwrap();
    let e = scrubbed_env();
    for k in ["DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_CONFIG"] {
        assert!(!e.contains_key(std::ffi::OsStr::new(k)));
    }
    let c = docker_cmd(&p, Path::new("/example/d.sock"), &["ps"]);
    let args: Vec<_> = c
        .get_args()
        .map(|x| x.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["-H", "unix:///example/d.sock", "ps"]);
    for (k, v) in c.get_envs() {
        if v.is_some() {
            assert!(
                !["DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_CONFIG"]
                    .iter()
                    .any(|x| k == *x)
            );
        }
    }
}

#[test]
#[cfg(unix)]
fn refused_tunnel_cleans_its_socket_directory() {
    use cfdcraft_executor::tunnel::Tunnel;
    let mut p = HostProfile::load(
        Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/hosts.example.toml"
        )),
        "box",
    )
    .unwrap();
    p.ssh_target = "cfdcraft-test@127.0.0.1".into();
    let base = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/tunnel-test"
    ));
    let r = Tunnel::open(&p, Path::new("/nonexistent-cfdcraft-test-key"), base);
    assert!(r.is_err());
    assert!(!base.join(format!("socket-{}", std::process::id())).exists());
}
