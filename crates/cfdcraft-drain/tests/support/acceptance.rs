//! What the acceptance tests share: a document with a sweep, a drain over the in-memory host, and
//! helpers that start a job, let the solver finish it and read the host's call log. The tests use
//! the real operations, the real drain and a saved document; only the compute host is a double.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use cfdcraft_assess::Spec;
use cfdcraft_backend_esi::Dialect;
use cfdcraft_capture::fixture::{FixtureSpec, write_results};
use cfdcraft_drain::Drain;
use cfdcraft_drain::finalize::JobReport;
use cfdcraft_drain::watch::{LiveSpec, RowView};
use cfdcraft_ops::convergence::spec_for;
use cfdcraft_ops::queue_ops::{AddRequest, queue_add, queue_list};
use cfdcraft_ops::sweep_ops::sweep_create;
use cfdcraft_ops::{QueueData, Repro, StatusData, document_status, prepare};
use cfdcraft_queue::{JobState, Queue};
use cfdcraft_remote::ContainerState;
use cfdcraft_settle::Status;
use cfdcraft_sweep::{Parameter, Sweep, Target, Values};
use cfdcraft_text::{Line, Text};
use serde_json::{Value, json};

use crate::support::*;

/// The setting the sweeps turn.
pub const LID: &str = "/boundary_conditions/lid/velocity/value";

/// The id of the sweep `sweep_world` makes.
pub const SWEEP: &str = "lids";

/// A world whose document holds a sweep named [`SWEEP`] over the lid speed, one point per value.
/// The points are called `lids-001`, `lids-002` and so on.
pub fn sweep_world(tag: &str, speeds: &[f64]) -> World {
    let w = world(tag);
    let sweep = Sweep {
        id: SWEEP.to_owned(),
        base: "base".to_owned(),
        parameters: vec![Parameter {
            name: "lid_speed".to_owned(),
            target: Target::Number {
                setting: LID.to_owned(),
            },
            values: Values::List {
                items: speeds.to_vec(),
            },
        }],
    };
    passed(&sweep_create(&w.doc, &sweep).report);
    w
}

/// Puts every point of the sweep in the queue and returns the ids of all the jobs in it.
pub fn queue_sweep(w: &World) -> Vec<String> {
    let request = AddRequest {
        sweep: Some(SWEEP.to_owned()),
        ..AddRequest::default()
    };
    passed(&queue_add(&w.doc, &request).report);
    w.queue().jobs.iter().map(|job| job.id.clone()).collect()
}

/// The state word of every job, in queue order.
pub fn states(w: &World) -> Vec<String> {
    w.queue()
        .jobs
        .iter()
        .map(|job| job.state.as_str().to_owned())
        .collect()
}

/// How many jobs hold the compute host now.
pub fn active_count(queue: &Queue) -> usize {
    queue
        .jobs
        .iter()
        .filter(|job| {
            matches!(
                job.state,
                JobState::Starting
                    | JobState::Running
                    | JobState::Stopping
                    | JobState::Finishing
                    | JobState::Unreachable
            )
        })
        .count()
}

/// Ticks until job `id` has ended, at most `limit` times, and returns its state. After every tick
/// the queue is checked: never two jobs hold the host.
pub fn settle(w: &World, drain: &mut Drain<Shared>, id: &str, limit: usize) -> JobState {
    for _ in 0..limit {
        if w.state(id).is_terminal() {
            break;
        }
        drain.tick().unwrap();
        assert!(
            active_count(&w.queue()) <= 1,
            "two jobs are active at once: {:?}",
            states(w)
        );
    }
    w.state(id)
}

/// Ticks until job `id` is running and returns the name of its run.
pub fn start_job(w: &World, drain: &mut Drain<Shared>, id: &str) -> String {
    if w.state(id) == JobState::Running {
        return w.run_id(id);
    }
    for _ in 0..8 {
        drain.tick().unwrap();
        assert!(
            active_count(&w.queue()) <= 1,
            "two jobs are active at once: {:?}",
            states(w)
        );
        if w.state(id) == JobState::Running {
            return w.run_id(id);
        }
    }
    panic!("the job {id} did not start: {:?}", w.state(id));
}

/// The solver goes on to its end: its log gets the end of a good run after whatever it has
/// written so far, the files a good run leaves are put into the run, and the container exits.
pub fn play_on(w: &World, run_id: &str) {
    let mut whole = w.shared.file(run_id, "log.simpleFoam").unwrap_or_default();
    w.play(run_id);
    let ending = w
        .shared
        .file(run_id, "log.simpleFoam")
        .expect("the played run has a log");
    whole.extend_from_slice(&ending);
    let text = std::str::from_utf8(&whole).expect("a log is text");
    w.shared.replace(run_id, "log.simpleFoam", text);
}

/// Starts job `id` if it has not started, lets the solver finish, and ticks until the job has
/// ended. Returns its state.
pub fn finish_job(w: &World, drain: &mut Drain<Shared>, id: &str) -> JobState {
    let run_id = start_job(w, drain, id);
    play_on(w, &run_id);
    settle(w, drain, id, 8)
}

/// What a solver that is cut short at `time` has left in the run, on a mesh of `cells`: the mesh,
/// the logs, and the fields of that time. The solver log does not end yet; a host that follows a
/// stop adds its end.
pub fn put_partial(w: &World, run_id: &str, time: u64, cells: u64) {
    let dir = w.dir.join("partial");
    let _ = fs::remove_dir_all(&dir);
    let spec = FixtureSpec {
        end_time: time,
        cells,
        samples: 64,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    for rel in files_under(&dir) {
        let mut text = fs::read_to_string(dir.join(&rel)).unwrap();
        if rel == "log.simpleFoam" {
            text = text
                .trim_end()
                .strip_suffix("End")
                .expect("the fixture log ends")
                .to_owned();
        }
        w.shared.replace(run_id, &rel, &text);
    }
}

/// The length of a file of the live view of a job, or 0 when it is not there.
pub fn mirror_len(w: &World, id: &str, rel: &str) -> u64 {
    fs::metadata(w.live(id).join(rel)).map_or(0, |m| m.len())
}

/// The calls the host has seen after the first `seen`.
pub fn calls_since(w: &World, seen: usize) -> Vec<String> {
    w.shared.calls().split_off(seen)
}

/// The calls that started a container, in order.
pub fn start_calls(w: &World) -> Vec<String> {
    w.shared
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("start "))
        .collect()
}

/// The offset of the first fetch of `rel` among `calls`.
pub fn first_tail_offset(calls: &[String], run_id: &str, rel: &str) -> u64 {
    let prefix = format!("tail {run_id} {rel} ");
    calls
        .iter()
        .find_map(|call| call.strip_prefix(&prefix))
        .expect("the file was fetched")
        .parse()
        .expect("an offset is a number")
}

/// The queue as the operation that lists it shows it.
pub fn listed(w: &World) -> QueueData {
    let out = queue_list(&w.doc);
    passed(&out.report);
    match out.report.repro {
        Some(Repro::Queue(data)) => *data,
        other => panic!("expected the queue, found {other:?}"),
    }
}

/// The channel study, which has an inlet and an outlet.
pub const CHANNEL: &str = include_str!("../../../cfdcraft-model/tests/fixtures/channel_v1.json");

// ---- monitors ----------------------------------------------------------------------------------

/// How a monitored value moves over `n` iterations.
#[derive(Debug, Clone, Copy)]
pub enum Shape {
    /// Approaches `base` and stays there.
    Settled,
    /// Climbs by one hundredth of `base` at every iteration.
    Ramp,
    /// Climbs by a few thousandths of its value across the last fifth of 300 iterations: a drift
    /// the default tolerance calls drifting and a looser one calls settled.
    SlowRamp,
}

fn at(shape: Shape, base: f64, t: u64) -> f64 {
    let t = t as f64;
    match shape {
        Shape::Settled => base + 1e-5 / t,
        Shape::Ramp => base * (1.0 + 0.01 * t),
        Shape::SlowRamp => base * (1.0 + 2e-5 * t),
    }
}

/// The file of one velocity probe after `n` iterations; Ux moves by `shape` from `base`, Uy from
/// `-base`. Numbers are written with twelve decimals, so the text is the same on every system.
pub fn probe_text(n: u64, shape: Shape, base: f64) -> String {
    let mut text = "# Probe 0 (0.05 0.05 0.025)\n#       Probe 0\n#          Time\n".to_owned();
    for t in 1..=n {
        let ux = at(shape, base, t);
        let uy = at(shape, -base, t);
        let _ = writeln!(text, "{t} ({ux:.12} {uy:.12} 0)");
    }
    text
}

/// A surface file with one column; `values` go with the times 1, 2, 3 and so on.
pub fn surface_rows(column: &str, values: &[f64]) -> String {
    let mut text = format!(
        "# Region type : patch lid\n# Faces       : 20\n# Area        : 0.100000000000\n# Scale factor : 1\n#\n# Time\t{column}\n"
    );
    for (i, value) in values.iter().enumerate() {
        let _ = writeln!(text, "{:.12}\t{value:.12}", (i + 1) as f64);
    }
    text
}

/// A surface file of `n` iterations of one quantity that moves by `shape` from `base`.
pub fn surface_text(column: &str, n: u64, shape: Shape, base: f64) -> String {
    let values: Vec<f64> = (1..=n).map(|t| at(shape, base, t)).collect();
    surface_rows(column, &values)
}

/// A sampled velocity line of 16 points whose component means are `ux` and `uy`.
pub fn line_text(ux: f64, uy: f64) -> String {
    let mut text = String::new();
    for i in 0..16 {
        let x = f64::from(i) * 0.01;
        let _ = writeln!(
            text,
            "{x:.12} 0.000000000000 0.000000000000 {ux:.12} {uy:.12} 0.000000000000"
        );
    }
    text
}

/// The first `folders` write times of the line `line_1`, each with the file it wrote. The means
/// settle fast.
pub fn line_files(folders: u64) -> Vec<(String, String)> {
    (1..=folders)
        .map(|i| {
            let k = (-(i as f64)).exp();
            (
                format!("postProcessing/line_1/{}/line_U.xy", 100 * i),
                line_text(0.3 * (1.0 + k), -0.1 * (1.0 + k)),
            )
        })
        .collect()
}

/// The cavity with one probe, one line and one surface integral listed in its study.
pub fn listed_study() -> String {
    edit(CAVITY, |study| {
        study["monitors"]["probes"] = json!({"centre": {
            "x": {"value": 0.05, "unit": "m"},
            "y": {"value": 0.05, "unit": "m"},
            "field": "velocity"
        }});
        study["monitors"]["lines"] = json!({"mid": {
            "x0": {"value": 0.0, "unit": "m"},
            "y0": {"value": 0.05, "unit": "m"},
            "x1": {"value": 0.1, "unit": "m"},
            "y1": {"value": 0.05, "unit": "m"},
            "samples": 16,
            "field": "velocity"
        }});
        study["monitors"]["surfaces"] = json!({"lidflux": {"boundary": "lid", "quantity": "flux"}});
    })
}

/// A world whose document is the cavity with its monitors listed.
pub fn monitored_world(tag: &str) -> World {
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", &listed_study());
    World {
        dir,
        doc,
        shared: Shared::new(),
    }
}

/// The files of the listed monitors after `n` iterations and `folders` write times: the probe
/// moves by `probe`, the surface integral by `surface`.
pub fn listed_files(n: u64, folders: u64, probe: Shape, surface: Shape) -> Vec<(String, String)> {
    let mut files = vec![
        (
            "postProcessing/probe_1/0/U".to_owned(),
            probe_text(n, probe, 0.03),
        ),
        (
            "postProcessing/surface_1/0/surfaceFieldValue.dat".to_owned(),
            surface_text("sum(phi)", n, surface, 0.5),
        ),
    ];
    files.extend(line_files(folders));
    files
}

/// Puts files into the run on the host, replacing any of the same name.
pub fn put_files(w: &World, run_id: &str, files: &[(String, String)]) {
    for (rel, text) in files {
        w.shared.replace(run_id, rel, text);
    }
}

/// The solver goes to its end but writes no monitor file: the log shows small residuals and
/// nothing else.
pub fn play_bare(w: &World, run_id: &str) {
    let dir = w.dir.join("played-bare");
    let _ = fs::remove_dir_all(&dir);
    let spec = FixtureSpec {
        end_time: 5000,
        cells: 16384,
        samples: 64,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    for rel in files_under(&dir) {
        let text = fs::read_to_string(dir.join(&rel)).unwrap();
        w.shared.replace(run_id, &rel, &text);
    }
    w.shared.set_container(
        run_id,
        ContainerState::Exited {
            code: 0,
            oom_killed: false,
        },
    );
}

/// What the live view of a job was judged by.
pub fn live_spec(w: &World, id: &str) -> LiveSpec {
    let text = fs::read_to_string(w.live(id).join("spec.json")).expect("the job has a spec");
    serde_json::from_str(&text).expect("the spec reads")
}

/// A row of the monitor table as a person reads it.
pub fn row_sentence(row: &RowView) -> String {
    let text = Text::english();
    let word = match row.state {
        Status::Settled => Line::MonitorSettled,
        Status::Drifting => Line::MonitorDrifting,
        Status::Oscillating => Line::MonitorOscillating,
        Status::TooEarly => Line::MonitorTooEarly,
        Status::TooFewSamples => Line::MonitorTooFewSamples,
        Status::Error => return "cannot be read".to_owned(),
    };
    let line = Line::MonitorLine {
        name: row.label.clone().unwrap_or_else(|| row.name.clone()),
        last: format!("{:.5e}", row.last),
        mean: format!("{:.5e}", row.mean),
        spread: format!("{:.5e}", row.spread),
        state: text.render(&word),
    };
    text.render(&line)
}

/// Whether the result stage and the mesh stage of the document are up to date, and why not.
pub fn status(doc: &Path) -> StatusData {
    let out = document_status(doc);
    match out.report.repro {
        Some(Repro::Status(data)) => *data,
        other => panic!("expected the status, found {other:?}"),
    }
}

// ---- a study with an inlet and an outlet -------------------------------------------------------

/// What the channel study monitors, made the way a run of it is judged.
pub fn channel_spec() -> Spec {
    let prepared = prepare(CHANNEL, Dialect::EsiV2512).expect("the channel passes the gates");
    spec_for(&prepared.resolved)
}

/// The results folder of a run of the channel in `dir`: a log with small residuals, five settled
/// probes, and a flux file for every patch. The inlet takes in `1e-3`; the outlet gives back
/// `outlet_share` of it, or has no file when `outlet_file` is false.
pub fn write_channel_run(dir: &Path, spec: &Spec, outlet_share: f64, outlet_file: bool) {
    let _ = fs::remove_dir_all(dir);
    let fixture = FixtureSpec {
        end_time: 60,
        cells: 2000,
        samples: 32,
        shift: 0.0,
    };
    write_results(dir, &fixture).unwrap();
    let put = |rel: String, text: String| {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    for probe in &spec.probes {
        let rel = format!("postProcessing/{}/0/U", probe.fo);
        put(rel, probe_text(300, Shape::Settled, 0.05));
    }
    for patch in &spec.patches {
        let flux = if patch.inlet {
            -1.0e-3
        } else if patch.patch == "outlet" {
            if !outlet_file {
                continue;
            }
            1.0e-3 * outlet_share
        } else {
            0.0
        };
        let rel = format!("postProcessing/{}/0/surfaceFieldValue.dat", patch.fo);
        put(rel, surface_rows("sum(phi)", &[flux, flux, flux]));
    }
}

// ---- runs that go wrong, and runs on more than one process --------------------------------------

/// A job added and started: the world, its drain, the job id and the run name.
pub fn started_job(tag: &str) -> (World, Drain<Shared>, String, String) {
    let w = world(tag);
    let id = w.add();
    let mut drain = w.drain();
    let run_id = start_job(&w, &mut drain, &id);
    (w, drain, id, run_id)
}

/// Iterations from `from` on, each with one Ux solve whose initial residual is the next value.
pub fn escalating(from: u64, residuals: &[f64]) -> String {
    let mut text = String::new();
    for (offset, r) in residuals.iter().enumerate() {
        let _ = writeln!(text, "Time = {}\n", from + offset as u64);
        let _ = writeln!(
            text,
            "smoothSolver:  Solving for Ux, Initial residual = {r:.6e}, Final residual = 1e-9, No Iterations 1\n"
        );
    }
    text
}

/// A started job whose solver log gets `log` and whose container then exits with a failure; the
/// drain takes the two ticks that finish it. Returns the world, the job id and the run name.
pub fn fail_with_log(tag: &str, log: &str, oom_killed: bool) -> (World, String, String) {
    let (w, mut drain, id, run_id) = started_job(tag);
    w.shared.append(&run_id, "log.simpleFoam", log);
    w.shared.set_container(
        &run_id,
        ContainerState::Exited {
            code: 1,
            oom_killed,
        },
    );
    drain.tick().unwrap();
    drain.tick().unwrap();
    (w, id, run_id)
}

/// The text of the report a job left in its live view.
pub fn report_text(w: &World, id: &str) -> String {
    fs::read_to_string(w.live(id).join("report.json")).expect("a report was written")
}

/// The report a job left in its live view.
pub fn report_of(w: &World, id: &str) -> JobReport {
    serde_json::from_str(&report_text(w, id)).expect("the report reads")
}

/// The control file the case was written with, as it is on the host.
pub fn control_of(w: &World, run_id: &str) -> String {
    text_of(w, run_id, "system/controlDict")
}

/// A text file of the run on the host.
pub fn text_of(w: &World, run_id: &str, rel: &str) -> String {
    let bytes = w
        .shared
        .file(run_id, rel)
        .unwrap_or_else(|| panic!("no {rel} on the host"));
    String::from_utf8(bytes).expect("a case file is text")
}

/// The cells of the large cavity: four processes' worth at 20 000 cells each, so a cap decides.
pub const BIG_CELLS: u64 = 90_000;

/// A world whose document is the cavity with 300 x 300 cells.
pub fn big_world(tag: &str) -> World {
    let study = edit(CAVITY, |s| {
        s["mesh"]["cells_x"] = Value::from(300);
        s["mesh"]["cells_y"] = Value::from(300);
    });
    let dir = scratch(tag);
    let doc = new_document(&dir, "cavity", &study);
    World {
        dir,
        doc,
        shared: Shared::new(),
    }
}

/// The solver goes to its end on a mesh of `cells`: the files a good run leaves are put into the
/// run and the container exits. A parallel run's log has one more line after `End`.
pub fn play_cells(w: &World, run_id: &str, cells: u64, trailer: bool) {
    let dir = w.dir.join("played-cells");
    let _ = fs::remove_dir_all(&dir);
    let spec = FixtureSpec {
        end_time: 5000,
        cells,
        samples: 64,
        shift: 0.0,
    };
    write_results(&dir, &spec).unwrap();
    write_monitor_files(&dir);
    for rel in files_under(&dir) {
        let mut text = fs::read_to_string(dir.join(&rel)).unwrap();
        if trailer && rel == "log.simpleFoam" {
            text.push_str("Finalising parallel run\n");
        }
        w.shared.replace(run_id, &rel, &text);
    }
    w.shared.set_container(
        run_id,
        ContainerState::Exited {
            code: 0,
            oom_killed: false,
        },
    );
}
