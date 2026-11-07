//! The probe answers of a lattice run reach the benchmark comparers under the paths and in the
//! formats they read, with the unit conversions done by the backend. A quantity the replies do not
//! hold has no file, and a reply file past the read limit gives no view at all. Every reply is
//! synthetic: no solver ran. The files are read with the parsers the comparers use.

#[path = "support/replies.rs"]
mod replies;

use cfdcraft_backend::Backend;
use cfdcraft_backend_lbm::{
    backends, commands::REFERENCE_DENSITY, markers::READ_CAP_BYTES, obstacles, thermal,
};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{Face, MonitorField, ResolvedStudy, Study, resolve};
use cfdcraft_monitors::{Value as Cell, parse_coefficients, parse_probes, parse_surface, parse_xy};
use cfdcraft_templates::{TemplateOptions, build};
use replies::{Mem, Stream};
use serde_json::json;

fn backend() -> &'static dyn Backend {
    backends()[0]
}

fn template(id: &str) -> ResolvedStudy {
    resolve(&build(id, &TemplateOptions::default()).unwrap()).unwrap()
}

/// The heated cavity without wall grading, which the lattice cannot take.
fn heated() -> ResolvedStudy {
    let study = build("heated-cavity", &TemplateOptions::default()).unwrap();
    let mut value = serde_json::to_value(&study).unwrap();
    value["mesh"].as_object_mut().unwrap().remove("grading");
    resolve(&Study::from_json(&value.to_string()).unwrap()).unwrap()
}

fn view(files: Mem, study: &ResolvedStudy) -> Box<dyn ResultFiles> {
    backend().results(Box::new(files), study)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

/// The field the synthetic lines are cut from.
fn field(x: f64, y: f64) -> [f64; 3] {
    [0.5 * y + 0.1 * x, -0.25 * x + 0.05 * y, 0.0]
}

/// Whether the file of a line holds the distance and the three components of [`field`] at `count`
/// evenly spaced points.
fn check_line(text: &str, from: [f64; 2], to: [f64; 2], count: usize) {
    let file = parse_xy(text).unwrap();
    assert_eq!(file.columns, 4, "{text}");
    assert_eq!(file.rows.len(), count, "{text}");
    let length = (to[0] - from[0]).hypot(to[1] - from[1]);
    for (i, row) in file.rows.iter().enumerate() {
        let t = i as f64 / (count - 1) as f64;
        let (x, y) = (
            from[0] + t * (to[0] - from[0]),
            from[1] + t * (to[1] - from[1]),
        );
        assert!(close(row[0], t * length), "distance {} at {i}", row[0]);
        for (k, want) in field(x, y).iter().enumerate() {
            assert!(
                close(row[1 + k], *want),
                "component {k} at {i}: {} {want}",
                row[1 + k]
            );
        }
    }
}

fn the_cavity_centrelines_reach_the_comparer_path() {
    let study = template("lid-cavity");
    let (lx, ly) = (study.domain.length_x, study.domain.length_y);
    let mut stream = Stream::good();
    stream.velocity_line([lx / 2.0, 0.0], [lx / 2.0, ly], 0.001, 5, field);
    stream.velocity_line([0.0, ly / 2.0], [lx, ly / 2.0], 0.001, 5, field);
    let results = view(stream.files(), &study);
    let end = study.solver.end_time.to_string();
    assert_eq!(
        results.list("postProcessing/centreline"),
        std::slice::from_ref(&end)
    );
    let read = |name: &str| {
        results
            .read(&format!("postProcessing/centreline/{end}/{name}"))
            .unwrap_or_else(|| panic!("{name}"))
    };
    check_line(&read("vertical_U.xy"), [lx / 2.0, 0.0], [lx / 2.0, ly], 5);
    check_line(&read("horizontal_U.xy"), [0.0, ly / 2.0], [lx, ly / 2.0], 5);
}

/// The lines every template study plans reach `postProcessing/line_<n>/<end>/line_U.xy`.
fn planned_lines_reach_their_files(study: &ResolvedStudy) {
    let plan = study.monitor_plan();
    let lines: Vec<_> = plan
        .lines
        .iter()
        .filter(|line| line.field == MonitorField::Velocity)
        .collect();
    assert!(!lines.is_empty());
    let mut stream = Stream::good();
    for line in &lines {
        stream.velocity_line([line.x0, line.y0], [line.x1, line.y1], 0.001, 7, field);
    }
    let results = view(stream.files(), study);
    let end = study.solver.end_time.to_string();
    for line in lines {
        let text = results
            .read(&format!("postProcessing/{}/{end}/line_U.xy", line.fo))
            .unwrap_or_else(|| panic!("{}", line.fo));
        check_line(&text, [line.x0, line.y0], [line.x1, line.y1], 7);
    }
}

fn the_step_and_heated_cavity_lines_reach_their_paths() {
    planned_lines_reach_their_files(&template("backward-step-laminar"));
    planned_lines_reach_their_files(&heated());
}

/// A point answer: a fluid cell holding pressure and velocity.
fn fluid(pressure: f64) -> serde_json::Value {
    json!({"cell": [10, 10, 1], "solid": false, "velocity": [0.1, 0.2, 0.0], "pressure": pressure})
}

fn the_cylinder_forces_and_pressures_reach_the_comparer_files() {
    let study = template("cylinder");
    let plan = study.monitor_plan();
    let lattice = obstacles::lattice(&study).unwrap();
    let probe = |label: &str| {
        plan.probes
            .iter()
            .find(|p| p.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("{label}"))
    };
    let (front, rear) = (probe("front"), probe("rear"));
    let z = lattice.slab() / 2.0;
    // The probes lie on the cylinder, so the solver is asked about the cells beside them.
    let near = 0.4 * lattice.spacing;
    let asked = [[front.x - near, front.y, z], [rear.x + near, rear.y, z]];
    let mut stream = Stream::good();
    // Frames in no order, and one in which the front point lies in the body.
    let frames = [
        (3.0, fluid(3.5)),
        (1.0, fluid(3.2)),
        (2.0, fluid(3.3)),
        (4.0, json!({"cell": [10, 10, 1], "solid": true})),
    ];
    for (time, front_answer) in frames {
        stream.probe(
            json!({"step": "Run", "points": asked}),
            &json!({"time": time, "points": [front_answer, fluid(0.7)]}),
        );
    }
    // The force on the body's two faces, and on the floor of the channel, which is no part of it.
    let force = &plan.forces[0];
    let body = lattice.obstacle.as_ref().unwrap().name;
    let scale = 0.5
        * REFERENCE_DENSITY
        * force.reference_velocity
        * force.reference_velocity
        * force.reference_length
        * lattice.slab();
    let (first, second) = (
        [[0.6, 0.1, 0.0], [0.7, -0.2, 0.0]],
        [[1.5, 0.3, 0.0], [1.4, 0.2, 0.0]],
    );
    stream.probe(
        json!({"step": "Run", "forces": [body]}),
        &json!({
            "times": [0.5, 1.0],
            "forces": {
                (format!("{body}/F1:side")): first,
                (format!("{body}/F2:side")): second,
                "y_min": [[99.0, 99.0, 0.0], [99.0, 99.0, 0.0]],
            },
        }),
    );
    let results = view(stream.files(), &study);

    let text = results
        .read(&format!("postProcessing/{}/0/p", front.fo))
        .expect("front pressure");
    let file = parse_probes(&text).unwrap();
    let times: Vec<f64> = file.rows.iter().map(|r| r.time).collect();
    assert_eq!(times, [1.0, 2.0, 3.0]);
    let pressures: Vec<f64> = file
        .rows
        .iter()
        .map(|r| match r.values[0] {
            Cell::Scalar(p) => p,
            Cell::Vector(_) => panic!("a vector"),
        })
        .collect();
    for (got, want) in pressures.iter().zip([3.2, 3.3, 3.5]) {
        assert!(close(*got, want / REFERENCE_DENSITY), "{got} {want}");
    }
    let rear_text = results
        .read(&format!("postProcessing/{}/0/p", rear.fo))
        .expect("rear pressure");
    assert_eq!(parse_probes(&rear_text).unwrap().rows.len(), 4);

    let text = results
        .read(&format!("postProcessing/{}/0/coefficient.dat", force.fo))
        .expect("coefficients");
    let file = parse_coefficients(&text).unwrap();
    assert_eq!(file.lift_dir, Some([0.0, 1.0, 0.0]));
    assert_eq!(file.drag_dir, Some([1.0, 0.0, 0.0]));
    assert_eq!(file.columns, ["Cd", "Cl"]);
    for (i, row) in file.rows.iter().enumerate() {
        let cd = (first[i][0] + second[i][0]) / scale;
        let cl = (first[i][1] + second[i][1]) / scale;
        match row.values[..] {
            [Cell::Scalar(got_cd), Cell::Scalar(got_cl)] => {
                assert!(close(got_cd, cd), "Cd {got_cd} {cd}");
                assert!(close(got_cl, cl), "Cl {got_cl} {cl}");
            }
            _ => panic!("{:?}", row.values),
        }
    }
    assert_eq!(file.rows.len(), 2);

    // The monitors' folders are listed, nothing else is made up.
    let listed = results.list("postProcessing");
    for fo in [&front.fo, &rear.fo, &force.fo] {
        assert!(listed.contains(fo), "{listed:?}");
    }
    assert!(
        results.read("log.fieldcraft").is_some(),
        "native files stay readable"
    );
    assert!(results.read("../log.fieldcraft").is_none());
    assert!(results.list("..").is_empty());
}

fn the_heat_surfaces_reach_the_comparer_files() {
    let study = heated();
    let plan = study.monitor_plan();
    let lattice = obstacles::lattice(&study).unwrap();
    let conductivity = thermal::material(&study, REFERENCE_DENSITY)
        .unwrap()
        .unwrap()["conductivity"]
        .as_f64()
        .unwrap();
    // The wanted gradient integrals along the wall's axis, per time, for each heat surface.
    let wanted = [(-0.02, -0.021), (-0.018, -0.019)];
    let times = [10.0, 20.0];
    let mut walls = serde_json::Map::new();
    let mut keys = Vec::new();
    let surfaces: Vec<_> = plan
        .surfaces
        .iter()
        .filter(|s| s.label.as_deref().is_some_and(|l| l.ends_with("_flux")))
        .collect();
    assert_eq!(surfaces.len(), 2);
    for (surface, (first, second)) in surfaces.iter().zip(wanted) {
        let face = study.boundary_conditions[&surface.boundary].faces[0];
        let (key, normal) = match face {
            Face::XMin => ("x_min", -1.0),
            Face::XMax => ("x_max", 1.0),
            other => panic!("{other:?}"),
        };
        // The power into a wall is minus the conductivity times the gradient along its outward normal.
        let power = |integral: f64| -integral * conductivity * normal;
        walls.insert(key.to_owned(), json!([power(first), power(second)]));
        keys.push(key);
    }
    let mut stream = Stream::good();
    stream.probe(
        json!({"step": "Run", "heat": keys}),
        &json!({"times": times, "heat": walls}),
    );
    let results = view(stream.files(), &study);
    let end = study.solver.end_time.to_string();
    for (surface, (first, second)) in surfaces.iter().zip(wanted) {
        let text = results
            .read(&format!(
                "postProcessing/{}/{end}/surfaceFieldValue.dat",
                surface.fo
            ))
            .unwrap_or_else(|| panic!("{}", surface.fo));
        let file = parse_surface(&text).unwrap();
        assert_eq!(file.region, format!("patch {}", surface.boundary));
        assert_eq!(file.columns, ["areaIntegrate(gradT)"]);
        assert!(
            close(file.area, study.domain.length_y * lattice.slab()),
            "{}",
            file.area
        );
        let cells = (study.domain.length_y / lattice.spacing).round() as u64;
        assert_eq!(file.faces, cells);
        let got: Vec<f64> = file
            .rows
            .iter()
            .map(|row| match row.values[0] {
                Cell::Vector(v) => v[0],
                Cell::Scalar(_) => panic!("a scalar"),
            })
            .collect();
        assert_eq!(file.rows.iter().map(|r| r.time).collect::<Vec<_>>(), times);
        assert!(
            close(got[0], first) && close(got[1], second),
            "{got:?} {first} {second}"
        );
    }
}

/// The four comparer families: the cavity centrelines, the step lines, the cylinder forces and
/// pressures, and the heated cavity's heat surfaces and lines.
#[test]
fn the_probe_replies_reach_the_comparers() {
    the_cavity_centrelines_reach_the_comparer_path();
    the_step_and_heated_cavity_lines_reach_their_paths();
    the_cylinder_forces_and_pressures_reach_the_comparer_files();
    the_heat_surfaces_reach_the_comparer_files();
}

#[test]
fn a_quantity_the_replies_do_not_hold_has_no_file() {
    let study = template("cylinder");
    let plan = study.monitor_plan();
    let lattice = obstacles::lattice(&study).unwrap();
    let z = lattice.slab() / 2.0;
    let mut stream = Stream::good();

    // One component of a line is not a velocity line.
    let (lx, ly) = (study.domain.length_x, study.domain.length_y);
    let xyz: Vec<[f64; 3]> = (0..5)
        .map(|i| [lx / 2.0, f64::from(i) * ly / 4.0, z])
        .collect();
    stream.probe(
        json!({"step": "Run", "line": {
            "from": [lx / 2.0, 0.0, z], "to": [lx / 2.0, ly, z], "count": 5, "field": "velocity_x",
        }}),
        &json!({"xyz": xyz, "values": [0.0, 0.1, 0.2, 0.3, 0.4]}),
    );

    // A point that lies in the body, an answer that is an error, and a force with no wall of the body.
    let front = plan
        .probes
        .iter()
        .find(|p| p.label.as_deref() == Some("front"))
        .unwrap();
    stream.probe(
        json!({"step": "Run", "points": [[front.x, front.y, z]]}),
        &json!({"time": 1.0, "points": [{"cell": [1, 1, 1], "solid": true}]}),
    );
    let id = stream.probe(
        json!({"step": "Run", "points": [[front.x, front.y, z]]}),
        &json!({}),
    );
    stream.replies[id as usize - 1] =
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -1, "message": "refused"}});
    stream.probe(
        json!({"step": "Run", "forces": ["y_min"]}),
        &json!({"times": [1.0], "forces": {"y_min": [[1.0, 1.0, 0.0]]}}),
    );
    let results = view(stream.files(), &study);
    let end = study.solver.end_time.to_string();
    assert!(
        results
            .read(&format!("postProcessing/centreline/{end}/vertical_U.xy"))
            .is_none()
    );
    assert!(
        results.list("postProcessing").is_empty(),
        "{:?}",
        results.list("postProcessing")
    );

    // The force on a wall of the box is not served even when the replies hold it: it carries the
    // rest pressure of the lattice.
    let cavity = template("lid-cavity");
    let mut stream = Stream::good();
    stream.probe(
        json!({"step": "Run", "forces": ["y_max"]}),
        &json!({"times": [1.0, 2.0], "forces": {"y_max": [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]}}),
    );
    let results = view(stream.files(), &cavity);
    assert!(results.list("postProcessing").is_empty());
}

#[test]
fn a_reply_file_past_the_read_cap_gives_no_view() {
    let study = template("lid-cavity");
    let (lx, ly) = (study.domain.length_x, study.domain.length_y);
    let mut stream = Stream::good();
    stream.velocity_line([lx / 2.0, 0.0], [lx / 2.0, ly], 0.001, 5, field);
    let end = study.solver.end_time.to_string();
    let path = format!("postProcessing/centreline/{end}/vertical_U.xy");
    let whole = stream.files();

    // The same replies give the file when the replies file is within the cap.
    assert!(view(whole.clone(), &study).read(&path).is_some());

    // Padded past the cap, the replies are the same lines and the view gives nothing built from them.
    let padding = " ".repeat(usize::try_from(READ_CAP_BYTES).unwrap() + 1);
    let long = {
        let text = whole.files["replies.jsonl"].clone();
        whole
            .clone()
            .with("replies.jsonl", &format!("{text}{padding}"))
    };
    let results = view(long, &study);
    assert!(results.read(&path).is_none());
    assert!(
        results
            .read(&format!("postProcessing/centreline/{end}/horizontal_U.xy"))
            .is_none()
    );
    assert!(results.list("postProcessing").is_empty());
    assert!(
        results
            .list(&format!("postProcessing/centreline/{end}"))
            .is_empty()
    );

    // The same for a request file past the cap.
    let long_requests = {
        let text = whole.files["commands.jsonl"].clone();
        whole.with("commands.jsonl", &format!("{text}{padding}"))
    };
    assert!(view(long_requests, &study).read(&path).is_none());
}
