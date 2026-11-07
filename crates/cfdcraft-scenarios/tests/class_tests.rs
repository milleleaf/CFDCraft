use std::collections::HashSet;

use cfdcraft_keys::{StageKeys, StageRecipes, ToolIdentity};
use cfdcraft_model::{
    InitMode, InletProfile, ResolvedBuoyancy, ResolvedGrading, ResolvedPhysics, ResolvedShape,
    ResolvedStudy, ResolvedThermal, ResolvedTime, ResolvedTurbulence, Study, TurbulenceModel,
    resolve,
};
use cfdcraft_scenarios::{
    KNOWN_SETTINGS, Override, Scenario, SettingClass, apply_overrides, classify, classify_setting,
    effective_study,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn cavity_json() -> Value {
    serde_json::from_str(CAVITY).unwrap()
}

fn set(setting: &str, value: Value) -> Override {
    Override::Set {
        setting: setting.to_string(),
        value,
    }
}

fn remove(setting: &str) -> Override {
    Override::Remove {
        setting: setting.to_string(),
    }
}

fn raw(file: &str) -> Value {
    json!({"backend": "esi", "file": file, "entry": "scale", "value": "1"})
}

/// The cavity with one raw override labelled `x` in the given file.
fn with_raw(file: &str) -> Value {
    let mut study = cavity_json();
    study["raw_overrides"] = json!({ "x": raw(file) });
    study
}

const MESH_FILE: &str = "system/blockMeshDict";
const OTHER_FILE: &str = "system/fvSolution";
const LID: &str = "/boundary_conditions/lid/velocity/value";

#[test]
fn the_table_has_at_least_21_rows_and_no_pointer_twice() {
    assert!(KNOWN_SETTINGS.len() >= 21);
    let mut seen = HashSet::new();
    for (pointer, class) in KNOWN_SETTINGS {
        assert!(seen.insert(*pointer), "{pointer} is listed twice");
        let concrete = pointer.replace("<label>", "x").replace("<id>", "x");
        assert_eq!(
            classify_setting(&concrete),
            Some(*class),
            "{pointer} does not read back as its own row"
        );
    }
}

#[test]
fn a_lid_speed_is_setting_only_and_a_cell_count_is_geometry() {
    assert_eq!(classify_setting(LID), Some(SettingClass::Setting));
    assert_eq!(
        classify_setting("/mesh/cells_x"),
        Some(SettingClass::Geometry)
    );
    assert_eq!(
        classify_setting("/boundary_conditions/walls/faces"),
        Some(SettingClass::Geometry)
    );
}

#[test]
fn placeholders_match_one_segment_only() {
    assert_eq!(classify_setting("/boundary_conditions/a/b/kind"), None);
    assert_eq!(classify_setting("/boundary_conditions//kind"), None);
    assert_eq!(classify_setting("/domain/length_x/value/extra"), None);
}

#[test]
fn a_whole_boundary_entry_is_geometry() {
    assert_eq!(
        classify_setting("/boundary_conditions/lid"),
        Some(SettingClass::Geometry)
    );
    let parent = cavity_json();
    let entry = json!({"faces": ["YMax"], "kind": "MovingWall"});
    assert_eq!(
        classify(&parent, &[set("/boundary_conditions/lid", entry)]),
        SettingClass::Geometry
    );
    assert_eq!(
        classify(&parent, &[remove("/boundary_conditions/lid")]),
        SettingClass::Geometry
    );
}

#[test]
fn an_unknown_setting_has_no_class_and_classify_reads_it_as_geometry() {
    assert_eq!(classify_setting("/fluid/density"), None);
    assert_eq!(
        classify(&cavity_json(), &[set("/fluid/density", json!(1))]),
        SettingClass::Geometry
    );
}

#[test]
fn a_variant_is_geometry_when_any_override_is() {
    let parent = cavity_json();
    let lid = set(LID, json!(0.2));
    assert_eq!(classify(&parent, &[]), SettingClass::Setting);
    assert_eq!(
        classify(&parent, std::slice::from_ref(&lid)),
        SettingClass::Setting
    );
    let cells = set("/mesh/cells_x", json!(256));
    assert_eq!(classify(&parent, &[lid, cells]), SettingClass::Geometry);
}

#[test]
fn a_raw_override_is_geometry_only_for_a_mesh_dictionary() {
    let parent = cavity_json();
    assert_eq!(
        classify(&parent, &[set("/raw_overrides/x", raw(MESH_FILE))]),
        SettingClass::Geometry
    );
    assert_eq!(
        classify(&parent, &[set("/raw_overrides/x", raw(OTHER_FILE))]),
        SettingClass::Setting
    );
    assert_eq!(
        classify(
            &parent,
            &[set("/raw_overrides/x", json!("not an override"))]
        ),
        SettingClass::Geometry
    );
    // A member takes the class of the entry the parent holds.
    let member = [set("/raw_overrides/x/value", json!("2"))];
    assert_eq!(
        classify(&with_raw(MESH_FILE), &member),
        SettingClass::Geometry
    );
    assert_eq!(
        classify(&with_raw(OTHER_FILE), &member),
        SettingClass::Setting
    );
    // Pointing the entry at a mesh dictionary makes it geometry.
    let to_mesh = [set("/raw_overrides/x/file", json!(MESH_FILE))];
    assert_eq!(
        classify(&with_raw(OTHER_FILE), &to_mesh),
        SettingClass::Geometry
    );
    // Rewriting a mesh entry as another kind of entry still moves the mesh.
    let rewrite = [set("/raw_overrides/x", raw(OTHER_FILE))];
    assert_eq!(
        classify(&with_raw(MESH_FILE), &rewrite),
        SettingClass::Geometry
    );
}

#[test]
fn removing_a_raw_override_takes_the_class_of_the_one_removed() {
    let gone = [remove("/raw_overrides/x")];
    assert_eq!(
        classify(&with_raw(MESH_FILE), &gone),
        SettingClass::Geometry
    );
    assert_eq!(
        classify(&with_raw(OTHER_FILE), &gone),
        SettingClass::Setting
    );
    assert_eq!(classify(&cavity_json(), &gone), SettingClass::Setting);
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn keys_of(resolved: &ResolvedStudy) -> StageKeys {
    StageRecipes::from_resolved(resolved, &tool()).keys()
}

fn resolved_of(study: Study) -> ResolvedStudy {
    resolve(&study).unwrap()
}

fn cavity() -> ResolvedStudy {
    resolved_of(Study::from_json(CAVITY).unwrap())
}

/// The keys of the cavity after the overrides are applied to it.
fn keys_after(overrides: &[Override]) -> StageKeys {
    let value = apply_overrides(&cavity_json(), overrides).unwrap();
    keys_of(&resolved_of(serde_json::from_value(value).unwrap()))
}

/// How a sample changes the cavity.
enum How {
    /// An override on the study.
    Override(Value),
    /// A direct change to the resolved study, for the two settings that cannot be changed alone
    /// on a valid study.
    Resolved(fn(&mut ResolvedStudy)),
    /// A direct change to a resolved study that first has a base applied, for a setting that
    /// only means something once its parent exists. The keys before the change are the base's.
    ResolvedOn(fn(&mut ResolvedStudy), fn(&mut ResolvedStudy)),
}

struct Sample {
    /// The row of the table the sample stands for.
    row: String,
    /// The setting with its placeholders filled in.
    setting: String,
    how: How,
    /// False when the sample writes the value the study already has.
    changes: bool,
}

fn sample(row: &str, setting: &str, value: Value) -> Sample {
    Sample {
        row: row.to_string(),
        setting: setting.to_string(),
        how: How::Override(value),
        changes: true,
    }
}

fn samples() -> Vec<Sample> {
    let mut out = Vec::new();
    for axis in ["x", "y", "z"] {
        let row = format!("/domain/length_{axis}");
        out.push(sample(&row, &row, json!({"value": 0.2, "unit": "m"})));
        let value = format!("{row}/value");
        out.push(sample(&value, &value, json!(0.2)));
        let unit = format!("{row}/unit");
        out.push(sample(&unit, &unit, json!("mm")));
        let cells = format!("/mesh/cells_{axis}");
        let count = if axis == "z" { 2 } else { 256 };
        out.push(sample(&cells, &cells, json!(count)));
    }
    out.push(sample("/mesh/level", "/mesh/level", json!(2)));
    out.push(sample("/mesh/grading", "/mesh/grading", json!({"x": 1.1})));
    out.push(Sample {
        row: "/mesh/grading/x".to_string(),
        setting: "/mesh/grading/x".to_string(),
        how: How::Resolved(|r| {
            r.mesh.grading = Some(ResolvedGrading {
                x: Some(1.1),
                y: None,
            })
        }),
        changes: true,
    });
    out.push(Sample {
        row: "/mesh/grading/y".to_string(),
        setting: "/mesh/grading/y".to_string(),
        how: How::Resolved(|r| {
            r.mesh.grading = Some(ResolvedGrading {
                x: None,
                y: Some(1.1),
            })
        }),
        changes: true,
    });
    // The cavity is a plain box, so a shape cannot be written onto it as a valid study; each
    // sample puts a shape on the resolved study, which is what the mesh key reads.
    type Shaper = fn(&mut ResolvedStudy);
    let shapes: [(&str, Shaper); 6] = [
        ("step_height", |r| {
            r.domain.shape = Some(ResolvedShape::BackwardStep {
                step_height: 0.01,
                upstream_length: 0.05,
                startup_length: 0.0,
            })
        }),
        ("upstream_length", |r| {
            r.domain.shape = Some(ResolvedShape::BackwardStep {
                step_height: 0.02,
                upstream_length: 0.06,
                startup_length: 0.0,
            })
        }),
        ("startup_length", |r| {
            r.domain.shape = Some(ResolvedShape::BackwardStep {
                step_height: 0.02,
                upstream_length: 0.05,
                startup_length: 0.01,
            })
        }),
        ("diameter", |r| {
            r.domain.shape = Some(ResolvedShape::CylinderChannel {
                diameter: 0.01,
                centre_x: 0.05,
                centre_y: 0.05,
            })
        }),
        ("centre_x", |r| {
            r.domain.shape = Some(ResolvedShape::CylinderChannel {
                diameter: 0.02,
                centre_x: 0.04,
                centre_y: 0.05,
            })
        }),
        ("centre_y", |r| {
            r.domain.shape = Some(ResolvedShape::CylinderChannel {
                diameter: 0.02,
                centre_x: 0.05,
                centre_y: 0.04,
            })
        }),
    ];
    out.push(Sample {
        row: "/domain/shape".to_string(),
        setting: "/domain/shape".to_string(),
        how: How::Resolved(|r| {
            r.domain.shape = Some(ResolvedShape::BackwardStep {
                step_height: 0.02,
                upstream_length: 0.05,
                startup_length: 0.0,
            })
        }),
        changes: true,
    });
    for (member, edit) in shapes {
        for suffix in ["", "/value", "/unit"] {
            let setting = format!("/domain/shape/{member}{suffix}");
            out.push(Sample {
                row: setting.clone(),
                setting,
                how: How::Resolved(edit),
                changes: true,
            });
        }
    }
    out.push(Sample {
        row: "/domain/open".to_string(),
        setting: "/domain/open".to_string(),
        how: How::Resolved(|r| r.domain.open = !r.domain.open),
        changes: true,
    });
    out.push(sample(
        "/boundary_conditions/<label>/kind",
        "/boundary_conditions/walls/kind",
        json!("SlipWall"),
    ));
    out.push(Sample {
        row: "/boundary_conditions/<label>/faces".to_string(),
        setting: "/boundary_conditions/walls/faces".to_string(),
        how: How::Resolved(|r| {
            r.boundary_conditions.get_mut("walls").unwrap().faces.pop();
        }),
        changes: true,
    });
    out.push(sample(
        "/boundary_conditions/<label>/velocity",
        "/boundary_conditions/lid/velocity",
        json!({"value": 0.2, "unit": "m/s"}),
    ));
    out.push(sample(
        "/boundary_conditions/<label>/velocity/value",
        LID,
        json!(0.2),
    ));
    out.push(sample(
        "/fluid/kinematic_viscosity",
        "/fluid/kinematic_viscosity",
        json!({"value": 0.0002, "unit": "m2/s"}),
    ));
    out.push(sample(
        "/fluid/kinematic_viscosity/value",
        "/fluid/kinematic_viscosity/value",
        json!(0.0002),
    ));
    out.push(Sample {
        changes: false,
        ..sample("/solver/solver", "/solver/solver", json!("SimpleLaminar"))
    });
    // The cavity writes every 500 s up to 5000 s; the new values keep the interval a divisor.
    out.push(sample(
        "/solver/end_time",
        "/solver/end_time",
        json!({"value": 10000, "unit": "s"}),
    ));
    out.push(sample(
        "/solver/end_time/value",
        "/solver/end_time/value",
        json!(10000),
    ));
    out.push(sample(
        "/solver/write_interval",
        "/solver/write_interval",
        json!({"value": 250, "unit": "s"}),
    ));
    out.push(sample(
        "/solver/write_interval/value",
        "/solver/write_interval/value",
        json!(250),
    ));
    out.push(sample(
        "/solver/convergence",
        "/solver/convergence",
        json!({"p": 1e-5, "U": 1e-5}),
    ));
    out.push(sample(
        "/solver/convergence/p",
        "/solver/convergence/p",
        json!(1e-5),
    ));
    out.push(sample(
        "/solver/convergence/U",
        "/solver/convergence/U",
        json!(1e-5),
    ));
    out.push(sample(
        "/monitors/centreline_u_sample_count",
        "/monitors/centreline_u_sample_count",
        json!(32),
    ));
    thermal_and_profile_samples(&mut out);
    physics_samples(&mut out);
    out
}

fn resolved_sample(row: &str, setting: &str, how: How) -> Sample {
    Sample {
        row: row.to_string(),
        setting: setting.to_string(),
        how,
        changes: true,
    }
}

/// The cavity with the walls held at 300 K.
fn walls_at_300(r: &mut ResolvedStudy) {
    r.boundary_conditions.get_mut("walls").unwrap().thermal =
        Some(ResolvedThermal::FixedTemperature { value: 300.0 });
}

fn thermal_and_profile_samples(out: &mut Vec<Sample>) {
    out.push(resolved_sample(
        "/boundary_conditions/<label>/thermal",
        "/boundary_conditions/walls/thermal",
        How::Resolved(walls_at_300),
    ));
    // Only the temperature changes, and the walls stay the only fixed temperature, so the mesh
    // keeps its patches.
    for suffix in ["", "/value", "/unit"] {
        let row = format!("/boundary_conditions/<label>/thermal/value{suffix}");
        let setting = format!("/boundary_conditions/walls/thermal/value{suffix}");
        out.push(resolved_sample(
            &row,
            &setting,
            How::ResolvedOn(walls_at_300, |r| {
                r.boundary_conditions.get_mut("walls").unwrap().thermal =
                    Some(ResolvedThermal::FixedTemperature { value: 310.0 });
            }),
        ));
    }
    out.push(resolved_sample(
        "/boundary_conditions/<label>/profile",
        "/boundary_conditions/lid/profile",
        How::Resolved(|r| {
            r.boundary_conditions.get_mut("lid").unwrap().profile = Some(InletProfile::Parabolic);
        }),
    ));
}

/// The cavity with every physics member stated.
fn full_physics(r: &mut ResolvedStudy) {
    r.physics = Some(ResolvedPhysics {
        time: ResolvedTime::Transient { delta_t: 0.001 },
        turbulence: ResolvedTurbulence {
            model: TurbulenceModel::KOmegaSst,
            intensity: Some(0.05),
            viscosity_ratio: Some(10.0),
            reference_velocity: Some(1.0),
        },
        buoyancy: Some(ResolvedBuoyancy {
            gravity: 9.81,
            beta: 0.003,
            reference_temperature: 300.0,
            prandtl: 0.71,
            turbulent_prandtl: 0.85,
        }),
        init: InitMode::Potential,
    });
}

fn physics_mut(r: &mut ResolvedStudy) -> &mut ResolvedPhysics {
    r.physics.as_mut().unwrap()
}

fn physics_samples(out: &mut Vec<Sample>) {
    // The cavity's solver kind is SimpleLaminar, which refuses any physics as a valid study, so
    // the whole record is put on the resolved study like the members below.
    out.push(resolved_sample(
        "/physics",
        "/physics",
        How::Resolved(full_physics),
    ));
    type Edit = fn(&mut ResolvedStudy);
    let edits: [(&[&str], Edit); 11] = [
        (&["/physics/time", "/physics/time/mode"], |r| {
            physics_mut(r).time = ResolvedTime::Steady;
        }),
        (
            &[
                "/physics/time/delta_t",
                "/physics/time/delta_t/value",
                "/physics/time/delta_t/unit",
            ],
            |r| physics_mut(r).time = ResolvedTime::Transient { delta_t: 0.002 },
        ),
        (&["/physics/turbulence"], |r| {
            physics_mut(r).turbulence = ResolvedTurbulence {
                model: TurbulenceModel::Laminar,
                intensity: None,
                viscosity_ratio: None,
                reference_velocity: None,
            };
        }),
        (&["/physics/turbulence/model"], |r| {
            physics_mut(r).turbulence.model = TurbulenceModel::RealizableKe;
        }),
        (&["/physics/turbulence/intensity"], |r| {
            physics_mut(r).turbulence.intensity = Some(0.1);
        }),
        (&["/physics/turbulence/viscosity_ratio"], |r| {
            physics_mut(r).turbulence.viscosity_ratio = Some(5.0);
        }),
        (
            &[
                "/physics/turbulence/reference_velocity",
                "/physics/turbulence/reference_velocity/value",
                "/physics/turbulence/reference_velocity/unit",
            ],
            |r| physics_mut(r).turbulence.reference_velocity = Some(2.0),
        ),
        (&["/physics/buoyancy"], |r| physics_mut(r).buoyancy = None),
        (&["/physics/buoyancy/gravity"], |r| {
            physics_mut(r).buoyancy.as_mut().unwrap().gravity = 1.62;
        }),
        (&["/physics/buoyancy/beta"], |r| {
            physics_mut(r).buoyancy.as_mut().unwrap().beta = 0.004;
        }),
        (&["/physics/init"], |r| {
            physics_mut(r).init = InitMode::Uniform
        }),
    ];
    for (rows, edit) in edits {
        for row in rows {
            out.push(resolved_sample(
                row,
                row,
                How::ResolvedOn(full_physics, edit),
            ));
        }
    }
    let buoyancy: [(&[&str], Edit); 3] = [
        (
            &[
                "/physics/buoyancy/reference_temperature",
                "/physics/buoyancy/reference_temperature/value",
                "/physics/buoyancy/reference_temperature/unit",
            ],
            |r| {
                physics_mut(r)
                    .buoyancy
                    .as_mut()
                    .unwrap()
                    .reference_temperature = 310.0
            },
        ),
        (&["/physics/buoyancy/prandtl"], |r| {
            physics_mut(r).buoyancy.as_mut().unwrap().prandtl = 0.7;
        }),
        (&["/physics/buoyancy/turbulent_prandtl"], |r| {
            physics_mut(r).buoyancy.as_mut().unwrap().turbulent_prandtl = 0.9;
        }),
    ];
    for (rows, edit) in buoyancy {
        for row in rows {
            out.push(resolved_sample(
                row,
                row,
                How::ResolvedOn(full_physics, edit),
            ));
        }
    }
}

#[test]
fn every_known_setting_has_a_sample_and_every_sample_a_row() {
    let rows: Vec<&str> = KNOWN_SETTINGS.iter().map(|(row, _)| *row).collect();
    let sampled: Vec<String> = samples().into_iter().map(|s| s.row).collect();
    let unique: HashSet<&str> = sampled.iter().map(String::as_str).collect();
    assert_eq!(sampled.len(), unique.len(), "a row is sampled twice");
    for row in &rows {
        assert!(unique.contains(row), "{row} has no sample");
    }
    for row in &sampled {
        assert!(rows.contains(&row.as_str()), "{row} is not a row");
    }
    assert_eq!(rows.len(), sampled.len());
}

#[test]
fn the_mesh_key_moves_exactly_for_geometry_class_settings() {
    let plain = keys_of(&cavity());
    for sample in samples() {
        let (_, class) = KNOWN_SETTINGS
            .iter()
            .find(|(row, _)| *row == sample.row)
            .unwrap();
        assert_eq!(
            classify_setting(&sample.setting),
            Some(*class),
            "{} is not classed as its row",
            sample.setting
        );
        let (before, after) = match &sample.how {
            How::Override(value) => {
                let item = set(&sample.setting, value.clone());
                assert_eq!(
                    classify(&cavity_json(), std::slice::from_ref(&item)),
                    *class
                );
                (plain, keys_after(&[item]))
            }
            How::Resolved(change) => {
                let mut resolved = cavity();
                change(&mut resolved);
                (plain, keys_of(&resolved))
            }
            How::ResolvedOn(base, change) => {
                let mut resolved = cavity();
                base(&mut resolved);
                let before = keys_of(&resolved);
                change(&mut resolved);
                (before, keys_of(&resolved))
            }
        };
        let mesh_moved = before.mesh != after.mesh;
        assert_eq!(
            mesh_moved,
            *class == SettingClass::Geometry,
            "{}: mesh key moved is {mesh_moved}",
            sample.row
        );
        assert_eq!(
            before.result != after.result,
            sample.changes,
            "{}: result key",
            sample.row
        );
    }
}

fn scenario(id: &str, parent: &str, overrides: Vec<Override>) -> Scenario {
    Scenario {
        id: id.to_string(),
        parent: Some(parent.to_string()),
        name: None,
        pinned: false,
        notes: None,
        overrides,
        origin: None,
    }
}

#[test]
fn a_setting_only_variant_keeps_its_parents_mesh_key() {
    let base = Study::from_json(CAVITY).unwrap();
    let setting_only = vec![
        set(LID, json!(0.05)),
        set("/fluid/kinematic_viscosity/value", json!(0.0002)),
    ];
    assert_eq!(
        classify(&cavity_json(), &setting_only),
        SettingClass::Setting
    );
    let list = [
        scenario("slow", "base", setting_only),
        scenario("finer", "slow", vec![set("/mesh/cells_x", json!(256))]),
    ];
    let keys = |id: &str| keys_of(&resolved_of(effective_study(&base, &list, id).unwrap()));
    let parent = keys_of(&cavity());
    let slow = keys("slow");
    assert_eq!(slow.mesh, parent.mesh);
    assert_ne!(slow.result, parent.result);
    let finer = keys("finer");
    assert_ne!(finer.mesh, slow.mesh);
    assert_eq!(
        classify(&cavity_json(), &list[1].overrides),
        SettingClass::Geometry
    );
}

#[test]
fn a_whole_entry_that_keeps_kind_and_faces_keeps_the_mesh_although_it_is_geometry_class() {
    let entry = json!({
        "faces": ["YMax"],
        "kind": "MovingWall",
        "velocity": {"value": 0.3, "unit": "m/s"}
    });
    let item = set("/boundary_conditions/lid", entry);
    assert_eq!(
        classify(&cavity_json(), std::slice::from_ref(&item)),
        SettingClass::Geometry
    );
    let before = keys_of(&cavity());
    let after = keys_after(&[item]);
    assert_eq!(before.mesh, after.mesh);
    assert_ne!(before.result, after.result);
}

#[test]
fn mesh_dictionary_raw_overrides_move_the_mesh_key_and_others_do_not() {
    let before = keys_of(&cavity());
    let on_mesh = set("/raw_overrides/blk", raw(MESH_FILE));
    assert_eq!(
        classify(&cavity_json(), std::slice::from_ref(&on_mesh)),
        SettingClass::Geometry
    );
    let with_mesh = keys_after(std::slice::from_ref(&on_mesh));
    assert_ne!(with_mesh.mesh, before.mesh);
    let on_solution = set("/raw_overrides/sol", raw(OTHER_FILE));
    assert_eq!(
        classify(&cavity_json(), std::slice::from_ref(&on_solution)),
        SettingClass::Setting
    );
    let with_solution = keys_after(&[on_solution]);
    assert_eq!(with_solution.mesh, before.mesh);
    assert_ne!(with_solution.result, before.result);
    // Taking the mesh entry away again returns the original mesh.
    let held = apply_overrides(&cavity_json(), &[on_mesh]).unwrap();
    let gone = [remove("/raw_overrides/blk")];
    assert_eq!(classify(&held, &gone), SettingClass::Geometry);
    let back = apply_overrides(&held, &gone).unwrap();
    let back = keys_of(&resolved_of(serde_json::from_value(back).unwrap()));
    assert_eq!(back.mesh, before.mesh);
}
