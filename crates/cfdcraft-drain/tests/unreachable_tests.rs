//! A host that goes quiet makes a job unreachable, never failed; and a killed process leaves no
//! tunnel socket folder behind.

use cfdcraft_drain::{Drain, TickOutcome};
use cfdcraft_queue::{Action, JobState, StopCause, StopMode};
use cfdcraft_text::{Line, Text};

mod support;
use support::*;

/// A job added and started: the drain, the id and the run name.
fn started(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    assert_eq!(drain.tick().unwrap(), TickOutcome::Advanced);
    let run_id = w.run_id(&id);
    (w, drain, id, run_id)
}

fn go_quiet(w: &World, ticks: u32) {
    w.shared.inner.borrow_mut().fail_next(ticks);
}

/// Ticks until the job is unreachable, at most `limit` times.
fn tick_until_unreachable(drain: &mut Drain<Shared>, w: &World, id: &str, limit: usize) {
    for _ in 0..limit {
        if w.state(id) == JobState::Unreachable {
            return;
        }
        drain.tick().unwrap();
    }
    assert_eq!(w.state(id), JobState::Unreachable);
}

fn mutations(w: &World) -> Vec<String> {
    w.shared
        .calls()
        .into_iter()
        .filter(|call| {
            ["stop_container", "remove_container", "release"]
                .iter()
                .any(|word| call.starts_with(word))
        })
        .collect()
}

#[test]
fn five_failed_ticks_keep_the_job_running_and_the_sixth_marks_it_unreachable() {
    let (w, mut drain, id, _) = started("quiet-six");
    go_quiet(&w, 6);
    for _ in 0..5 {
        drain.tick().unwrap();
        assert_eq!(w.state(&id), JobState::Running);
    }
    drain.tick().unwrap();
    assert_eq!(w.state(&id), JobState::Unreachable);
    assert!(mutations(&w).is_empty(), "{:?}", mutations(&w));
}

#[test]
fn an_unreachable_job_comes_back_when_the_host_answers() {
    let (w, mut drain, id, run_id) = started("quiet-back");
    w.shared.grow_log(&run_id, 1, 4);
    drain.tick().unwrap();
    let before = std::fs::metadata(w.live(&id).join("log.simpleFoam"))
        .unwrap()
        .len();
    assert!(before > 0);

    go_quiet(&w, 6);
    tick_until_unreachable(&mut drain, &w, &id, 10);
    w.shared.grow_log(&run_id, 5, 8);
    let seen = w.shared.calls().len();
    for _ in 0..4 {
        if w.state(&id) == JobState::Running {
            break;
        }
        drain.tick().unwrap();
    }
    let job = w.job(&id);
    assert_eq!(
        job.state,
        JobState::Running,
        "the next probe found the host"
    );
    let said = Text::english().render(&Line::JobReattached { job: id.clone() });
    assert_eq!(job.progress.and_then(|p| p.summary), Some(said));

    drain.tick().unwrap();
    let prefix = format!("tail {run_id} log.simpleFoam ");
    let first: u64 = w
        .shared
        .calls()
        .split_off(seen)
        .iter()
        .find_map(|call| call.strip_prefix(&prefix))
        .expect("the log was fetched again")
        .parse()
        .unwrap();
    assert_eq!(first, before, "polling resumed at the mirror length");
    assert_eq!(
        std::fs::read(w.live(&id).join("log.simpleFoam")).unwrap(),
        w.shared.file(&run_id, "log.simpleFoam").unwrap()
    );
    assert!(mutations(&w).is_empty());
}

#[test]
fn an_unreachable_job_is_probed_less_often() {
    let (w, mut drain, id, _) = started("quiet-probe");
    go_quiet(&w, 6);
    tick_until_unreachable(&mut drain, &w, &id, 10);
    go_quiet(&w, 10_000);

    let mut probed = 0;
    for _ in 0..8 {
        let seen = w.shared.calls().len();
        drain.tick().unwrap();
        if w.shared.calls().len() > seen {
            probed += 1;
        }
    }
    assert_eq!(probed, 2, "one tick in four asks the host");
    assert_eq!(w.state(&id), JobState::Unreachable);
}

#[test]
fn a_stop_request_survives_being_unreachable() {
    let (w, mut drain, id, _) = started("quiet-stop");
    go_quiet(&w, 6);
    tick_until_unreachable(&mut drain, &w, &id, 10);
    go_quiet(&w, 10_000);
    let stop = Action::RequestStop {
        id: id.clone(),
        mode: StopMode::WriteNow,
        cause: StopCause::User,
    };
    drain.persist(stop).unwrap();
    for _ in 0..6 {
        drain.tick().unwrap();
        let job = w.job(&id);
        assert_eq!(job.state, JobState::Unreachable);
        assert!(job.stop.is_some(), "the request is kept in the queue file");
    }

    go_quiet(&w, 0);
    for _ in 0..4 {
        if w.state(&id) != JobState::Unreachable {
            break;
        }
        drain.tick().unwrap();
    }
    let job = w.job(&id);
    assert_eq!(job.state, JobState::Running);
    assert!(job.stop.is_some(), "the request is still there");
}

#[test]
fn a_job_unreachable_for_a_long_time_is_never_failed_by_the_drain() {
    let (w, mut drain, id, run_id) = started("quiet-long");
    go_quiet(&w, 10_000);
    for _ in 0..200 {
        drain.tick().unwrap();
        let state = w.state(&id);
        assert!(
            matches!(state, JobState::Running | JobState::Unreachable),
            "{state:?}"
        );
    }
    assert_eq!(w.state(&id), JobState::Unreachable);
    assert!(mutations(&w).is_empty());
    go_quiet(&w, 0);
    assert_eq!(
        w.shared.container(&run_id),
        cfdcraft_remote::ContainerState::Running
    );
}

#[cfg(unix)]
mod sockets {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::Path;
    use std::process::{Child, Command};

    use cfdcraft_drain::reattach::clean_stale_sockets;

    use super::*;

    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    fn live_child() -> Child {
        Command::new("sleep").arg("30").spawn().unwrap()
    }

    fn folder(base: &Path, name: &str) {
        fs::create_dir_all(base.join(name)).unwrap();
        fs::write(base.join(name).join("d.sock"), b"x").unwrap();
    }

    #[test]
    fn stale_socket_folders_of_dead_processes_are_removed() {
        let base = scratch("sock-clean");
        let dead = dead_pid();
        let mut child = live_child();
        let me = std::process::id();
        let stale = format!("socket-{dead}-ab12cd34");
        let mine = format!("socket-{me}-ab12cd34");
        let theirs = format!("socket-{}-ab12cd34", child.id());
        let other = format!("tunnel-{dead}-ab12cd34");
        let names = [
            stale.as_str(),
            mine.as_str(),
            theirs.as_str(),
            other.as_str(),
            "socket-abc-x",
            "socket-77",
        ];
        for name in names {
            folder(&base, name);
        }
        fs::write(base.join("socket-1-x"), b"a file").unwrap();

        let removed = clean_stale_sockets(&base);
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(removed, 1);
        assert!(!base.join(&stale).exists(), "the dead one is gone");
        for kept in &names[1..] {
            assert!(base.join(kept).join("d.sock").is_file(), "{kept} is kept");
        }
        assert!(base.join("socket-1-x").is_file());
    }

    #[test]
    fn cleaning_never_leaves_the_base_folder() {
        let base = scratch("sock-link");
        let outside = scratch("sock-link-outside");
        fs::write(outside.join("keep.txt"), b"mine").unwrap();
        let name = format!("socket-{}-zz", dead_pid());
        symlink(&outside, base.join(&name)).unwrap();

        assert_eq!(clean_stale_sockets(&base), 0);
        assert!(
            fs::symlink_metadata(base.join(&name)).is_ok(),
            "the link is kept"
        );
        assert!(
            outside.join("keep.txt").is_file(),
            "its target is untouched"
        );
        assert!(base.is_dir());
    }

    #[test]
    fn starting_the_drain_cleans_the_configured_base() {
        let w = world("sock-startup");
        let base = scratch("sock-startup-base");
        let stale = format!("socket-{}-ab12cd34", dead_pid());
        folder(&base, &stale);
        let config = cfdcraft_drain::DrainConfig {
            socket_dir: Some(base.clone()),
            ..w.config()
        };
        let mut drain = Drain::new(config, w.shared.clone());
        assert_eq!(drain.tick().unwrap(), TickOutcome::Idle);
        assert!(!base.join(&stale).exists());
    }
}
