//! The verbs for the job queue of a study file: put jobs in it, list it, move a waiting job, cancel
//! or stop a job, and start the runner that advances it. Each one calls the matching operation and
//! prints its outcome; none holds queue logic.
//!
//! The runner is a process of its own that goes on after the terminal is closed. The key file is
//! handed to the compute host connection and nowhere else: no output of these verbs names the key
//! or the ssh target, and the runner is started with no standard streams.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use cfdcraft_backend_esi::dialect::IMAGE;
use cfdcraft_doc::store_dir;
use cfdcraft_drain::{DrainConfig, DrainEnd, run_drain, spawn_detached};
use cfdcraft_executor::HostProfile;
use cfdcraft_ops::queue_file::QueueFile;
use cfdcraft_ops::queue_ops::{
    AddRequest, queue_add, queue_cancel, queue_list, queue_reorder, queue_stop,
};
use cfdcraft_ops::repro::JobView;
use cfdcraft_ops::{ExitClass, Report, Repro};
use cfdcraft_remote::SshHost;
use cfdcraft_text::{Line, Text};

use super::{Error, emit_doc, json_with};

/// How many idle looks in a row end a foreground runner when the person did not say.
const IDLE_EXIT_AFTER: u32 = 12;
/// What a person is told when the compute host cannot be set up. It names no path and no target.
const HOST_SETUP: &str =
    "the compute host could not be set up; check the key file, the hosts file and the profile name";

/// What the `queue add` verb was asked.
pub struct AddArgs {
    pub doc: PathBuf,
    pub scenario: Option<String>,
    pub sweep: Option<String>,
    pub continue_run: Option<String>,
    pub remake_pruned: bool,
    pub json: bool,
}

/// Put jobs in the queue: the study, a variant, every point of a sweep, or the continuation of a
/// stopped run.
pub fn add(args: &AddArgs) -> Result<i32, Error> {
    let request = AddRequest {
        scenario: args.scenario.clone(),
        sweep: args.sweep.clone(),
        remake_pruned: args.remake_pruned,
        continue_run: args.continue_run.clone(),
    };
    Ok(emit_doc(&queue_add(&args.doc, &request), args.json))
}

/// One line for a job: its id, state, variant or `-`, and what it is doing or why it ended.
fn row(job: &JobView) -> String {
    let scenario = job.scenario.as_deref().unwrap_or("-");
    let detail = job
        .progress_line
        .as_deref()
        .or(job.code.as_deref())
        .unwrap_or_default();
    format!("{:<8} {:<12} {:<24} {detail}", job.id, job.state, scenario)
        .trim_end()
        .to_owned()
}

/// The jobs of the queue in the order they start. In text a table, one line for each job.
pub fn list(doc: &Path, json: bool) -> Result<i32, Error> {
    let outcome = queue_list(doc);
    let report = &outcome.report;
    if json {
        println!("{}", report.to_json());
    } else {
        if report.class() == ExitClass::Passed
            && let Some(Repro::Queue(data)) = &report.repro
        {
            for job in &data.jobs {
                println!("{}", row(job));
            }
        }
        eprint!("{}", report.render_text());
    }
    Ok(i32::from(report.exit_class))
}

/// Move a waiting job. `to` counts the waiting jobs from 1, which is the first to start.
pub fn reorder(doc: &Path, job: &str, to: u32, json: bool) -> Result<i32, Error> {
    let place = usize::try_from(to.saturating_sub(1)).unwrap_or(usize::MAX);
    Ok(emit_doc(&queue_reorder(doc, job, place), json))
}

/// Cancel a job: a waiting one ends at once, a running one is asked to stop.
pub fn cancel(doc: &Path, job: &str, json: bool) -> Result<i32, Error> {
    Ok(emit_doc(&queue_cancel(doc, job), json))
}

/// Ask a running job to write its latest results and stop.
pub fn stop(doc: &Path, job: &str, json: bool) -> Result<i32, Error> {
    Ok(emit_doc(&queue_stop(doc, job), json))
}

/// What the `queue drain` verb was asked.
pub struct DrainArgs {
    pub doc: PathBuf,
    pub key: PathBuf,
    pub profile: String,
    pub hosts: PathBuf,
    pub socket_dir: Option<PathBuf>,
    pub foreground: bool,
    pub idle_exit: Option<u32>,
    pub json: bool,
}

/// The longest the name of a tunnel socket below the base can be: `/socket-<process>-h<count>/d.sock`
/// with a ten-digit process number and count.
#[cfg(all(test, unix))]
const LONGEST_SOCKET_SUFFIX: usize = 37;

/// The folder the tunnel sockets are made in when the person chose none. The whole socket path has
/// a limit, so the base is short.
#[cfg(unix)]
fn socket_base_path() -> PathBuf {
    PathBuf::from("/tmp/cfdcraft-sock")
}

#[cfg(not(unix))]
fn socket_base_path() -> PathBuf {
    std::env::temp_dir().join("cfdcraft-sock")
}

/// The default socket folder, made if it is not there. On unix only its owner may enter it.
fn default_socket_base() -> Result<PathBuf, Error> {
    let base = socket_base_path();
    let unusable =
        "the folder for the tunnel sockets cannot be used; choose another with --socket-dir";
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&base)
            .map_err(|_| unusable)?;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| unusable)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(&base).map_err(|_| unusable)?;
    Ok(base)
}

/// A path made absolute, so the runner does not depend on the folder it is started from.
fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Start the runner that advances the queue of the study file, or run it in this process.
pub fn drain(args: &DrainArgs) -> Result<i32, Error> {
    if !args.doc.is_file() {
        return Err("the study file cannot be read".into());
    }
    if !args.key.is_file() || !args.hosts.is_file() {
        return Err("the key file or the hosts file cannot be read".into());
    }
    if args.foreground {
        foreground(args)
    } else {
        background(args)
    }
}

/// The arguments the detached runner is started with: this program's own verb, in the foreground.
fn runner_arguments(args: &DrainArgs) -> Vec<String> {
    let mut words = vec![
        "queue".to_owned(),
        "drain".to_owned(),
        absolute(&args.doc),
        "--foreground".to_owned(),
        "--key".to_owned(),
        absolute(&args.key),
        "--hosts".to_owned(),
        absolute(&args.hosts),
        "--profile".to_owned(),
        args.profile.clone(),
    ];
    if let Some(dir) = &args.socket_dir {
        words.push("--socket-dir".to_owned());
        words.push(absolute(dir));
    }
    if let Some(count) = args.idle_exit {
        words.push("--idle-exit".to_owned());
        words.push(count.to_string());
    }
    words
}

/// Start a runner as a process of its own and return at once, unless one is running already.
fn background(args: &DrainArgs) -> Result<i32, Error> {
    let (line, word) = if QueueFile::new(&args.doc).drain_running() {
        (Line::QueueDrainRunning, "running")
    } else {
        let exe = std::env::current_exe().map_err(|_| "this program cannot find itself")?;
        spawn_detached(&exe, &runner_arguments(args))
            .map_err(|_| "the queue runner could not be started")?;
        (Line::QueueDrainStarted, "started")
    };
    if args.json {
        let data = serde_json::Value::from(word);
        println!(
            "{}",
            json_with(&Report::new("queue-drain"), "runner", data)?
        );
    } else {
        println!("{}", Text::english().render(&line));
    }
    Ok(0)
}

/// Run the queue in this process until it has been idle for a while, over the compute host the
/// hosts file names. The key goes to the host connection and is never printed.
fn foreground(args: &DrainArgs) -> Result<i32, Error> {
    let profile = HostProfile::load(&args.hosts, &args.profile).map_err(|_| HOST_SETUP)?;
    let mut config = DrainConfig::new(&args.doc, &args.profile);
    config.np_cap = profile.core_cap;
    config.min_free_gb = profile.min_free_gb;
    config.clean_run_dirs = profile.clean_run_dirs;
    config.idle_exit_after = args.idle_exit.unwrap_or(IDLE_EXIT_AFTER);
    let base = match &args.socket_dir {
        Some(dir) => dir.clone(),
        None => default_socket_base()?,
    };
    config.socket_dir = Some(base.clone());
    let scratch = store_dir(&args.doc).join("live").join("_upload");
    std::fs::create_dir_all(&scratch).map_err(|_| HOST_SETUP)?;
    let host =
        SshHost::connect(profile, &args.key, base, scratch, IMAGE).map_err(|_| HOST_SETUP)?;
    // Nothing in this process sets the flag: the runner ends when its queue has been idle for a
    // while, or when its parent ends it.
    let shutdown = AtomicBool::new(false);
    ended(&run_drain(config, host, &shutdown), args.json)
}

/// Say how the runner ended and return the exit status for it.
fn ended(end: &DrainEnd, json: bool) -> Result<i32, Error> {
    let mut report = Report::new("queue-drain");
    let (word, sentence) = match end {
        DrainEnd::Idle => (
            "idle",
            "The queue runner finished: there was nothing left to do.".to_owned(),
        ),
        DrainEnd::Stopped => ("stopped", "The queue runner was stopped.".to_owned()),
        DrainEnd::AlreadyRunning => ("running", Text::english().render(&Line::QueueDrainRunning)),
        DrainEnd::Failed { code } => {
            report.set_class(ExitClass::Infrastructure);
            (
                "failed",
                format!("The queue runner gave up ({code}); the queue is kept as it was."),
            )
        }
    };
    if json {
        let data = serde_json::Value::from(word);
        println!("{}", json_with(&report, "runner", data)?);
    } else if report.class() == ExitClass::Passed {
        println!("{sentence}");
    } else {
        eprintln!("{sentence}");
    }
    Ok(i32::from(report.exit_class))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_default_socket_folder_leaves_room_for_the_longest_socket_name() {
        let base = socket_base_path().as_os_str().len();
        assert!(base < 60, "{base}");
        assert!(base + LONGEST_SOCKET_SUFFIX < 100, "{base}");
    }

    #[test]
    fn a_row_has_the_id_the_state_and_a_dash_for_the_study_itself() {
        let job = JobView {
            id: "q-001".to_owned(),
            scenario: None,
            kind: "solve".to_owned(),
            state: "queued".to_owned(),
            origin: None,
            run: None,
            code: None,
            progress_line: None,
        };
        let words: Vec<String> = row(&job).split_whitespace().map(str::to_owned).collect();
        assert_eq!(words, ["q-001", "queued", "-"]);
    }
}
