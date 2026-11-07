//! The probe calls that follow the solve in the lattice command stream, and the round trip from
//! those calls to the files the benchmark comparers read. Every reply is made from the parsed
//! requests of the stream and not from a copy of the writer's numbers; no solver ran.

#[path = "support/replies.rs"]
mod replies;

use cfdcraft_backend::{Backend, CompileOptions};
use cfdcraft_backend_lbm::{
    backends,
    markers::{BODY_FACE, FIELD_VELOCITY, PROBE_COMMAND},
    obstacles,
};
use cfdcraft_gate::ResultFiles;
use cfdcraft_model::{MonitorField, ResolvedStudy, Study, resolve};
use cfdcraft_monitors::{parse_coefficients, parse_probes, parse_surface, parse_xy};
use cfdcraft_templates::{MeshPreset, TemplateOptions, build};
use replies::{Mem, good_solve};
use serde_json::{Value, json};

fn backend() -> &'static dyn Backend {
    backends()[0]
}

/// A template study at a mesh preset with `edit` applied to its JSON.
fn template_at(id: &str, mesh: MeshPreset, edit: impl Fn(&mut Value)) -> ResolvedStudy {
    let options = TemplateOptions {
        mesh,
        ..TemplateOptions::default()
    };
    let study = build(id, &options).unwrap();
    let mut value = serde_json::to_value(&study).unwrap();
    edit(&mut value);
    resolve(&Study::from_json(&value.to_string()).unwrap()).unwrap()
}

/// A template study at the coarsest mesh with `edit` applied to its JSON.
fn template(id: &str, edit: impl Fn(&mut Value)) -> ResolvedStudy {
    template_at(id, MeshPreset::Coarse, edit)
}

/// The step with a flat inlet, which the lattice can take.
fn flat_step() -> ResolvedStudy {
    template("backward-step-laminar", |v| {
        v["boundary_conditions"]["inlet"]
            .as_object_mut()
            .unwrap()
            .remove("profile");
    })
}

/// The heated cavity without wall grading, which the lattice cannot take.
fn heated() -> ResolvedStudy {
    template("heated-cavity", |v| {
        v["mesh"].as_object_mut().unwrap().remove("grading");
    })
}

/// The lines of the compiled stream, parsed.
fn stream_of(study: &ResolvedStudy) -> Vec<Value> {
    let tree = backend()
        .compile(study, &CompileOptions::default())
        .unwrap();
    assert!(backend().scan(&tree).is_empty());
    assert!(backend().read_back(study, &tree).is_empty());
    String::from_utf8(tree.files[0].bytes.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The probe calls of a stream, in order: the number of the request and its parameters. Every one
/// is its own `execute` call.
fn probes(lines: &[Value]) -> Vec<(u64, Value)> {
    let mut out = Vec::new();
    for line in lines {
        let arguments = &line["params"]["arguments"];
        if arguments["command"] == PROBE_COMMAND {
            assert_eq!(line["params"]["name"], "execute");
            out.push((line["id"].as_u64().unwrap(), arguments["params"].clone()));
        }
    }
    out
}

/// The thickness of the slab, read from the domain the stream sets up.
fn slab_of(lines: &[Value]) -> f64 {
    let commands = lines[1]["params"]["arguments"]["commands"]
        .as_array()
        .unwrap();
    let domain = commands
        .iter()
        .find(|c| c["command"] == "model.fluid_domain.create")
        .unwrap();
    triple(&domain["params"]["max"])[2] - triple(&domain["params"]["min"])[2]
}

fn triple(value: &Value) -> [f64; 3] {
    let items = value.as_array().unwrap();
    assert_eq!(items.len(), 3);
    [
        items[0].as_f64().unwrap(),
        items[1].as_f64().unwrap(),
        items[2].as_f64().unwrap(),
    ]
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

fn close3(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| close(a[i], b[i]))
}

/// The field the synthetic lines are cut from.
fn field(x: f64, y: f64) -> [f64; 3] {
    [0.5 * y + 0.1 * x, -0.25 * x + 0.05 * y, 0.0]
}

/// The answer to a line request: the field at the sample points the request names.
fn line_answer(line: &Value) -> Value {
    let (from, to) = (triple(&line["from"]), triple(&line["to"]));
    let count = line["count"].as_u64().unwrap() as usize;
    let component = FIELD_VELOCITY
        .iter()
        .position(|name| line["field"] == *name)
        .unwrap();
    let xyz: Vec<[f64; 3]> = (0..count)
        .map(|i| {
            let t = i as f64 / (count - 1) as f64;
            [
                from[0] + t * (to[0] - from[0]),
                from[1] + t * (to[1] - from[1]),
                from[2],
            ]
        })
        .collect();
    let values: Vec<f64> = xyz.iter().map(|p| field(p[0], p[1])[component]).collect();
    json!({"xyz": xyz, "values": values})
}

/// A reply with its answer carried as the text of a content item.
fn text_reply(id: u64, answer: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"content": [{"type": "text", "text": answer.to_string()}], "isError": false},
    })
}

/// The files of a run of the stream: the requests as written, and a reply to each, with every
/// probe answered from its own parameters.
fn run_of(lines: &[Value], answer: impl Fn(&Value) -> Value) -> Mem {
    let mut commands = String::new();
    let mut replies = String::new();
    for line in lines {
        commands.push_str(&line.to_string());
        commands.push('\n');
        let id = line["id"].as_u64().unwrap();
        let arguments = &line["params"]["arguments"];
        let reply = if line["method"] == "initialize" {
            json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": "2025-06-18"}})
        } else if arguments["command"] == PROBE_COMMAND {
            text_reply(id, &answer(&arguments["params"]))
        } else if arguments["command"] == "solve.fluid" {
            text_reply(id, &good_solve())
        } else {
            text_reply(id, &json!({"ok": true}))
        };
        replies.push_str(&reply.to_string());
        replies.push('\n');
    }
    Mem::default()
        .with("commands.jsonl", &commands)
        .with("replies.jsonl", &replies)
        .with("log.fieldcraft", "fieldcraft-cli: server started\n")
}

/// The answer of a solver that holds [`field`] and nothing else.
fn answer_lines_only(params: &Value) -> Value {
    match params.get("line") {
        Some(line) => line_answer(line),
        None => json!({}),
    }
}

fn view(files: Mem, study: &ResolvedStudy) -> Box<dyn ResultFiles> {
    backend().results(Box::new(files), study)
}

/// The three line probes for the line from `from` to `to`, found in the stream by geometry: each
/// component once, in the order of [`FIELD_VELOCITY`], one right after the other.
fn line_calls(all: &[(u64, Value)], from: [f64; 3], to: [f64; 3]) -> Vec<(u64, Value)> {
    let found: Vec<(u64, Value)> = all
        .iter()
        .filter(|(_, params)| {
            params
                .get("line")
                .is_some_and(|l| close3(triple(&l["from"]), from) && close3(triple(&l["to"]), to))
        })
        .cloned()
        .collect();
    assert!(!found.is_empty(), "no probe for the line {from:?} {to:?}");
    found
}

/// The three probes of a line: its start, its end and its sample count are the same in all three.
fn assert_velocity_line(
    all: &[(u64, Value)],
    from: [f64; 3],
    to: [f64; 3],
    count: u64,
    name: &str,
) {
    let found = line_calls(all, from, to);
    assert_eq!(found.len() % 3, 0, "{name}");
    for chunk in found.chunks(3) {
        for (component, (id, params)) in chunk.iter().enumerate() {
            let line = &params["line"];
            assert_eq!(params["step"], "Run", "{name}");
            assert_eq!(line["field"], FIELD_VELOCITY[component], "{name} {id}");
            assert_eq!(line["count"].as_u64(), Some(count), "{name} {id}");
            assert!(close3(triple(&line["from"]), from), "{name} {id}");
            assert!(close3(triple(&line["to"]), to), "{name} {id}");
        }
        // One right after the other.
        assert_eq!(chunk[1].0, chunk[0].0 + 1, "{name}");
        assert_eq!(chunk[2].0, chunk[0].0 + 2, "{name}");
    }
}

#[test]
fn the_cavity_stream_asks_for_the_centrelines() {
    let study = template("lid-cavity", |_| {});
    let lines = stream_of(&study);
    // The opening, the setup and the solve come first.
    assert_eq!(lines[0]["method"], "initialize");
    assert_eq!(lines[1]["params"]["name"], "batch");
    assert_eq!(lines[2]["params"]["arguments"]["command"], "solve.fluid");
    let asked = probes(&lines);
    assert!(asked.len() >= 6, "{}", asked.len());

    let (lx, ly) = (study.domain.length_x, study.domain.length_y);
    let spacing = lx / study.mesh.cells_x as f64;
    let z = 1.5 * spacing;
    let count = study.monitors.centreline_u_sample_count;
    let centrelines = [
        ([lx / 2.0, 0.0, z], [lx / 2.0, ly, z]),
        ([0.0, ly / 2.0, z], [lx, ly / 2.0, z]),
    ];
    for (k, (from, to)) in centrelines.iter().enumerate() {
        for (component, field) in FIELD_VELOCITY.iter().enumerate() {
            let index = 3 * k + component;
            let (id, params) = &asked[index];
            // The numbers run on from the solve, one for each call.
            assert_eq!(*id, 4 + index as u64);
            let line = &params["line"];
            assert_eq!(line["field"], *field);
            assert_eq!(line["count"].as_u64(), Some(count));
            assert!(close3(triple(&line["from"]), *from), "{index}");
            assert!(close3(triple(&line["to"]), *to), "{index}");
            assert_eq!(params["step"], "Run");
            assert_eq!(params.as_object().unwrap().len(), 2, "{params}");
        }
    }
}

#[test]
fn a_velocity_line_is_three_line_probes_from_its_start() {
    // The step's monitor plan, with a pressure line added: the view has no pressure line, so none
    // is asked for.
    let study = template("backward-step-laminar", |v| {
        v["boundary_conditions"]["inlet"]
            .as_object_mut()
            .unwrap()
            .remove("profile");
        let mut pressure = v["monitors"]["lines"]["floor"].clone();
        pressure["field"] = json!("pressure");
        v["monitors"]["lines"]["zz_pressure"] = pressure;
        // A second velocity line, up from the floor at the step's foot.
        let mut profile = v["monitors"]["lines"]["floor"].clone();
        profile["x1"] = profile["x0"].clone();
        profile["y0"] = json!({"value": 0.0, "unit": "mm"});
        profile["y1"] = json!({"value": 5.0, "unit": "mm"});
        v["monitors"]["lines"]["zz_profile"] = profile;
    });
    let plan = study.monitor_plan();
    let lines = stream_of(&study);
    let asked = probes(&lines);
    let z = slab_of(&lines) / 2.0;
    let mut velocity_lines = 0;
    for line in &plan.lines {
        if line.field == MonitorField::Velocity {
            velocity_lines += 1;
            let (from, to) = ([line.x0, line.y0, z], [line.x1, line.y1, z]);
            assert_velocity_line(&asked, from, to, line.samples, &line.fo);
        }
    }
    assert!(velocity_lines >= 2, "{velocity_lines}");
    assert!(
        plan.lines.len() > velocity_lines,
        "the plan has a pressure line"
    );
    // Every line probe of the stream belongs to a velocity line of the plan, three calls each, so
    // the pressure line is not asked for.
    let line_probes = asked
        .iter()
        .filter(|(_, p)| p.get("line").is_some())
        .count();
    assert_eq!(line_probes, 3 * velocity_lines);
}

/// Whether the file at `path` of the view holds the line asked for in the stream, with the values
/// of [`field`] at the sample points the request names.
fn assert_served(
    results: &dyn ResultFiles,
    asked: &[(u64, Value)],
    path: &str,
    from: [f64; 3],
    to: [f64; 3],
) {
    // A line the study asks for twice (the heated cavity's middle lines are its centrelines) may
    // be asked with different sample counts: the file holds one of the asked counts.
    let counts: Vec<u64> = line_calls(asked, from, to)
        .iter()
        .map(|(_, params)| params["line"]["count"].as_u64().unwrap())
        .collect();
    let text = results
        .read(path)
        .unwrap_or_else(|| panic!("the view serves no file {path}"));
    let file = parse_xy(&text).unwrap();
    assert_eq!(file.columns, 4, "{path}");
    let count = file.rows.len();
    assert!(
        counts.contains(&(count as u64)),
        "{path}: {count} in {counts:?}"
    );
    let length = (to[0] - from[0]).hypot(to[1] - from[1]);
    for (i, row) in file.rows.iter().enumerate() {
        let t = i as f64 / (count - 1) as f64;
        let (x, y) = (
            from[0] + t * (to[0] - from[0]),
            from[1] + t * (to[1] - from[1]),
        );
        assert!(close(row[0], t * length), "{path}: distance at {i}");
        for (k, want) in field(x, y).iter().enumerate() {
            assert!(close(row[1 + k], *want), "{path}: component {k} at {i}");
        }
    }
}

#[test]
fn the_view_serves_every_line_file_the_writer_asks_for() {
    for (name, study) in [
        ("cavity", template("lid-cavity", |_| {})),
        ("step", flat_step()),
        ("heated cavity", heated()),
    ] {
        let lines = stream_of(&study);
        let asked = probes(&lines);
        let results = view(run_of(&lines, answer_lines_only), &study);
        let end = study.solver.end_time.to_string();
        let z = slab_of(&lines) / 2.0;
        let mut served = 0;
        if study.domain.shape.is_none() {
            let (lx, ly) = (study.domain.length_x, study.domain.length_y);
            for (file, from, to) in [
                ("vertical_U.xy", [lx / 2.0, 0.0, z], [lx / 2.0, ly, z]),
                ("horizontal_U.xy", [0.0, ly / 2.0, z], [lx, ly / 2.0, z]),
            ] {
                let path = format!("postProcessing/centreline/{end}/{file}");
                assert_served(results.as_ref(), &asked, &path, from, to);
                served += 1;
            }
        }
        for line in &study.monitor_plan().lines {
            let (from, to) = ([line.x0, line.y0, z], [line.x1, line.y1, z]);
            let path = format!("postProcessing/{}/{end}/line_U.xy", line.fo);
            assert_served(results.as_ref(), &asked, &path, from, to);
            served += 1;
        }
        // Both centrelines of a study with no shape, and each line of its plan.
        let expected =
            if study.domain.shape.is_none() { 2 } else { 0 } + study.monitor_plan().lines.len();
        assert_eq!(served, expected, "{name}");
    }
}

/// The calls of a stream that ask for point probes, in order.
fn point_calls(asked: &[(u64, Value)]) -> Vec<&Value> {
    asked
        .iter()
        .map(|(_, params)| params)
        .filter(|params| params.get("points").is_some())
        .collect()
}

/// The setup commands of a stream: the domain and the step the stream sets up.
fn setup_command<'a>(lines: &'a [Value], name: &str) -> &'a Value {
    let commands = lines[1]["params"]["arguments"]["commands"]
        .as_array()
        .unwrap();
    &commands.iter().find(|c| c["command"] == name).unwrap()["params"]
}

#[test]
fn the_cavity_stream_asks_for_the_default_probes_once_per_frame() {
    let study = template("lid-cavity", |_| {});
    let lines = stream_of(&study);
    let asked = probes(&lines);
    let frames = setup_command(&lines, "model.step.create")["procedure"]["frames"]
        .as_u64()
        .unwrap();
    assert_eq!(frames, 10);
    // Six centreline calls, then one point call for each frame, and nothing else.
    assert_eq!(asked.len() as u64, 6 + frames);
    assert!(asked[..6].iter().all(|(_, p)| p.get("line").is_some()));
    let plan = study.monitor_plan();
    assert_eq!(plan.probes.len(), 5);
    let z = slab_of(&lines) / 2.0;
    for (k, (id, params)) in asked[6..].iter().enumerate() {
        assert_eq!(*id, 10 + k as u64);
        assert_eq!(params["frame"].as_u64(), Some(k as u64 + 1));
        assert_eq!(params["step"], "Run");
        assert_eq!(params.as_object().unwrap().len(), 3, "{params}");
        let points = params["points"].as_array().unwrap();
        assert_eq!(points.len(), plan.probes.len());
        for (point, probe) in points.iter().zip(&plan.probes) {
            assert!(close3(triple(point), [probe.x, probe.y, z]), "{point}");
        }
    }
}

/// The distance from a point to the closed outline.
fn distance_to_outline(outline: &[[f64; 2]], p: [f64; 2]) -> f64 {
    let mut least = f64::INFINITY;
    for (i, a) in outline.iter().enumerate() {
        let b = outline[(i + 1) % outline.len()];
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        least = least.min((p[0] - (a[0] + t * dx)).hypot(p[1] - (a[1] + t * dy)));
    }
    least
}

#[test]
fn a_surface_point_is_moved_off_the_body() {
    let study = template("cylinder", |_| {});
    let lines = stream_of(&study);
    let asked = probes(&lines);
    let calls = point_calls(&asked);
    let frames = setup_command(&lines, "model.step.create")["procedure"]["frames"]
        .as_u64()
        .unwrap();
    // The two surface pressure probes are asked in one call for each frame.
    assert_eq!(calls.len() as u64, frames);
    let plan = study.monitor_plan();
    assert_eq!(plan.probes.len(), 2);
    let lattice = obstacles::lattice(&study).unwrap();
    let outline = &lattice.obstacle.as_ref().unwrap().outline;
    let cell = lattice.spacing;
    let (centre_x, centre_y) = (0.2, 0.2);
    let radius = 0.05;
    for call in calls {
        let points = call["points"].as_array().unwrap();
        assert_eq!(points.len(), 2);
        for (point, probe) in points.iter().zip(&plan.probes) {
            let at = triple(point);
            // On the fluid side of the body.
            assert!(
                (at[0] - centre_x).hypot(at[1] - centre_y) > radius,
                "{at:?}"
            );
            // Farther from the outline than half the diagonal of a cell, which a fluid cell
            // is certain to be, and within one cell of the planned point.
            let off = distance_to_outline(outline, [at[0], at[1]]);
            assert!(off > cell * std::f64::consts::SQRT_2 / 2.0, "{off}");
            assert!((at[0] - probe.x).hypot(at[1] - probe.y) <= cell, "{at:?}");
        }
    }
}

/// The walls named by the calls of one form, in order.
fn named(asked: &[(u64, Value)], form: &str) -> Vec<Vec<String>> {
    asked
        .iter()
        .filter_map(|(_, params)| params.get(form))
        .map(|list| {
            list.as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap().to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn forces_and_heat_name_their_walls_and_other_forces_are_not_asked() {
    // The cylinder: one force probe naming the face of the body, also when a force on the walls
    // of the channel is planned too.
    let wall_force = |v: &mut Value| {
        let mut force = v["monitors"]["forces"]["cylinder_forces"].clone();
        force["boundary"] = json!("channel_walls");
        v["monitors"]["forces"]["wall_forces"] = force;
    };
    for study in [
        template("cylinder", |_| {}),
        template("cylinder", wall_force),
    ] {
        let asked = probes(&stream_of(&study));
        assert_eq!(
            named(&asked, "forces"),
            [[format!("Cylinder/{BODY_FACE}")]],
            "{}",
            study.monitor_plan().forces.len()
        );
        assert!(named(&asked, "heat").is_empty());
    }
    // The heated cavity: one heat probe naming the sides of its two heat-flux surfaces.
    let asked = probes(&stream_of(&heated()));
    assert_eq!(named(&asked, "heat"), [["x_min", "x_max"]]);
    assert!(named(&asked, "forces").is_empty());
    // The cavity holds neither, and neither does a cavity with a force on its walls.
    let asked = probes(&stream_of(&template("lid-cavity", |_| {})));
    assert!(named(&asked, "forces").is_empty() && named(&asked, "heat").is_empty());
    let walls = template("lid-cavity", |v| {
        v["monitors"]["forces"] = json!({"wall_force": {
            "boundary": "walls",
            "reference_velocity": {"value": 0.1, "unit": "m/s"},
            "reference_length": {"value": 0.1, "unit": "m"},
        }});
    });
    let asked = probes(&stream_of(&walls));
    assert!(named(&asked, "forces").is_empty() && named(&asked, "heat").is_empty());
}

/// The frames the step records and the cells of the lattice, from the stream itself.
fn frames_and_cells(lines: &[Value]) -> (u64, u64) {
    let frames = setup_command(lines, "model.step.create")["procedure"]["frames"]
        .as_u64()
        .unwrap();
    let domain = setup_command(lines, "model.fluid_domain.create");
    let (low, high) = (triple(&domain["min"]), triple(&domain["max"]));
    let spacing = domain["spacing"].as_f64().unwrap();
    let along = |axis: usize| ((high[axis] - low[axis]) / spacing).round() as u64;
    (frames, along(0) * along(1) * along(2))
}

#[test]
fn the_frame_count_follows_the_time_mode_and_the_memory_budget() {
    // A steady study sets ten frames and asks once for each.
    let steady = stream_of(&template("lid-cavity", |_| {}));
    assert_eq!(frames_and_cells(&steady).0, 10);
    assert_eq!(point_calls(&probes(&steady)).len(), 10);

    // A transient study sets the most the solver allows at its coarse mesh, and fewer as the
    // lattice grows, never below ten, so the recorded frames stay inside half the solver's default
    // cap on frame memory (one billion bytes at twenty bytes per cell).
    let mut previous = u64::MAX;
    let mut first = true;
    for preset in [MeshPreset::Coarse, MeshPreset::Medium, MeshPreset::Fine] {
        let study = template_at("cylinder", preset, |_| {});
        let lines = stream_of(&study);
        let (frames, cells) = frames_and_cells(&lines);
        if first {
            assert_eq!(frames, 1000, "the most the solver allows");
            first = false;
        }
        assert!((10..=1000).contains(&frames), "{preset:?}: {frames}");
        assert!(frames <= previous, "{preset:?}: {frames} after {previous}");
        assert!(
            frames == 10 || frames * cells * 20 <= 1_000_000_000,
            "{preset:?}: {frames} frames of {cells} cells"
        );
        assert!(
            frames == 1000 || (frames + 1) * cells * 20 > 1_000_000_000,
            "{preset:?}: {frames} frames is fewer than the budget allows"
        );
        assert_eq!(point_calls(&probes(&lines)).len() as u64, frames);
        previous = frames;
    }
    assert!(previous < 1000, "a finer lattice records fewer frames");
}

/// The answer of a solver that holds [`field`] along lines, the same fluid state at every point
/// of a frame, and a steady force and heat history for every wall a request names.
fn answer_every_kind(params: &Value) -> Value {
    if let Some(line) = params.get("line") {
        return line_answer(line);
    }
    if let Some(points) = params.get("points") {
        // One flow time for each frame.
        let time = 0.5 * params["frame"].as_f64().unwrap();
        let answers: Vec<Value> = points
            .as_array()
            .unwrap()
            .iter()
            .map(|point| {
                let p = triple(point);
                json!({
                    "cell": [10, 10, 1],
                    "solid": false,
                    "velocity": [0.1 * p[0], 0.2 * p[1], 0.0],
                    "pressure": 3.0 + p[0] - p[1],
                })
            })
            .collect();
        return json!({"time": time, "points": answers});
    }
    let history = |walls: &Value, row: Value| {
        let mut map = serde_json::Map::new();
        for wall in walls.as_array().unwrap() {
            map.insert(wall.as_str().unwrap().to_owned(), json!([row, row, row]));
        }
        Value::Object(map)
    };
    if let Some(walls) = params.get("forces") {
        return json!({"times": [0.5, 1.0, 1.5], "forces": history(walls, json!([1.0, 0.5, 0.0]))});
    }
    let walls = &params["heat"];
    json!({"times": [10.0, 20.0, 30.0], "heat": history(walls, json!(2.0))})
}

#[test]
fn the_view_serves_every_file_the_writer_asks_for() {
    let mut files = 0;
    for (name, study) in [
        ("cavity", template("lid-cavity", |_| {})),
        ("step", flat_step()),
        ("cylinder", template("cylinder", |_| {})),
        ("heated cavity", heated()),
    ] {
        let lines = stream_of(&study);
        let asked = probes(&lines);
        let results = view(run_of(&lines, answer_every_kind), &study);
        let end = study.solver.end_time.to_string();
        let plan = study.monitor_plan();
        let z = slab_of(&lines) / 2.0;
        let frames = point_calls(&asked).len();

        // The lines: both centrelines of a study with no shape, and every velocity line.
        if study.domain.shape.is_none() {
            let (lx, ly) = (study.domain.length_x, study.domain.length_y);
            for (file, from, to) in [
                ("vertical_U.xy", [lx / 2.0, 0.0, z], [lx / 2.0, ly, z]),
                ("horizontal_U.xy", [0.0, ly / 2.0, z], [lx, ly / 2.0, z]),
            ] {
                let path = format!("postProcessing/centreline/{end}/{file}");
                assert_served(results.as_ref(), &asked, &path, from, to);
                files += 1;
            }
        }
        for line in &plan.lines {
            let (from, to) = ([line.x0, line.y0, z], [line.x1, line.y1, z]);
            let path = format!("postProcessing/{}/{end}/line_U.xy", line.fo);
            assert_served(results.as_ref(), &asked, &path, from, to);
            files += 1;
        }
        // A probe file for each planned probe, one row for each frame.
        for probe in &plan.probes {
            let field = match probe.field {
                MonitorField::Pressure => "p",
                MonitorField::Velocity => "U",
            };
            let path = format!("postProcessing/{}/0/{field}", probe.fo);
            let text = results
                .read(&path)
                .unwrap_or_else(|| panic!("{name}: the view serves no file {path}"));
            assert_eq!(parse_probes(&text).unwrap().rows.len(), frames, "{path}");
            files += 1;
        }
        // A coefficient file for the force on the body, from the face the request named.
        let on_body = |boundary: &str| {
            study.boundary_conditions[boundary].faces == [cfdcraft_model::Face::Obstacle]
        };
        for force in plan.forces.iter().filter(|f| on_body(&f.boundary)) {
            let path = format!("postProcessing/{}/0/coefficient.dat", force.fo);
            let text = results
                .read(&path)
                .unwrap_or_else(|| panic!("{name}: the view serves no file {path}"));
            assert_eq!(parse_coefficients(&text).unwrap().rows.len(), 3, "{path}");
            files += 1;
        }
        // A surface file for each heat-flux surface on a side of the box.
        for surface in &plan.surfaces {
            let path = format!("postProcessing/{}/{end}/surfaceFieldValue.dat", surface.fo);
            let text = results
                .read(&path)
                .unwrap_or_else(|| panic!("{name}: the view serves no file {path}"));
            assert_eq!(parse_surface(&text).unwrap().rows.len(), 3, "{path}");
            files += 1;
        }
    }
    // Every planned file was asserted served above; this pins how many the four studies plan, so a
    // study that quietly plans fewer or more files is seen.
    assert_eq!(files, 17, "{files}");
}
