mod support;

use std::f64::consts::PI;
use std::fmt::Write as _;
use std::ops::Range;

use cfdcraft_assess::{Assessment, Files, ForceSpec, Spec, assess};
use cfdcraft_monitors::FieldKind;
use cfdcraft_settle::{Settings, Status, VerdictState};
use support::{Shape, cavity, line_folders, line_spec, probe_spec, put};

/// The time step of every series, in seconds.
const DT: f64 = 1e-3;

/// A 3 Hz sine shifted by 5 ms so that no sample sits on the series mean (the mean of a sine over
/// whole periods is about zero, and a sample on that level would make the cycle count depend on
/// rounding).
fn sine(t: f64) -> f64 {
    (2.0 * PI * 3.0 * (t + 0.005)).sin()
}

fn growing_sine(t: f64) -> f64 {
    (1.0 + 0.05 * t) * sine(t)
}

/// A force coefficient file for the steps `steps` of [`DT`].
fn coefficient_file(steps: Range<u32>, cd: impl Fn(f64) -> f64, cl: impl Fn(f64) -> f64) -> String {
    let mut text = String::from(
        "# Force coefficients\n# dragDir      : (1 0 0)\n# liftDir      : (0 1 0)\n# Time\tCd\tCl\n",
    );
    for k in steps {
        let t = f64::from(k) * DT;
        let _ = writeln!(text, "{t:.6}\t{:.12}\t{:.12}", cd(t), cl(t));
    }
    text
}

/// A pressure probes file for the steps `steps` of [`DT`].
fn pressure_probe_file(steps: Range<u32>, value: impl Fn(f64) -> f64) -> String {
    let mut text = String::from(
        "# Probe 0 (0.050000000000 0.050000000000 0.005000000000)\n#       Probe             0\n#        Time\n",
    );
    for k in steps {
        let t = f64::from(k) * DT;
        let _ = writeln!(text, "{t:.6}   {:.12}", value(t));
    }
    text
}

fn force_spec(periodic: bool) -> Spec {
    Spec {
        forces: vec![ForceSpec {
            fo: "force_1".to_owned(),
            label: "cylinder".to_owned(),
        }],
        periodic,
        ..Spec::default()
    }
}

fn files_of(fo: &str, text: String) -> Files {
    let mut files = Files::default();
    put(&mut files, fo, text);
    files
}

fn run(spec: &Spec, files: &Files) -> Assessment {
    assess(spec, files, &Settings::default())
}

fn status_of(a: &Assessment, name: &str) -> Status {
    a.quantities
        .iter()
        .find(|q| q.name == name)
        .unwrap_or_else(|| panic!("no quantity {name}"))
        .status
}

#[test]
fn force_coefficients_are_read_and_judged() {
    let flat = coefficient_file(0..200, |_| 1.2, |_| 0.3);
    let a = run(&force_spec(false), &files_of("force_1", flat));
    assert_eq!(status_of(&a, "force_1.Cd"), Status::Settled);
    assert_eq!(status_of(&a, "force_1.Cl"), Status::Settled);
    assert_eq!(a.verdict.state, VerdictState::Converged);
    assert_eq!(
        a.labels.get("force_1").map(String::as_str),
        Some("cylinder")
    );

    let a = run(&force_spec(false), &Files::default());
    assert_eq!(a.notes, ["missing:force_1"]);
    assert_eq!(a.quantities.len(), 2);
    assert!(a.quantities.iter().all(|q| q.status == Status::TooEarly));

    let a = run(
        &force_spec(false),
        &files_of("force_1", "not a coefficient file".to_owned()),
    );
    assert_eq!(a.notes, ["parse:force_1"]);
    assert_eq!(a.quantities.len(), 1);
    assert_eq!(a.quantities[0].status, Status::Error);

    // A file that parses but holds no lift column leaves the lift an error, never a settled value.
    let only_drag = "# Time\tCd\n1.0\t1.2\n2.0\t1.2\n".to_owned();
    let a = run(&force_spec(false), &files_of("force_1", only_drag));
    assert_eq!(status_of(&a, "force_1.Cl"), Status::Error);
    assert_ne!(a.verdict.state, VerdictState::Converged);
}

#[test]
fn a_periodic_lift_is_settled_in_a_transient_spec() {
    let text = coefficient_file(0..8000, |_| 1.2, sine);
    let files = files_of("force_1", text);

    let a = run(&force_spec(true), &files);
    assert_eq!(status_of(&a, "force_1.Cl"), Status::Settled);
    assert_eq!(status_of(&a, "force_1.Cd"), Status::Settled);
    assert_eq!(
        a.verdict.state,
        VerdictState::Converged,
        "{:?}",
        a.verdict.reasons
    );

    // Without the flag the steady tail rules see only an oscillation.
    let a = run(&force_spec(false), &files);
    assert_ne!(status_of(&a, "force_1.Cl"), Status::Settled);
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
}

#[test]
fn a_growing_lift_is_drifting_and_blocks_the_verdict() {
    let text = coefficient_file(0..8000, |_| 1.2, growing_sine);
    let a = run(&force_spec(true), &files_of("force_1", text));
    assert_eq!(status_of(&a, "force_1.Cl"), Status::Drifting);
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
    assert_eq!(a.verdict.reasons, ["not_settled:force_1.Cl"]);
}

#[test]
fn too_few_cycles_is_too_early() {
    let text = coefficient_file(0..2000, |_| 1.2, sine);
    let a = run(&force_spec(true), &files_of("force_1", text));
    assert_eq!(status_of(&a, "force_1.Cl"), Status::TooEarly);
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
    assert!(
        a.verdict
            .reasons
            .contains(&"not_settled:force_1.Cl".to_owned())
    );
}

#[test]
fn pressure_probes_use_the_periodic_path_too() {
    let spec = |periodic: bool| Spec {
        probes: vec![probe_spec("probe_1", FieldKind::Pressure)],
        periodic,
        ..Spec::default()
    };
    let files = files_of("probe_1", pressure_probe_file(0..8000, sine));
    assert_eq!(
        status_of(&run(&spec(true), &files), "probe_1.p"),
        Status::Settled
    );
    assert_ne!(
        status_of(&run(&spec(false), &files), "probe_1.p"),
        Status::Settled
    );

    // A line monitor is judged on its write-time samples whatever the flag says.
    let line = |periodic: bool| Spec {
        lines: vec![line_spec("line_1")],
        periodic,
        ..Spec::default()
    };
    let mut files = Files::default();
    files
        .monitor_files
        .insert("line_1".to_owned(), line_folders(20));
    let steady = run(&line(false), &files);
    let flagged = run(&line(true), &files);
    assert_eq!(steady.quantities, flagged.quantities);
    assert!(
        flagged
            .quantities
            .iter()
            .all(|q| q.status == Status::Settled)
    );
}

#[test]
fn restarted_coefficient_files_join() {
    // The first run is wrong from step 4000 on; the restart rewrites those steps with the real
    // values, and the later value is the one kept.
    let first = coefficient_file(0..5001, |_| 1.2, |t| if t >= 4.0 { 99.0 } else { sine(t) });
    let second = coefficient_file(4000..8000, |_| 1.2, sine);
    let mut files = Files::default();
    files.monitor_files.insert(
        "force_1".to_owned(),
        vec![("0".to_owned(), first), ("4".to_owned(), second)],
    );
    let a = run(&force_spec(true), &files);
    assert_eq!(status_of(&a, "force_1.Cl"), Status::Settled);
    let cl = a.quantities.iter().find(|q| q.name == "force_1.Cl");
    assert_eq!(cl.map(|q| q.samples), Some(8000));
    assert!(a.notes.is_empty(), "{:?}", a.notes);
}

#[test]
fn a_spec_without_forces_or_periodic_is_unchanged() {
    // Everything steady and settled is judged the same whether or not the periodic path is on.
    let (mut spec, files) = cavity(Shape::Settles);
    let before = run(&spec, &files);
    spec.periodic = true;
    assert_eq!(run(&spec, &files), before);
    assert_eq!(before.verdict.state, VerdictState::Converged);

    // A ramp has no cycle, so it keeps its steady verdict on the periodic path.
    let (mut spec, files) = cavity(Shape::Ramps);
    spec.periodic = true;
    let a = run(&spec, &files);
    assert_eq!(status_of(&a, "probe_3.Ux"), Status::Drifting);
    assert_eq!(a.verdict.state, VerdictState::NotConverged);
}
