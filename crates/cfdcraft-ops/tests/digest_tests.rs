//! The solver image and the command that runs in it are each defined once. The README a person
//! follows by hand, the `docker run` the executor builds and the backend's constant all agree.
#![cfg(unix)]

mod support;

use cfdcraft_backend_esi::{Dialect, dialect::IMAGE};
use cfdcraft_executor::{
    HostProfile,
    docker::{RunSpec, run_args},
};
use cfdcraft_ops::prepare;
use support::*;

#[test]
fn the_readme_the_argv_and_the_backend_constant_name_one_image_and_one_command() {
    let root = new_root("ops-digest-tests", "one");
    let profile = HostProfile::load(&root.join("hosts.toml"), "box").unwrap();
    let remote = format!("{}/job-1", profile.work_dir);
    let spec = RunSpec::allrun(IMAGE).unwrap();
    let argv = run_args(&profile, &remote, "cfdcraft-job-1", &spec).unwrap();

    // The executor runs exactly the backend's image, then the one Allrun.
    let tail = &argv[argv.len() - 4..];
    assert_eq!(tail[0], IMAGE);
    assert_eq!(&tail[1..3], ["bash", "-lc"]);
    let script = &tail[3];
    assert_eq!(script, "cd /w && sh ./Allrun");

    // The README produced by the export names that image in the pull line and the run command.
    let prepared = prepare(CAVITY, Dialect::EsiV2512).unwrap();
    let readme = prepared
        .export_tree
        .files
        .iter()
        .find(|f| f.rel_path.to_string_lossy() == "README.md")
        .expect("the export carries a README");
    let readme = String::from_utf8(readme.bytes.clone()).unwrap();
    let pull: Vec<&str> = readme
        .lines()
        .filter(|l| l.starts_with("docker pull "))
        .collect();
    let run: Vec<&str> = readme
        .lines()
        .filter(|l| l.starts_with("docker run "))
        .collect();
    assert_eq!(pull, [format!("docker pull {IMAGE}")]);
    assert_eq!(run.len(), 1);
    assert!(run[0].contains(&format!(" {IMAGE} ")), "{}", run[0]);
    assert!(
        run[0].ends_with(&format!("bash -lc '{script}'")),
        "the README and the executor end with the same shell command: {}",
        run[0]
    );

    // No other image digest appears anywhere in the README.
    assert_eq!(readme.matches("@sha256:").count(), 2, "{readme}");
    assert_eq!(readme.matches(IMAGE).count(), 2, "{readme}");
}
