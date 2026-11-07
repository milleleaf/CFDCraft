//! The result view of the SU2 backend: native history and volume files, presented under the
//! paths and in the formats the benchmark comparers read. The synthetic fields are linear in x and
//! y, which the bilinear sampling reproduces exactly, so every number can be checked against the
//! formula. Nothing here is solver output.
//!
//! The comparers' own parsers sit in crates this one does not depend on, so each check below reads
//! the text the way the comparer does (whitespace separated numbers, the same column order and
//! the same conditions) and says what it expects.

mod common;

use cfdcraft_backend::Backend;
use cfdcraft_backend_su2::markers::READ_CAP_BYTES;
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{MonitorField, ResolvedStudy, SurfaceQuantity};
use common::{HISTORY, Mem, VOLUME, coarse, history_text, volume_text};

/// The backend behind its trait, so the trait is in scope for every call.
fn row() -> &'static dyn Backend {
    common::backend()
}

fn view(files: Mem, study: &ResolvedStudy) -> Box<dyn ResultFiles> {
    row().results(Box::new(files), study)
}

/// The time folder the view names: the end time of the study.
fn folder(study: &ResolvedStudy) -> String {
    study.solver.end_time.to_string()
}

fn numbers(text: &str) -> Vec<Vec<f64>> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            l.split_whitespace()
                .map(|w| w.parse::<f64>().expect("a number"))
                .collect()
        })
        .collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

/// The velocity field of the cavity and step tests.
fn flow(x: f64, y: f64) -> [f64; 3] {
    [1.0 + 2.0 * y + 0.5 * x, 3.0 * x - y, 300.0]
}

#[test]
fn the_cavity_centre_lines_reach_the_ghia_reader() {
    let study = coarse("lid-cavity");
    let (lx, ly) = (study.domain.length_x, study.domain.length_y);
    let n = study.monitors.centreline_u_sample_count as usize;
    let files = Mem::default().with(VOLUME, volume_text(&study, flow));
    let results = view(files, &study);
    let end = folder(&study);

    let root = "postProcessing/centreline";
    assert_eq!(results.list(root), vec![end.clone()]);
    assert_eq!(
        results.list(&format!("{root}/{end}")),
        vec!["horizontal_U.xy", "vertical_U.xy"]
    );

    // The reader: rows of a coordinate and three velocity components, the coordinate increasing
    // from one physical wall to the other.
    let read = |name: &str, length: f64, component: usize| {
        let text = results.read(&format!("{root}/{end}/{name}")).expect(name);
        let rows = numbers(&text);
        assert_eq!(rows.len(), n, "{name}");
        let coords: Vec<f64> = rows.iter().map(|r| r[0]).collect();
        assert!(rows.iter().all(|r| r.len() == 4));
        assert!(coords.windows(2).all(|w| w[0] < w[1]));
        assert!(coords[0].abs() <= 1e-8 && (coords[n - 1] / length - 1.0).abs() <= 1e-8);
        rows.iter()
            .map(|r| (r[0], r[component]))
            .collect::<Vec<_>>()
    };
    let mut wrong = Vec::new();
    for (at, (y, ux)) in read("vertical_U.xy", ly, 1).into_iter().enumerate() {
        let want = flow(lx / 2.0, y)[0];
        if !near(ux, want) {
            wrong.push(format!("vertical sample {at}: {ux} against {want}"));
        }
    }
    for (at, (x, uy)) in read("horizontal_U.xy", lx, 2).into_iter().enumerate() {
        let want = flow(x, ly / 2.0)[1];
        if !near(uy, want) {
            wrong.push(format!("horizontal sample {at}: {uy} against {want}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    // A native file the run did not leave gives nothing, not an empty text.
    let nothing = view(Mem::default(), &study);
    assert_eq!(nothing.read(&format!("{root}/{end}/vertical_U.xy")), None);
    assert!(nothing.list(root).is_empty());
    // Names that leave the folder are refused.
    assert_eq!(results.read("../vertical_U.xy"), None);
    assert_eq!(results.read("/etc/hostname"), None);
    assert!(results.list("..").is_empty());
}

#[test]
fn the_other_benchmark_paths_are_presented() {
    the_step_lines_are_presented_as_sampled_lines();
    the_cylinder_series_are_presented_from_history_columns();
    the_heated_cavity_surfaces_and_lines_are_presented();
}

fn the_step_lines_are_presented_as_sampled_lines() {
    let study = coarse("backward-step-laminar");
    let results = view(
        Mem::default().with(VOLUME, volume_text(&study, flow)),
        &study,
    );
    let end = folder(&study);
    assert!(!study.monitors.lines.is_empty());
    let mut wrong = Vec::new();
    for (at, line) in study.monitors.lines.iter().enumerate() {
        let path = format!("postProcessing/line_{}/{end}/line_U.xy", at + 1);
        let Some(text) = results.read(&path) else {
            wrong.push(format!("{path} is missing"));
            continue;
        };
        // The step comparer reads six numbers a row: x, y, z, then the velocity.
        let rows = numbers(&text);
        if rows.len() as u64 != line.samples || rows.iter().any(|r| r.len() != 6) {
            wrong.push(format!("{path}: {} rows", rows.len()));
            continue;
        }
        let first = &rows[0];
        let last = &rows[rows.len() - 1];
        if !(near(first[0], line.x0) && near(first[1], line.y0)) {
            wrong.push(format!("{path}: starts at {} {}", first[0], first[1]));
        }
        if !(near(last[0], line.x1) && near(last[1], line.y1)) {
            wrong.push(format!("{path}: ends at {} {}", last[0], last[1]));
        }
        for r in &rows {
            let want = flow(r[0], r[1]);
            if !(near(r[3], want[0]) && near(r[4], want[1]) && r[5] == 0.0) {
                wrong.push(format!(
                    "{path}: velocity at {} {} is {} {}",
                    r[0], r[1], r[3], r[4]
                ));
                break;
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

fn the_cylinder_series_are_presented_from_history_columns() {
    let study = coarse("cylinder");
    let plan = study.monitor_plan();
    let force = &plan.forces[0].fo;
    let probes: Vec<&String> = plan
        .probes
        .iter()
        .filter(|p| p.field == MonitorField::Pressure)
        .map(|p| &p.fo)
        .collect();
    assert_eq!(probes.len(), 2);
    let history = history_text(
        &[
            "Cur_Time",
            "CD",
            "CL",
            probes[0].as_str(),
            probes[1].as_str(),
        ],
        25,
        |i| {
            let t = i as f64 * 0.001;
            vec![t, 3.2 + t, 0.5 * t, 3.0 + t, 0.7]
        },
    );
    let results = view(Mem::default().with(HISTORY, history), &study);

    // The coefficient file: the lift direction along y, then time, drag and lift.
    let text = results
        .read(&format!("postProcessing/{force}/0/coefficient.dat"))
        .expect("the coefficient file");
    assert!(text.contains("# liftDir      : (0 1 0)"));
    assert!(text.contains("# dragDir      : (1 0 0)"));
    assert!(text.contains("# Time\tCd\tCl"));
    let rows = numbers(&text);
    assert_eq!(rows.len(), 25);
    for (i, r) in rows.iter().enumerate() {
        let t = i as f64 * 0.001;
        assert!(r.len() == 3 && near(r[0], t) && near(r[1], 3.2 + t) && near(r[2], 0.5 * t));
    }

    // The probe files: a header with the probe's place, then time and pressure.
    for (k, fo) in probes.iter().enumerate() {
        let text = results
            .read(&format!("postProcessing/{fo}/0/p"))
            .expect("the probe file");
        assert!(text.starts_with("# Probe 0 ("), "{text}");
        let rows = numbers(&text);
        assert_eq!(rows.len(), 25);
        let want = if k == 0 { 3.0 + 0.024 } else { 0.7 };
        assert!(
            rows.iter().all(|r| r.len() == 2) && near(rows[24][1], want),
            "{fo}"
        );
    }
    assert_eq!(results.list("postProcessing/force_1"), vec!["0"]);

    // Without the columns nothing is presented, and nothing is made up.
    let plain = history_text(&["Cur_Time", "rms[P]"], 5, |i| vec![i as f64, -5.0]);
    let bare = view(Mem::default().with(HISTORY, plain), &study);
    assert_eq!(
        bare.read(&format!("postProcessing/{force}/0/coefficient.dat")),
        None
    );
    assert_eq!(
        bare.read(&format!("postProcessing/{}/0/p", probes[0])),
        None
    );

    // Two force monitors share one drag column, so none is presented.
    let mut two = coarse("cylinder");
    let mut second = two.monitors.forces[0].clone();
    second.label = "other_forces".to_owned();
    two.monitors.forces.push(second);
    let history = history_text(&["Cur_Time", "CD", "CL"], 5, |i| vec![i as f64, 1.0, 1.0]);
    let shared = view(Mem::default().with(HISTORY, history), &two);
    assert!(shared.list("postProcessing").is_empty());
}

fn the_heated_cavity_surfaces_and_lines_are_presented() {
    let study = coarse("heated-cavity");
    let slope = 7.0;
    let results = view(
        Mem::default().with(
            VOLUME,
            volume_text(&study, |x, y| [x + y, x - y, 300.5 - slope * x]),
        ),
        &study,
    );
    let end = folder(&study);
    let plan = study.monitor_plan();
    let area = study.domain.length_y * study.domain.length_z;

    // The heat flux surfaces: the gradient of a temperature that falls by `slope` per metre along
    // x is -slope in x and zero in y, integrated over a wall of this area.
    let mut wrong = Vec::new();
    let surfaces: Vec<_> = plan
        .surfaces
        .iter()
        .filter(|s| s.quantity == SurfaceQuantity::HeatFlux)
        .collect();
    assert_eq!(surfaces.len(), 2);
    for surface in surfaces {
        let path = format!("postProcessing/{}/{end}/surfaceFieldValue.dat", surface.fo);
        let Some(text) = results.read(&path) else {
            wrong.push(format!("{path} is missing"));
            continue;
        };
        assert!(text.contains("# Time areaIntegrate(gradT)"), "{text}");
        assert!(text.contains(&format!("# Region type : patch {}", surface.boundary)));
        let last = text.lines().last().expect("a row");
        let values: Vec<f64> = last
            .replace(['(', ')'], " ")
            .split_whitespace()
            .skip(1)
            .map(|w| w.parse().expect("a number"))
            .collect();
        if !(values.len() == 3
            && near(values[0], -slope * area)
            && values[1].abs() < 1e-9
            && values[2] == 0.0)
        {
            wrong.push(format!("{path}: {values:?} against {}", -slope * area));
        }
        let header_area = text
            .lines()
            .find_map(|l| l.strip_prefix("# Area : "))
            .and_then(|v| v.parse::<f64>().ok());
        if !header_area.is_some_and(|a| near(a, area)) {
            wrong.push(format!("{path}: area {header_area:?} against {area}"));
        }
    }
    // The two mid-plane lines.
    for line in &plan.lines {
        let path = format!("postProcessing/{}/{end}/line_U.xy", line.fo);
        let text = results.read(&path).expect("a line file");
        let rows = numbers(&text);
        if rows.len() as u64 != line.samples || rows.iter().any(|r| r.len() != 6) {
            wrong.push(format!("{path}: {} rows", rows.len()));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    // A volume file without a temperature column gives lines but no surface.
    let cold: String = volume_text(&study, |x, y| [x + y, x - y, 0.0])
        .lines()
        .map(|l| l.rsplit_once(',').map_or(l, |(head, _)| head).to_owned() + "\n")
        .collect();
    let no_heat = view(Mem::default().with(VOLUME, cold), &study);
    assert!(
        no_heat
            .list(&format!("postProcessing/{}", plan.surfaces[0].fo))
            .is_empty()
    );
    assert!(
        !no_heat
            .list(&format!("postProcessing/{}", plan.lines[0].fo))
            .is_empty()
    );
}

#[test]
fn a_native_file_past_the_read_cap_gives_no_view() {
    let pad = " ".repeat(usize::try_from(READ_CAP_BYTES).expect("a size"));

    // A volume file over the limit gives none of the paths built from it.
    let study = coarse("lid-cavity");
    let volume = volume_text(&study, flow);
    let end = folder(&study);
    let path = format!("postProcessing/centreline/{end}/vertical_U.xy");
    let small = view(Mem::default().with(VOLUME, volume.clone()), &study);
    assert!(small.read(&path).is_some());
    let big = view(
        Mem::default().with(VOLUME, format!("{volume}{pad}")),
        &study,
    );
    assert_eq!(big.read(&path), None);
    assert!(big.list("postProcessing/centreline").is_empty());
    assert!(big.list("postProcessing").is_empty());

    // A history file over the limit gives no partial series.
    let cylinder = coarse("cylinder");
    let plan = cylinder.monitor_plan();
    let history = history_text(&["Cur_Time", "CD", "CL"], 30, |i| {
        vec![i as f64 * 0.001, 3.2, 0.1]
    });
    let coefficients = format!("postProcessing/{}/0/coefficient.dat", plan.forces[0].fo);
    let small = view(Mem::default().with(HISTORY, history.clone()), &cylinder);
    assert!(small.read(&coefficients).is_some());
    let big = view(
        Mem::default().with(HISTORY, format!("{history}{pad}")),
        &cylinder,
    );
    assert_eq!(big.read(&coefficients), None);
    assert!(big.list("postProcessing").is_empty());
    // The native name itself is not handed on either.
    assert_eq!(big.read(HISTORY), None);
}
