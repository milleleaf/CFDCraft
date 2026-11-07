//! The view of a run that the benchmark comparers read, over monitoring histories of an analytic
//! field at the points the writer placed.

mod common;

use cfdcraft_backend_codesaturne::{
    markers::{PROBES_PRESSURE_FILE, PROBES_VELOCITY_FILE, READ_CAP_BYTES},
    probes::{CENTRELINE_HORIZONTAL, CENTRELINE_VERTICAL, ProbePoint, ProbeSet, probe_set},
};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{MonitorField, ResolvedLine, ResolvedProbe, ResolvedStudy};
use common::{cavity, coarse};
use std::collections::{BTreeMap, BTreeSet};

/// Result files held in memory, by name.
#[derive(Clone, Default)]
struct MapResults(BTreeMap<String, String>);

impl ResultFiles for MapResults {
    fn read(&self, rel: &str) -> Option<String> {
        self.0.get(rel).cloned()
    }

    fn list(&self, rel_dir: &str) -> Vec<String> {
        let prefix = if rel_dir.is_empty() {
            String::new()
        } else {
            format!("{rel_dir}/")
        };
        let names: BTreeSet<String> = self
            .0
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter_map(|rest| rest.split('/').next())
            .map(str::to_owned)
            .collect();
        names.into_iter().collect()
    }
}

const TIMES: [f64; 3] = [1.0, 2.0, 3.0];
const LAST: f64 = 3.0;

fn ux(p: &ProbePoint, t: f64) -> f64 {
    2.0 * p.x + 3.0 * p.y + 0.1 * t
}

fn uy(p: &ProbePoint, t: f64) -> f64 {
    p.x - p.y + 0.01 * t
}

fn pressure(p: &ProbePoint, t: f64) -> f64 {
    p.x * p.y + t
}

fn velocity_csv(points: &[ProbePoint]) -> String {
    let mut text = String::from("Time,velocity columns\n");
    for t in TIMES {
        text.push_str(&format!("{t}"));
        for p in points {
            text.push_str(&format!(",{},{},{}", ux(p, t), uy(p, t), 0.5));
        }
        text.push('\n');
    }
    text
}

fn pressure_csv(points: &[ProbePoint]) -> String {
    let mut text = String::from("Time,pressure columns\n");
    for t in TIMES {
        text.push_str(&format!("{t}"));
        for p in points {
            text.push_str(&format!(",{}", pressure(p, t)));
        }
        text.push('\n');
    }
    text
}

/// The study with a diagonal line, a pressure point and a velocity point added to the monitors, so
/// the monitor plan names `line_1`, `probe_1` (pressure) and `probe_2` (velocity).
fn monitored_cavity() -> ResolvedStudy {
    let mut study = cavity(4);
    study.monitors.lines.push(ResolvedLine {
        label: "diagonal".to_owned(),
        x0: 0.0,
        y0: 0.0,
        x1: 0.1,
        y1: 0.1,
        samples: 5,
        field: MonitorField::Velocity,
    });
    for (label, x, field) in [
        ("front", 0.025, MonitorField::Pressure),
        ("mid", 0.05, MonitorField::Velocity),
    ] {
        study.monitors.probes.push(ResolvedProbe {
            label: label.to_owned(),
            x,
            y: 0.05,
            field,
            boundary: None,
        });
    }
    study
}

fn native(set: &ProbeSet) -> MapResults {
    let mut files = MapResults::default();
    files
        .0
        .insert(PROBES_VELOCITY_FILE.to_owned(), velocity_csv(&set.points));
    files
        .0
        .insert(PROBES_PRESSURE_FILE.to_owned(), pressure_csv(&set.points));
    files
}

fn view_of(study: &ResolvedStudy, files: MapResults) -> Box<dyn ResultFiles> {
    common::backend().results(Box::new(files), study)
}

/// The numbers of every data row of a sample file; a comment line and the brackets of a vector are
/// skipped.
fn numbers(text: &str) -> Vec<Vec<f64>> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| {
            l.replace(['(', ')'], " ")
                .split_whitespace()
                .map(|w| w.parse::<f64>().unwrap())
                .collect()
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9
}

fn assert_row(row: &[f64], expected: &[f64]) {
    assert_eq!(row.len(), expected.len(), "{row:?}");
    for (a, b) in row.iter().zip(expected) {
        assert!(close(*a, *b), "{row:?} against {expected:?}");
    }
}

fn point_of(set: &ProbeSet, run: &str) -> ProbePoint {
    set.points_of(run).unwrap()[0]
}

#[test]
fn the_probe_history_reaches_the_comparers() {
    let study = monitored_cavity();
    let set = probe_set(&study).unwrap();
    let view = view_of(&study, native(&set));
    let end = format!("{}", study.solver.end_time);

    // The two centre lines, as coordinate then the three velocity components, from corner to
    // corner.
    for (run, file, along_y, length) in [
        (
            CENTRELINE_VERTICAL,
            "vertical_U.xy",
            true,
            study.domain.length_y,
        ),
        (
            CENTRELINE_HORIZONTAL,
            "horizontal_U.xy",
            false,
            study.domain.length_x,
        ),
    ] {
        let text = view
            .read(&format!("postProcessing/centreline/{end}/{file}"))
            .unwrap();
        let rows = numbers(&text);
        let points = set.points_of(run).unwrap();
        assert_eq!(rows.len(), points.len());
        for (row, p) in rows.iter().zip(points) {
            let d = if along_y { p.y } else { p.x };
            assert_row(row, &[d, ux(p, LAST), uy(p, LAST), 0.5]);
        }
        assert!(close(rows[0][0], 0.0));
        assert!(close(rows[rows.len() - 1][0], length));
    }
    assert_eq!(
        view.list("postProcessing/centreline"),
        vec![end.clone()],
        "one folder at the end time"
    );
    assert_eq!(
        view.list(&format!("postProcessing/centreline/{end}")),
        vec!["horizontal_U.xy".to_owned(), "vertical_U.xy".to_owned()]
    );

    // A study line is sampled by its distance from the start of the line.
    let rows = numbers(
        &view
            .read(&format!("postProcessing/line_1/{end}/line_U.xy"))
            .unwrap(),
    );
    let points = set.points_of("line_1").unwrap();
    assert_eq!(rows.len(), 5);
    for (row, p) in rows.iter().zip(points) {
        assert_row(row, &[p.x.hypot(p.y), ux(p, LAST), uy(p, LAST), 0.5]);
    }

    // A pressure point gives a probe file of pressures, a velocity point one of vectors.
    let text = view.read("postProcessing/probe_1/0/p").unwrap();
    assert!(text.starts_with("# Probe 0 ("), "{text}");
    let p1 = point_of(&set, "probe_1");
    for (row, t) in numbers(&text).iter().zip(TIMES) {
        assert_row(row, &[t, pressure(&p1, t)]);
    }
    let text = view.read("postProcessing/probe_2/0/U").unwrap();
    let p2 = point_of(&set, "probe_2");
    let rows = numbers(&text);
    assert_eq!(rows.len(), TIMES.len());
    for (row, t) in rows.iter().zip(TIMES) {
        assert_row(row, &[t, ux(&p2, t), uy(&p2, t), 0.5]);
    }

    // Quantities the native files cannot give have no file.
    assert!(
        view.read("postProcessing/cylinder_forces/0/coefficient.dat")
            .is_none()
    );
    assert!(
        view.read("postProcessing/surface_1/0/surfaceFieldValue.dat")
            .is_none()
    );
}

#[test]
fn a_probe_point_missing_from_the_native_files_gives_none() {
    let study = monitored_cavity();
    let set = probe_set(&study).unwrap();
    let mut files = native(&set);
    // The velocity history lacks the last point, which is `probe_2`.
    let short = &set.points[..set.points.len() - 1];
    files
        .0
        .insert(PROBES_VELOCITY_FILE.to_owned(), velocity_csv(short));
    let view = view_of(&study, files);
    let end = format!("{}", study.solver.end_time);
    assert!(view.read("postProcessing/probe_2/0/U").is_none());
    assert!(
        view.read(&format!("postProcessing/line_1/{end}/line_U.xy"))
            .is_some()
    );
    assert!(view.read("postProcessing/probe_1/0/p").is_some());
    let shown = view.list("postProcessing");
    assert!(!shown.contains(&"probe_2".to_owned()), "{shown:?}");
    assert!(shown.contains(&"probe_1".to_owned()), "{shown:?}");
}

#[test]
fn the_step_lines_are_served_under_the_names_the_study_gives_them() {
    let study = coarse("backward-step");
    let set = probe_set(&study).unwrap();
    let view = view_of(&study, native(&set));
    let end = format!("{}", study.solver.end_time);
    let plan = study.monitor_plan();
    assert!(!plan.lines.is_empty());
    for line in &plan.lines {
        let text = view
            .read(&format!("postProcessing/{}/{end}/line_U.xy", line.fo))
            .unwrap_or_else(|| panic!("no file for {}", line.fo));
        let rows = numbers(&text);
        let points = set.points_of(&line.fo).unwrap();
        assert_eq!(rows.len(), points.len());
        for (row, p) in rows.iter().zip(points) {
            let d = (p.x - line.x0).hypot(p.y - line.y0);
            assert_row(row, &[d, ux(p, LAST), uy(p, LAST), 0.5]);
        }
    }
    // A shaped study has no centre line.
    assert!(
        !view
            .list("postProcessing")
            .contains(&"centreline".to_owned())
    );
}

#[test]
fn files_that_are_not_part_of_the_view_are_read_through() {
    let study = cavity(4);
    let set = probe_set(&study).unwrap();
    let mut files = native(&set);
    files
        .0
        .insert("RESU/run/summary".to_owned(), "native text".to_owned());
    let view = view_of(&study, files);
    assert_eq!(
        view.read("RESU/run/summary").as_deref(),
        Some("native text")
    );
    assert!(view.list("RESU/run").contains(&"summary".to_owned()));
}

#[test]
fn a_native_file_past_the_read_cap_gives_no_view() {
    let study = cavity(4);
    let set = probe_set(&study).unwrap();
    let end = format!("{}", study.solver.end_time);
    let path = format!("postProcessing/centreline/{end}/vertical_U.xy");

    let whole = view_of(&study, native(&set));
    assert!(whole.read(&path).is_some());
    assert!(
        whole
            .list("postProcessing")
            .contains(&"centreline".to_owned())
    );

    let mut files = native(&set);
    files.0.insert(
        PROBES_VELOCITY_FILE.to_owned(),
        "x".repeat(READ_CAP_BYTES as usize + 1),
    );
    let cut = view_of(&study, files);
    for rel in [
        path.as_str(),
        "postProcessing/probe_1/0/U",
        "postProcessing/probe_5/0/U",
    ] {
        assert!(cut.read(rel).is_none(), "{rel}");
    }
    let shown = cut.list("postProcessing");
    assert!(shown.is_empty(), "{shown:?}");
    assert!(
        cut.list(&format!("postProcessing/centreline/{end}"))
            .is_empty()
    );
}
