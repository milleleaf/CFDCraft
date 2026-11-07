//! Removing the folder of a run on the host: the one command template, the folders it must never
//! touch (run for real with `sh` in test folders), the in-memory box and the real host over the
//! fake ssh.

use cfdcraft_remote::{
    Host, HostError,
    commands::{PROJECT_WORK_DIR, remove_run_dir_command, remove_run_dir_in},
    fake::FakeBox,
};

#[cfg(unix)]
mod support;

const ID: &str = "cfdcraft-demo-1-2";

#[test]
fn the_command_is_one_fixed_template_for_a_validated_id() {
    let cmd = remove_run_dir_command(PROJECT_WORK_DIR, "cfdcraft-demo-1-2").unwrap();
    assert!(cmd.starts_with("r='/data/cfdcraft/work/cfdcraft-demo-1-2'; "));
    assert!(cmd.contains("readlink -f '/data/cfdcraft/work'"));
    assert!(cmd.contains("\"$w/cfdcraft-demo-1-2\""));
    assert!(cmd.ends_with("rm -rf -- \"$p\""));
    assert_eq!(cmd.matches("rm -rf").count(), 1);
}

#[test]
fn hostile_ids_and_work_dirs_are_refused() {
    for id in [
        "",
        "cfdcraft-",
        "cfdcraft-a'b",
        "cfdcraft-a b",
        "cfdcraft-a;b",
        "cfdcraft-a\nb",
        "cfdcraft-a/b",
        "cfdcraft-..",
        "cfdcraft-a/../b",
        "../cfdcraft-a",
        "/data/cfdcraft/work",
        "/data/cfdcraft/work/cfdcraft-a",
        "-rf",
        "-cfdcraft-a",
        "demo-1-2",
        ".cfdcraft-job-lock",
        "other-cfdcraft-a",
    ] {
        assert!(
            remove_run_dir_command(PROJECT_WORK_DIR, id).is_err(),
            "{id:?}"
        );
    }
    for work in [
        "",
        "/",
        "/data",
        "/data/cfdcraft",
        "/data/cfdcraft/work/",
        "/data/cfdcraft/work/x",
        "/data/cfdcraft/work/..",
        "data/cfdcraft/work",
        "/data/other/work",
        "/data/cfdcraft/work'; rm -rf /; '",
    ] {
        assert!(remove_run_dir_command(work, ID).is_err(), "{work:?}");
    }
    // The general form still refuses a work directory that could break out of its quotes.
    assert!(remove_run_dir_in("/tmp/a'b", ID).is_err());
    assert!(remove_run_dir_in("/tmp/a b", ID).is_err());
    assert!(remove_run_dir_in("/tmp/../etc", ID).is_err());
}

#[test]
fn the_default_trait_method_refuses() {
    struct Bare;
    impl Host for Bare {
        fn start(
            &mut self,
            _: &str,
            _: &std::path::Path,
            _: &str,
            _: u32,
        ) -> Result<(), HostError> {
            unreachable!()
        }
        fn inspect(&mut self, _: &str) -> Result<cfdcraft_remote::ContainerState, HostError> {
            unreachable!()
        }
        fn holder(&mut self) -> Result<Option<cfdcraft_remote::Holder>, HostError> {
            unreachable!()
        }
        fn tail(&mut self, _: &str, _: &str, _: u64, _: u64) -> Result<Vec<u8>, HostError> {
            unreachable!()
        }
        fn files(
            &mut self,
            _: &str,
            _: &str,
        ) -> Result<Vec<cfdcraft_remote::RemoteFile>, HostError> {
            unreachable!()
        }
        fn time_dirs(&mut self, _: &str) -> Result<Vec<cfdcraft_remote::TimeDir>, HostError> {
            unreachable!()
        }
        fn free_kb(&mut self) -> Result<u64, HostError> {
            unreachable!()
        }
        fn read_text(&mut self, _: &str, _: &str) -> Result<String, HostError> {
            unreachable!()
        }
        fn write_text(&mut self, _: &str, _: &str, _: &str) -> Result<(), HostError> {
            unreachable!()
        }
        fn stop_container(&mut self, _: &str, _: u32) -> Result<(), HostError> {
            unreachable!()
        }
        fn download(&mut self, _: &str, _: &std::path::Path) -> Result<(), HostError> {
            unreachable!()
        }
        fn remove_container(&mut self, _: &str) -> Result<(), HostError> {
            unreachable!()
        }
        fn release(&mut self, _: &str) -> Result<cfdcraft_remote::ReleaseOutcome, HostError> {
            unreachable!()
        }
    }
    assert!(matches!(
        Bare.remove_run_dir(ID),
        Err(HostError::Refused { .. })
    ));
}

#[test]
fn the_fake_box_removes_only_that_folder() {
    let other = "cfdcraft-demo-3-4";
    let mut b = FakeBox::new();
    b.append(ID, "log.simpleFoam", "a\n");
    b.append(ID, "0/U", "a\n");
    b.append(other, "log.simpleFoam", "b\n");
    b.set_holder(cfdcraft_remote::Holder::new(other, other, 0, 3600, "fake").ok());
    b.remove_run_dir(ID).unwrap();
    assert!(b.file(ID, "log.simpleFoam").is_none());
    assert!(b.file(ID, "0/U").is_none());
    assert!(b.file(other, "log.simpleFoam").is_some());
    assert_eq!(b.holder().unwrap().unwrap().run_id, other);
    // Removing it again is fine, and an id of another tool is refused.
    b.remove_run_dir(ID).unwrap();
    b.append("someone-else-1", "x", "x\n");
    assert!(matches!(
        b.remove_run_dir("someone-else-1"),
        Err(HostError::Refused { .. })
    ));
    assert!(b.file("someone-else-1", "x").is_some());
}

#[cfg(unix)]
mod real {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A work folder with `.cfdcraft-job-lock`, a run folder holding a link that leads out, a
    /// second run folder and a folder next to the work folder that nothing may touch.
    struct Tree {
        root: PathBuf,
        work: String,
    }

    fn make(tag: &str) -> Tree {
        let root =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("rm{}{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let work = root.join("w");
        fs::create_dir_all(work.join(".cfdcraft-job-lock")).unwrap();
        fs::write(work.join(".cfdcraft-job-lock/holder"), "h\n").unwrap();
        fs::create_dir_all(work.join(ID).join("0")).unwrap();
        fs::write(work.join(ID).join("0/U"), "u\n").unwrap();
        fs::write(work.join(ID).join("log"), "l\n").unwrap();
        fs::create_dir_all(work.join("cfdcraft-demo-3-4")).unwrap();
        fs::write(work.join("cfdcraft-demo-3-4/log"), "keep\n").unwrap();
        fs::create_dir_all(root.join("elsewhere")).unwrap();
        fs::write(root.join("elsewhere/secret"), "SECRET\n").unwrap();
        // Links inside the run folder, to a file and to a folder outside: removing the run
        // folder removes the links, never what they lead to.
        symlink(root.join("elsewhere/secret"), work.join(ID).join("leak")).unwrap();
        symlink(root.join("elsewhere"), work.join(ID).join("dirlink")).unwrap();
        Tree {
            work: work.to_string_lossy().into_owned(),
            root,
        }
    }

    fn sh(command: &str) -> bool {
        Command::new("sh")
            .arg("-c")
            .arg(command)
            .status()
            .unwrap()
            .success()
    }

    fn run(t: &Tree, id: &str) -> bool {
        sh(&remove_run_dir_in(&t.work, id).unwrap())
    }

    fn untouched(t: &Tree) {
        assert_eq!(
            fs::read_to_string(t.root.join("elsewhere/secret")).unwrap(),
            "SECRET\n"
        );
        assert!(
            Path::new(&t.work)
                .join(".cfdcraft-job-lock/holder")
                .is_file()
        );
        assert!(Path::new(&t.work).join("cfdcraft-demo-3-4/log").is_file());
    }

    #[test]
    fn a_plain_run_folder_goes_and_nothing_else() {
        let t = make("plain");
        assert!(run(&t, ID));
        assert!(!Path::new(&t.work).join(ID).exists());
        untouched(&t);
    }

    #[test]
    fn removing_a_folder_that_is_gone_is_fine_and_repeatable() {
        let t = make("again");
        assert!(run(&t, ID));
        assert!(run(&t, ID));
        assert!(run(&t, "cfdcraft-never-1-1"));
        untouched(&t);
    }

    #[test]
    fn a_run_folder_that_is_a_link_is_refused_and_not_followed() {
        let t = make("link");
        let link = Path::new(&t.work).join("cfdcraft-link-1-1");
        symlink(t.root.join("elsewhere"), &link).unwrap();
        assert!(!run(&t, "cfdcraft-link-1-1"));
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        // A link that leads to the lock folder or to another run is refused the same way.
        let to_lock = Path::new(&t.work).join("cfdcraft-lock-1-1");
        symlink(Path::new(&t.work).join(".cfdcraft-job-lock"), &to_lock).unwrap();
        assert!(!run(&t, "cfdcraft-lock-1-1"));
        let to_run = Path::new(&t.work).join("cfdcraft-run-1-1");
        symlink(Path::new(&t.work).join(ID), &to_run).unwrap();
        assert!(!run(&t, "cfdcraft-run-1-1"));
        // A link that leads nowhere is refused too.
        let dangling = Path::new(&t.work).join("cfdcraft-gone-1-1");
        symlink(t.root.join("nothing"), &dangling).unwrap();
        assert!(!run(&t, "cfdcraft-gone-1-1"));
        assert!(Path::new(&t.work).join(ID).join("log").is_file());
        untouched(&t);
    }

    #[test]
    fn a_plain_file_where_the_folder_should_be_is_refused() {
        let t = make("file");
        let file = Path::new(&t.work).join("cfdcraft-file-1-1");
        fs::write(&file, "x\n").unwrap();
        assert!(!run(&t, "cfdcraft-file-1-1"));
        assert!(file.is_file());
        untouched(&t);
    }

    #[test]
    fn a_work_folder_that_is_itself_reached_through_a_link_still_removes_the_right_folder() {
        let t = make("viaw");
        let alias = t.root.join("alias");
        symlink(&t.work, &alias).unwrap();
        let alias = alias.to_string_lossy().into_owned();
        assert!(sh(&remove_run_dir_in(&alias, ID).unwrap()));
        assert!(!Path::new(&t.work).join(ID).exists());
        untouched(&t);
    }

    #[test]
    fn odd_names_never_reach_a_shell() {
        let t = make("odd");
        let marker = t.root.join("pwned");
        for id in [
            format!("cfdcraft-a;touch {}", marker.display()),
            format!("cfdcraft-$(touch {})", marker.display()),
            "cfdcraft-`id`".to_owned(),
            "cfdcraft-a'b".to_owned(),
            "cfdcraft-a\"b".to_owned(),
            "cfdcraft-a\nb".to_owned(),
            "cfdcraft-..".to_owned(),
            "cfdcraft-a/../../elsewhere".to_owned(),
            "cfdcraft-*".to_owned(),
        ] {
            assert!(remove_run_dir_in(&t.work, &id).is_err(), "{id:?}");
        }
        assert!(!marker.exists());
        assert!(Path::new(&t.work).join(ID).is_dir());
        untouched(&t);
    }

    #[test]
    fn the_work_folder_and_the_lock_cannot_be_named_as_runs() {
        let t = make("keep");
        for id in [".", "..", "/", ".cfdcraft-job-lock", "w", t.work.as_str()] {
            assert!(remove_run_dir_in(&t.work, id).is_err(), "{id:?}");
        }
        assert!(Path::new(&t.work).is_dir());
        untouched(&t);
    }

    use super::support;

    const GROUP: &str = "remote-cleanup-tests";

    /// Run one scenario in a child with the fakes on its PATH.
    fn scenario(tag: &str) {
        let root = support::new_root(GROUP, tag);
        let c = support::worker(&root, "real::host_worker", "ok");
        support::assert_worker_ok(&c.output().unwrap());
    }

    fn connect(root: &Path) -> cfdcraft_remote::SshHost {
        cfdcraft_remote::SshHost::connect(
            support::profile(),
            root.join("dummy-key"),
            support::sockets(),
            root.join("scratch"),
            support::TEST_IMAGE,
        )
        .unwrap()
    }

    /// Child side. Does nothing unless the parent set the root.
    #[test]
    fn host_worker() {
        let Some(root) = std::env::var_os("CFC_TEST_ROOT").map(PathBuf::from) else {
            return;
        };
        let mut h = connect(&root);
        let other = "cfdcraft-demo-3-4";
        for id in [ID, other] {
            let dir = support::box_dir(&root, id);
            fs::create_dir_all(dir.join("0")).unwrap();
            fs::write(dir.join("0/U"), "u\n").unwrap();
        }
        let lock = root.join("box/.cfdcraft-job-lock");
        fs::create_dir_all(&lock).unwrap();
        h.remove_run_dir(ID).unwrap();
        assert!(!support::box_dir(&root, ID).exists());
        assert!(support::box_dir(&root, other).join("0/U").is_file());
        assert!(lock.is_dir());
        // Again, and with an id of another tool: nothing is sent for the second.
        h.remove_run_dir(ID).unwrap();
        assert!(matches!(
            h.remove_run_dir("someone-else-1"),
            Err(HostError::Refused { .. })
        ));
        let log = fs::read_to_string(root.join("ssh.log")).unwrap();
        assert!(!log.contains("someone-else-1"), "{log}");
        assert!(log.contains("rm -rf"), "{log}");
    }

    #[test]
    fn the_ssh_host_removes_the_folder_on_the_shell_fakes() {
        scenario("host");
    }
}
