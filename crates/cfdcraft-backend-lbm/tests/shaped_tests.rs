//! The step and the cylinder with their obstacle files, the parabolic inlet, the transient run and
//! the heated cavity in the command stream, and the settings the solver cannot represent, which
//! stay refused and named.

mod support;

use cfdcraft_backend::{Backend, CaseTree, CompileFailure, CompileOptions};
use cfdcraft_backend_lbm::{backends, claims::CLAIMS, commands::STEADY_TOLERANCE};
use cfdcraft_model::{ResolvedStudy, ResolvedThermal, Study, TurbulenceModel, diagnose, resolve};
use cfdcraft_templates::{MeshPreset, TemplateOptions, TimeChoice, build};
use serde_json::{Value, json};
use support::model_text;

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn backend() -> &'static dyn Backend {
    backends()[0]
}

/// A template study with `edit` applied to its JSON.
fn study(id: &str, mesh: MeshPreset, time: Option<TimeChoice>, edit: impl Fn(&mut Value)) -> Study {
    let options = TemplateOptions {
        mesh,
        time,
        ..TemplateOptions::default()
    };
    let template = build(id, &options).unwrap();
    let mut value = serde_json::to_value(&template).unwrap();
    edit(&mut value);
    Study::from_json(&value.to_string()).unwrap()
}

fn resolved(study: &Study) -> ResolvedStudy {
    resolve(study).unwrap()
}

fn cavity_fixture(edit: impl Fn(&mut Value)) -> Study {
    let mut value: Value = serde_json::from_str(CAVITY_V1).unwrap();
    edit(&mut value);
    Study::from_json(&value.to_string()).unwrap()
}

fn compile(study: &ResolvedStudy) -> CaseTree {
    backend()
        .compile(study, &CompileOptions::default())
        .unwrap()
}

fn refused(study: &ResolvedStudy) -> CompileFailure {
    backend()
        .compile(study, &CompileOptions::default())
        .unwrap_err()
}

fn setting(failure: CompileFailure) -> String {
    match failure {
        CompileFailure::NotSupported { setting, .. } => setting,
        other => panic!("{other:?}"),
    }
}

fn names(tree: &CaseTree) -> Vec<&str> {
    tree.files
        .iter()
        .map(|file| file.rel_path.to_str().unwrap())
        .collect()
}

fn stream_text(tree: &CaseTree) -> String {
    String::from_utf8(tree.files[0].bytes.clone()).unwrap()
}

/// Every command of the stream in order, with its parameters.
fn commands_of(text: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let message: Value = serde_json::from_str(line).unwrap();
        let arguments = &message["params"]["arguments"];
        let entries = match message["params"]["name"].as_str().unwrap() {
            "batch" => arguments["commands"].as_array().unwrap().clone(),
            _ => vec![arguments.clone()],
        };
        for entry in entries {
            out.push((
                entry["command"].as_str().unwrap().to_owned(),
                entry["params"].clone(),
            ));
        }
    }
    out
}

/// The names of the commands up to and including the solve. Every command after it is a probe.
fn order_of(commands: &[(String, Value)]) -> Vec<&str> {
    let solve = commands
        .iter()
        .position(|(name, _)| name == "solve.fluid")
        .unwrap();
    assert!(
        commands[solve + 1..]
            .iter()
            .all(|(name, _)| name == "solve.probe")
    );
    commands[..=solve]
        .iter()
        .map(|(name, _)| name.as_str())
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs())
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap()
}

const WITH_OBSTACLE: [&str; 6] = [
    "model.units",
    "model.material.create",
    "geometry.import.mesh",
    "model.fluid_domain.create",
    "model.step.create",
    "solve.fluid",
];

/// The step with a flat inlet: its template has a parabolic one, which the solver cannot spread
/// over the channel above the step.
fn flat_step() -> Study {
    study("backward-step-laminar", MeshPreset::Coarse, None, |v| {
        v["boundary_conditions"]["inlet"]
            .as_object_mut()
            .unwrap()
            .remove("profile");
    })
}

#[test]
fn the_step_and_the_cylinder_carry_their_obstacle() {
    // The cylinder: a parabolic inlet at its peak, a convective outlet, periodic slab ends.
    for (preset, spacing) in [
        (MeshPreset::Coarse, 0.01),
        (MeshPreset::Medium, 0.005),
        (MeshPreset::Fine, 0.0025),
    ] {
        let cylinder = resolved(&study("cylinder", preset, None, |_| {}));
        let tree = compile(&cylinder);
        assert_eq!(names(&tree), ["commands.jsonl", "obstacles/cylinder.3mf"]);
        assert!(backend().scan(&tree).is_empty());
        assert!(backend().read_back(&cylinder, &tree).is_empty());
        let commands = commands_of(&stream_text(&tree));
        let order = order_of(&commands);
        assert_eq!(order, WITH_OBSTACLE);
        assert_eq!(commands[2].1, json!({"path": "obstacles/cylinder.3mf"}));
        let domain = &commands[3].1;
        assert!(close(number(&domain["spacing"]), spacing), "{preset:?}");
        assert_eq!(domain["obstacles"], json!(["Cylinder"]));
        let faces = &domain["faces"];
        assert_eq!(faces["x_min"]["type"], "velocity");
        assert_eq!(faces["x_min"]["profile"], "parabolic");
        let peak: Vec<f64> = faces["x_min"]["velocity"]
            .as_array()
            .unwrap()
            .iter()
            .map(number)
            .collect();
        assert!(
            close(peak[0], 1.5) && peak[1] == 0.0 && peak[2] == 0.0,
            "{peak:?}"
        );
        assert_eq!(
            faces["x_max"],
            json!({"type": "pressure", "pressure": 0.0, "outflow": "convective"})
        );
        for side in ["y_min", "y_max"] {
            assert_eq!(faces[side], json!({"type": "wall"}), "{side}");
        }
        for side in ["z_min", "z_max"] {
            assert_eq!(faces[side], json!({"type": "periodic"}), "{side}");
        }
        let surface = model_text(&tree.files[1].bytes);
        assert!(surface.contains("name=\"Cylinder\""));
        assert!(surface.matches("<vertex ").count() >= 8);
    }

    // The step with a flat inlet: the inlet carries the mean speed of the study.
    let step = resolved(&flat_step());
    let tree = compile(&step);
    assert_eq!(names(&tree), ["commands.jsonl", "obstacles/step.3mf"]);
    assert!(backend().scan(&tree).is_empty());
    assert!(backend().read_back(&step, &tree).is_empty());
    let commands = commands_of(&stream_text(&tree));
    let order = order_of(&commands);
    assert_eq!(order, WITH_OBSTACLE);
    assert_eq!(commands[2].1, json!({"path": "obstacles/step.3mf"}));
    let domain = &commands[3].1;
    assert!(close(number(&domain["spacing"]), 1e-4));
    assert_eq!(domain["obstacles"], json!(["Step"]));
    let inlet = &domain["faces"]["x_min"];
    assert_eq!(inlet["type"], "velocity");
    assert_eq!(inlet["profile"], "uniform");
    let mean = step.boundary_conditions["inlet"].velocity.unwrap();
    assert!(close(number(&inlet["velocity"][0]), mean));
    assert_eq!(domain["faces"]["x_max"]["type"], "pressure");
    for side in ["z_min", "z_max"] {
        assert_eq!(domain["faces"][side], json!({"type": "slip"}), "{side}");
    }
    let surface = model_text(&tree.files[1].bytes);
    assert!(surface.contains("name=\"Step\""));
    assert_eq!(surface.matches("<vertex ").count(), 8);
    assert_eq!(surface.matches("<triangle ").count(), 12);

    // The parabolic step is refused by name, not given a flat inlet.
    let parabolic = resolved(&study(
        "backward-step-laminar",
        MeshPreset::Coarse,
        None,
        |_| {},
    ));
    assert_eq!(setting(refused(&parabolic)), "inlet profile");
}

#[test]
fn the_transient_cylinder_sets_the_end_time_and_leaves_the_step_automatic() {
    let transient = resolved(&study("cylinder", MeshPreset::Coarse, None, |_| {}));
    let commands = commands_of(&stream_text(&compile(&transient)));
    let procedure = &commands[4].1["procedure"];
    assert!(close(
        number(&procedure["end_time"]),
        transient.solver.end_time
    ));
    assert!(procedure.get("steady").is_none(), "{procedure}");
    assert!(procedure.get("time_step").is_none(), "{procedure}");

    // A steady study keeps the flow-through rule of the cavity: fifty crossings at the mean speed.
    let steady = resolved(&study(
        "cylinder",
        MeshPreset::Coarse,
        Some(TimeChoice::Steady),
        |_| {},
    ));
    let commands = commands_of(&stream_text(&compile(&steady)));
    let procedure = &commands[4].1["procedure"];
    assert!(close(number(&procedure["end_time"]), 50.0 * 2.2 / 1.0));
    assert!(close(
        number(&procedure["steady"]["tolerance"]),
        STEADY_TOLERANCE
    ));
    assert!(
        backend().run_plan(&transient, 1).transient,
        "the plan says the run is in time"
    );
    assert!(!backend().run_plan(&steady, 1).transient);
}

#[test]
fn the_heated_cavity_writes_the_thermal_setup() {
    let ungraded = study("heated-cavity", MeshPreset::Coarse, None, |v| {
        v["mesh"].as_object_mut().unwrap().remove("grading");
    });
    let heated = resolved(&ungraded);
    let buoyancy = heated.physics.as_ref().unwrap().buoyancy.unwrap();
    let tree = compile(&heated);
    assert_eq!(names(&tree), ["commands.jsonl"]);
    assert!(backend().scan(&tree).is_empty());
    assert!(backend().read_back(&heated, &tree).is_empty());
    let commands = commands_of(&stream_text(&tree));
    let order = order_of(&commands);
    assert_eq!(
        order,
        [
            "model.units",
            "model.material.create",
            "model.fluid_domain.create",
            "model.step.create",
            "solve.fluid"
        ]
    );

    // The conductivity gives the study's Prandtl number: viscosity over conductivity per
    // density and specific heat.
    let material = &commands[1].1;
    let density = number(&material["density"]);
    let specific_heat = number(&material["thermal"]["specific_heat"]);
    let conductivity = number(&material["thermal"]["conductivity"]);
    let viscosity = number(&material["fluid"]["viscosity"]) / density;
    let prandtl = viscosity / (conductivity / (density * specific_heat));
    assert!((prandtl - buoyancy.prandtl).abs() <= 1e-12 * buoyancy.prandtl);

    let domain = &commands[2].1;
    assert!(domain.get("acceleration").is_none(), "{domain}");
    assert!(domain.get("obstacles").is_none());
    let thermal = &domain["thermal"];
    assert!(close(
        number(&thermal["initial_temperature"]),
        buoyancy.reference_temperature
    ));
    let gravity: Vec<f64> = thermal["gravity"]
        .as_array()
        .unwrap()
        .iter()
        .map(number)
        .collect();
    assert_eq!(gravity[0], 0.0);
    assert!(close(gravity[1], -buoyancy.gravity));
    assert_eq!(gravity[2], 0.0);
    assert!(close(number(&thermal["expansion"]), buoyancy.beta));
    // The hot wall is at x_min and the cold one at x_max; nothing else is held.
    let held = thermal["faces"].as_object().unwrap();
    assert_eq!(held.len(), 2, "{held:?}");
    assert_eq!(held["x_min"]["type"], "temperature");
    assert!(close(number(&held["x_min"]["value"]), 300.5));
    assert!(close(number(&held["x_max"]["value"]), 299.5));
    // The lattice counts from the cold wall.
    assert!(close(number(&thermal["temperature_zero"]), 299.5));
    assert_eq!(domain["faces"]["z_min"], json!({"type": "periodic"}));

    // The time step holds the lattice speed of the buoyancy speed at the solver's target.
    let speed = (buoyancy.gravity * buoyancy.beta * 1.0 * 1.0).sqrt();
    let procedure = &commands[3].1["procedure"];
    assert!(close(
        number(&procedure["time_step"]),
        0.05 * (1.0 / 40.0) / speed
    ));
    assert!(close(number(&procedure["end_time"]), 50.0 * 1.0 / speed));
    assert!(procedure.get("steady").is_some());
    assert!(backend().run_plan(&heated, 1).buoyant);
}

#[test]
fn unrepresentable_settings_stay_diagnostics() {
    let codes = |study: &Study| -> Vec<String> {
        diagnose(study, &backend().capabilities())
            .into_iter()
            .map(|d| d.code)
            .collect()
    };
    let nothing_compiled = |study: &Study, code: &str, word: &str| {
        assert!(
            codes(study).iter().any(|c| c == code),
            "{code} in {:?}",
            codes(study)
        );
        match refused(&resolved(study)) {
            CompileFailure::NotSupported { setting, .. } => assert_eq!(setting, word),
            other => panic!("{other:?}"),
        }
    };
    nothing_compiled(
        &cavity_fixture(|v| v["mesh"]["grading"] = json!({"y": 1.2})),
        "CFD-BACKEND-007",
        "wall grading",
    );
    nothing_compiled(
        &cavity_fixture(|v| {
            v["solver"]["solver"] = "FromPhysics".into();
            v["physics"] = json!({"turbulence": {
                "model": "k_omega_sst", "intensity": 0.05, "viscosity_ratio": 10.0
            }});
        }),
        "CFD-BACKEND-002",
        "turbulence model",
    );
    nothing_compiled(
        &cavity_fixture(|v| {
            v["solver"]["solver"] = "FromPhysics".into();
            v["physics"] = json!({"init": "potential"});
        }),
        "CFD-BACKEND-005",
        "start of the flow",
    );
    let raw = cavity_fixture(|v| {
        v["raw_overrides"] = json!({"relax-u": {
            "backend": "esi", "file": "system/fvSolution",
            "entry": "relaxationFactors/equations/U", "value": "0.5",
        }});
    });
    assert!(codes(&raw).iter().any(|c| c == "CFD-BACKEND-006"));
    assert!(matches!(
        refused(&resolved(&raw)),
        CompileFailure::Raw { label, .. } if label == "relax-u"
    ));

    // An inlet or an outlet with fewer than two cells across it: the solver refuses it at run
    // time, so the writer refuses it first and names it.
    let channel = |inlet: &str, outlet: &str, walls: Value| {
        cavity_fixture(|v| {
            v["domain"]["length_x"] = json!({"value": 0.4, "unit": "m"});
            v["domain"]["open"] = json!(true);
            v["mesh"]["cells_x"] = json!(4);
            v["mesh"]["cells_y"] = json!(1);
            v["boundary_conditions"] = json!({
                "inlet": {
                    "faces": [inlet], "kind": "VelocityInlet",
                    "velocity": {"value": 1.0, "unit": "m/s"}
                },
                "outlet": {"faces": [outlet], "kind": "PressureOutlet"},
                "walls": {"faces": walls.clone(), "kind": "NoSlipWall"},
            });
        })
    };
    let narrow_inlet = channel("XMin", "XMax", json!(["YMin", "YMax"]));
    assert_eq!(setting(refused(&resolved(&narrow_inlet))), "inlet size");
    let narrow_outlet = channel("YMin", "XMax", json!(["XMin", "YMax"]));
    assert_eq!(setting(refused(&resolved(&narrow_outlet))), "outlet size");

    // A wall held at a temperature needs the buoyancy setting.
    // The model already refuses this study (CFD-PHYS-004), so the resolved form is edited here to
    // reach the writer's own refusal.
    let mut held = resolved(&cavity_fixture(|_| {}));
    held.boundary_conditions.get_mut("walls").unwrap().thermal =
        Some(ResolvedThermal::FixedTemperature { value: 300.0 });
    assert_eq!(setting(refused(&held)), "thermal condition");
    assert_eq!(
        backend().capabilities().turbulence_models,
        [TurbulenceModel::Laminar]
    );
}

#[test]
fn the_claim_list_is_still_empty() {
    assert_eq!(CLAIMS.len(), 0);
    let row = backend();
    assert!(row.preview());
    let limits = row.capabilities();
    assert!(limits.transient && limits.buoyancy && limits.parabolic_inlet);
    assert_eq!(limits.turbulence_models, [TurbulenceModel::Laminar]);
    assert!(!limits.potential_init && !limits.accepts_raw);
}

/// The tree with `edit` applied to the lines of its stream; the other files stay.
fn with_stream(tree: &CaseTree, edit: impl Fn(&mut Vec<Value>)) -> CaseTree {
    let mut lines: Vec<Value> = stream_text(tree)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    edit(&mut lines);
    let mut text = String::new();
    for line in &lines {
        text.push_str(&serde_json::to_string(line).unwrap());
        text.push('\n');
    }
    let mut files = tree.files.clone();
    files[0].bytes = text.into_bytes();
    CaseTree { files }
}

/// The setup command with this index, on the setup line.
fn command(lines: &mut [Value], index: usize) -> &mut Value {
    &mut lines[1]["params"]["arguments"]["commands"][index]["params"]
}

#[test]
fn the_reader_names_a_lost_surface_a_changed_body_or_a_changed_heat_setting() {
    let cylinder = resolved(&study("cylinder", MeshPreset::Coarse, None, |_| {}));
    let tree = compile(&cylinder);
    let named = |study: &ResolvedStudy, tree: &CaseTree, word: &str| {
        let found = backend().read_back(study, tree);
        assert!(!found.is_empty(), "nothing was reported for {word}");
        for d in &found {
            assert_eq!(d.code, "CFD-RT-001", "{d:?}");
        }
        assert!(
            found.iter().any(|d| d.setting.contains(word)),
            "{word} in {:?}",
            found.iter().map(|d| &d.setting).collect::<Vec<_>>()
        );
    };
    let bare = CaseTree {
        files: vec![tree.files[0].clone()],
    };
    named(&cylinder, &bare, "surface file");
    // The mean speed written where the peak belongs, a flat profile, a uniform outlet, another
    // body and another end time.
    named(
        &cylinder,
        &with_stream(&tree, |l| {
            command(l, 3)["faces"]["x_min"]["velocity"][0] = json!(1.0);
        }),
        "velocity",
    );
    named(
        &cylinder,
        &with_stream(&tree, |l| {
            command(l, 3)["faces"]["x_min"]["profile"] = json!("uniform");
        }),
        "profile",
    );
    named(
        &cylinder,
        &with_stream(&tree, |l| {
            command(l, 3)["faces"]["x_max"]["outflow"] = json!("uniform");
        }),
        "outlet form",
    );
    named(
        &cylinder,
        &with_stream(&tree, |l| {
            command(l, 3)["obstacles"] = json!(["Other"]);
        }),
        "name of the body",
    );
    named(
        &cylinder,
        &with_stream(&tree, |l| {
            command(l, 4)["procedure"]["end_time"] = json!(5.0);
        }),
        "end time",
    );

    let heated = resolved(&study("heated-cavity", MeshPreset::Coarse, None, |v| {
        v["mesh"].as_object_mut().unwrap().remove("grading");
    }));
    let tree = compile(&heated);
    named(
        &heated,
        &with_stream(&tree, |l| {
            command(l, 2)["thermal"]["gravity"][1] = json!(9.81);
        }),
        "gravity",
    );
    named(
        &heated,
        &with_stream(&tree, |l| {
            command(l, 2)["thermal"]
                .as_object_mut()
                .unwrap()
                .remove("temperature_zero");
        }),
        "temperature the lattice counts from",
    );
    named(
        &heated,
        &with_stream(&tree, |l| {
            command(l, 1)["thermal"]["conductivity"] = json!(1.0);
        }),
        "Prandtl",
    );
    named(
        &heated,
        &with_stream(&tree, |l| {
            command(l, 2)["thermal"]["faces"]["x_min"]["value"] = json!(305.0);
        }),
        "temperature of the wall",
    );
}
