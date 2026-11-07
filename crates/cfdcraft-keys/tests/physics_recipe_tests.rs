//! The physics, the thermal conditions, the inlet profile and the new monitors are part of the
//! recipes, the diff names them, and a study that states none of them keeps its recipe texts.

use cfdcraft_keys::{
    Change, PhysicsPart, StageKeys, StageRecipes, ToolIdentity, diff_mesh, diff_result,
};
use cfdcraft_model::{
    InitMode, InletProfile, MonitorField, ResolvedBuoyancy, ResolvedForce, ResolvedPhysics,
    ResolvedProbe, ResolvedStudy, ResolvedSurface, ResolvedThermal, ResolvedTime,
    ResolvedTurbulence, Study, SurfaceQuantity, TurbulenceModel, resolve,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

/// The mesh recipe text of the cavity before any of the new members existed.
const CAVITY_MESH_TEXT: &str = concat!(
    r#"{"cells":{"x":128,"y":128,"z":1},"#,
    r#""geometry":{"length_x":0.1,"length_y":0.1,"length_z":0.05,"open":false},"#,
    r#""patches":[{"faces":["YMax"],"kind":"MovingWall"},"#,
    r#"{"faces":["XMin","XMax","YMin"],"kind":"NoSlipWall"}],"#,
    r#""tool":{"dialect":"esi-v2512","image_digest":"sha256:aa","sidecar_lock":null,"writer_version":"0.1.0"}}"#,
);

/// The result recipe text of the cavity before any of the new members existed, without the mesh
/// key, which is a hash of the text above.
const CAVITY_RESULT_TEXT: &str = concat!(
    r#"{"boundaries":{"lid":{"faces":["YMax"],"kind":"MovingWall","velocity":0.1},"#,
    r#""walls":{"faces":["XMin","XMax","YMin"],"kind":"NoSlipWall"}},"#,
    r#""fluid":{"kinematic_viscosity":0.0001},"#,
    r#""monitors":{"centreline_u_sample_count":64,"probes":["#,
    r#"{"field":"velocity","x":0.05,"y":0.05},"#,
    r#"{"field":"velocity","x":0.025,"y":0.025},"#,
    r#"{"field":"velocity","x":0.07500000000000001,"y":0.025},"#,
    r#"{"field":"velocity","x":0.025,"y":0.07500000000000001},"#,
    r#"{"field":"velocity","x":0.07500000000000001,"y":0.07500000000000001}]},"#,
    r#""solver":{"convergence_p":0.000001,"convergence_u":0.000001,"end_time":5000,"write_interval":500},"#,
    r#""tool":{"dialect":"esi-v2512","image_digest":"sha256:aa","sidecar_lock":null,"writer_version":"0.1.0"}}"#,
);

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn recipes(resolved: &ResolvedStudy) -> StageRecipes {
    StageRecipes::from_resolved(resolved, &tool())
}

fn keys(resolved: &ResolvedStudy) -> StageKeys {
    recipes(resolved).keys()
}

fn cavity() -> ResolvedStudy {
    resolve(&Study::from_json(CAVITY).unwrap()).unwrap()
}

/// A closed cavity heated from the left and cooled from the right, resolved.
fn heated() -> ResolvedStudy {
    let mut v: Value = serde_json::from_str(CAVITY).unwrap();
    v["solver"]["solver"] = "FromPhysics".into();
    v["boundary_conditions"] = json!({
        "hot": {"faces": ["XMin"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": {"value": 300.5, "unit": "K"}}},
        "cold": {"faces": ["XMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "fixed_temperature", "value": {"value": 299.5, "unit": "K"}}},
        "adiabatic": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall",
            "thermal": {"kind": "adiabatic"}}
    });
    v["physics"] = json!({"buoyancy": {
        "gravity": 9.81,
        "beta": 0.003,
        "reference_temperature": {"value": 300.0, "unit": "K"},
        "prandtl": 0.71,
        "turbulent_prandtl": 0.85
    }});
    resolve(&Study::from_json(&v.to_string()).unwrap()).unwrap()
}

/// The heated cavity with every physics member and every new monitor stated.
fn full() -> ResolvedStudy {
    let mut r = heated();
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
    r.monitors.probes = vec![ResolvedProbe {
        label: "p".to_owned(),
        x: 0.05,
        y: 0.05,
        field: MonitorField::Pressure,
        boundary: Some("hot".to_owned()),
    }];
    r.monitors.surfaces = vec![ResolvedSurface {
        label: "s".to_owned(),
        boundary: "hot".to_owned(),
        quantity: SurfaceQuantity::HeatFlux,
    }];
    r.monitors.forces = vec![ResolvedForce {
        label: "a".to_owned(),
        boundary: "hot".to_owned(),
        reference_velocity: 1.0,
        reference_length: 0.1,
    }];
    r
}

fn physics_mut(r: &mut ResolvedStudy) -> &mut ResolvedPhysics {
    r.physics.as_mut().unwrap()
}

fn hot_at(r: &mut ResolvedStudy, thermal: ResolvedThermal) {
    r.boundary_conditions.get_mut("hot").unwrap().thermal = Some(thermal);
}

type Edit = fn(&mut ResolvedStudy);

#[test]
fn the_recipes_know_the_physics() {
    let base = full();
    let before = keys(&base);
    // Each of these changes the result and leaves the mesh as it was.
    let result_only: [(&str, Edit); 14] = [
        ("time mode", |r| physics_mut(r).time = ResolvedTime::Steady),
        ("time step", |r| {
            physics_mut(r).time = ResolvedTime::Transient { delta_t: 0.002 };
        }),
        ("turbulence model", |r| {
            physics_mut(r).turbulence.model = TurbulenceModel::RngKEpsilon;
        }),
        ("intensity", |r| {
            physics_mut(r).turbulence.intensity = Some(0.1)
        }),
        ("buoyancy beta", |r| {
            physics_mut(r).buoyancy.as_mut().unwrap().beta = 0.004;
        }),
        ("buoyancy off", |r| physics_mut(r).buoyancy = None),
        ("init", |r| physics_mut(r).init = InitMode::Uniform),
        ("thermal value", |r| {
            hot_at(r, ResolvedThermal::FixedTemperature { value: 301.5 });
        }),
        ("profile", |r| {
            r.boundary_conditions.get_mut("hot").unwrap().profile = Some(InletProfile::Parabolic);
        }),
        ("force reference velocity", |r| {
            r.monitors.forces[0].reference_velocity = 2.0;
        }),
        ("force reference length", |r| {
            r.monitors.forces[0].reference_length = 0.2;
        }),
        ("force boundary", |r| {
            r.monitors.forces[0].boundary = "cold".to_owned();
        }),
        ("heat flux boundary", |r| {
            r.monitors.surfaces[0].boundary = "cold".to_owned();
        }),
        ("probe boundary", |r| {
            r.monitors.probes[0].boundary = Some("cold".to_owned());
        }),
    ];
    for (name, edit) in result_only {
        let mut edited = full();
        edit(&mut edited);
        let after = keys(&edited);
        assert_eq!(before.mesh, after.mesh, "{name}: the mesh key moved");
        assert_ne!(before.result, after.result, "{name}: the result key stood");
    }
    // A label is for people: renaming a monitor changes neither key.
    let mut renamed = full();
    renamed.monitors.forces[0].label = "b".to_owned();
    renamed.monitors.surfaces[0].label = "t".to_owned();
    renamed.monitors.probes[0].label = "q".to_owned();
    assert_eq!(keys(&renamed), before);
    // The mesh sees which walls are isothermal and in which order, not the temperature.
    let mut adiabatic = full();
    hot_at(&mut adiabatic, ResolvedThermal::Adiabatic);
    assert_ne!(keys(&adiabatic).mesh, before.mesh);
    let mut swapped = full();
    hot_at(
        &mut swapped,
        ResolvedThermal::FixedTemperature { value: 298.0 },
    );
    assert_ne!(keys(&swapped).mesh, before.mesh);
    let mut warmer = full();
    hot_at(
        &mut warmer,
        ResolvedThermal::FixedTemperature { value: 310.0 },
    );
    assert_eq!(keys(&warmer).mesh, before.mesh);
}

#[test]
fn a_study_without_the_members_has_the_same_recipe_texts() {
    let recipes = recipes(&cavity());
    assert_eq!(
        serde_jcs::to_string(&recipes.mesh).unwrap(),
        CAVITY_MESH_TEXT
    );
    let mut result = serde_json::to_value(&recipes.result).unwrap();
    assert!(result.as_object_mut().unwrap().remove("mesh_key").is_some());
    let text = serde_jcs::to_string(&result).unwrap();
    assert_eq!(text, CAVITY_RESULT_TEXT);
    for word in [
        "physics",
        "thermal",
        "profile",
        "forces",
        "\"on\"",
        "boundary\"",
    ] {
        assert!(!text.contains(word), "{word}");
    }
}

fn changes(edit: Edit) -> Vec<Change> {
    let before = recipes(&full());
    let mut edited = full();
    edit(&mut edited);
    diff_result(&before.result, &recipes(&edited).result)
}

fn physics(what: PhysicsPart, from: &str, to: &str) -> Change {
    Change::Physics {
        what,
        from: from.to_owned(),
        to: to.to_owned(),
    }
}

#[test]
fn the_diff_and_reasons_name_the_physics() {
    let r = recipes(&full());
    assert!(diff_result(&r.result, &r.result).is_empty());
    assert!(diff_mesh(&r.mesh, &r.mesh).is_empty());
    assert_eq!(
        changes(|r| physics_mut(r).turbulence.model = TurbulenceModel::Laminar),
        [physics(PhysicsPart::Turbulence, "k_omega_sst", "laminar")]
    );
    assert_eq!(
        changes(|r| physics_mut(r).time = ResolvedTime::Transient { delta_t: 0.002 }),
        [physics(PhysicsPart::DeltaT, "0.001", "0.002")]
    );
    assert_eq!(
        changes(|r| physics_mut(r).time = ResolvedTime::Steady),
        [
            physics(PhysicsPart::Time, "transient", "steady"),
            physics(PhysicsPart::DeltaT, "0.001", "none"),
        ]
    );
    assert_eq!(
        changes(|r| physics_mut(r).turbulence.viscosity_ratio = None),
        [physics(PhysicsPart::ViscosityRatio, "10", "none")]
    );
    assert_eq!(
        changes(|r| physics_mut(r).init = InitMode::Uniform),
        [physics(PhysicsPart::Init, "potential", "uniform")]
    );
    let off = changes(|r| physics_mut(r).buoyancy = None);
    assert_eq!(off.len(), 1);
    let Change::Physics { what, from, to } = &off[0] else {
        panic!("{off:?}");
    };
    assert_eq!((*what, to.as_str()), (PhysicsPart::Buoyancy, "none"));
    assert!(
        from.contains("gravity 9.81") && from.contains("beta 0.003"),
        "{from}"
    );
    // A study that states only buoyancy is a steady laminar run.
    let plain = recipes(&heated());
    let found = diff_result(&plain.result, &r.result);
    assert!(found.contains(&physics(PhysicsPart::Turbulence, "laminar", "k_omega_sst")));
    assert!(found.contains(&physics(PhysicsPart::Time, "steady", "transient")));
}

#[test]
fn the_diff_names_thermal_profile_and_monitor_changes() {
    assert_eq!(
        changes(|r| hot_at(r, ResolvedThermal::FixedTemperature { value: 301.5 })),
        [Change::Thermal {
            boundary: "hot".to_owned(),
            from: Some("fixed 300.5 K".to_owned()),
            to: Some("fixed 301.5 K".to_owned()),
        }]
    );
    assert_eq!(
        changes(|r| {
            r.boundary_conditions.get_mut("hot").unwrap().profile = Some(InletProfile::Parabolic);
        }),
        [Change::InletProfile {
            boundary: "hot".to_owned(),
            from: None,
            to: Some("parabolic".to_owned()),
        }]
    );
    assert_eq!(
        changes(|r| r.monitors.forces[0].reference_length = 0.2),
        [Change::Monitor {
            id: "force_1".to_owned(),
            from: Some("forces on hot, speed 1 m/s, length 0.1 m".to_owned()),
            to: Some("forces on hot, speed 1 m/s, length 0.2 m".to_owned()),
        }]
    );
    assert_eq!(
        changes(|r| r.monitors.surfaces[0].boundary = "cold".to_owned()),
        [Change::Monitor {
            id: "surface_1".to_owned(),
            from: Some("heat flux on hot".to_owned()),
            to: Some("heat flux on cold".to_owned()),
        }]
    );
    assert_eq!(
        changes(|r| r.monitors.probes[0].boundary = Some("cold".to_owned())),
        [Change::Monitor {
            id: "probe_1".to_owned(),
            from: Some("x 0.05 m, y 0.05 m, pressure, on hot".to_owned()),
            to: Some("x 0.05 m, y 0.05 m, pressure, on cold".to_owned()),
        }]
    );
    // An isothermal wall that becomes adiabatic changes the patch layout.
    let before = recipes(&full());
    let mut edited = full();
    hot_at(&mut edited, ResolvedThermal::Adiabatic);
    let after = recipes(&edited);
    assert_eq!(diff_mesh(&before.mesh, &after.mesh), [Change::Layout]);
    assert_eq!(
        diff_result(&before.result, &after.result),
        [
            Change::MeshChanged,
            Change::Thermal {
                boundary: "hot".to_owned(),
                from: Some("fixed 300.5 K".to_owned()),
                to: Some("adiabatic".to_owned()),
            },
        ]
    );
}
