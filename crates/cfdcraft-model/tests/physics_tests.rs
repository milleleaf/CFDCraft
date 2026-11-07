use cfdcraft_model::{
    InitMode, ResolvedTime, Study, TimeMode, TurbulenceModel, canonical_hash, inlet_turbulence,
    resolve,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("fixtures/channel_v1.json");

/// The canonical text of the cavity as it was before the physics existed.
const CAVITY_TEXT: &str = concat!(
    r#"{"boundary_conditions":{"lid":{"faces":["YMax"],"kind":"MovingWall","velocity":0.1},"#,
    r#""walls":{"faces":["XMin","XMax","YMin"],"kind":"NoSlipWall"}},"#,
    r#""domain":{"length_x":0.1,"length_y":0.1,"length_z":0.05,"open":false},"#,
    r#""fluid":{"kinematic_viscosity":0.0001},"#,
    r#""mesh":{"cells_x":128,"cells_y":128,"cells_z":1},"#,
    r#""monitors":{"centreline_u_sample_count":64},"schema_version":1,"#,
    r#""solver":{"convergence":{"U":0.000001,"p":0.000001},"end_time":5000,"write_interval":500}}"#,
);

const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

/// The cavity with the given edit applied to its JSON.
fn cavity_with(edit: impl FnOnce(&mut Value)) -> Study {
    let mut value: Value = serde_json::from_str(CAVITY).unwrap();
    edit(&mut value);
    Study::from_json(&value.to_string()).unwrap()
}

fn text(study: &Study) -> String {
    serde_jcs::to_string(&resolve(study).unwrap()).unwrap()
}

/// Within a relative tolerance.
fn close(found: f64, wanted: f64, relative: f64) -> bool {
    (found - wanted).abs() <= relative * wanted.abs()
}

#[test]
fn a_study_without_physics_keeps_its_hash() {
    let plain = cavity_with(|_| {});
    assert_eq!(text(&plain), CAVITY_TEXT);
    assert_eq!(canonical_hash(&resolve(&plain).unwrap()), CAVITY_HASH);
    let defaults = [
        json!({}),
        json!({
            "time": {"mode": "steady"},
            "turbulence": {"model": "laminar"},
            "init": "uniform"
        }),
    ];
    for physics in defaults {
        let study = cavity_with(|v| v["physics"] = physics);
        let resolved = resolve(&study).unwrap();
        assert!(resolved.physics.is_none());
        assert_eq!(text(&study), CAVITY_TEXT);
        assert_eq!(canonical_hash(&resolved), CAVITY_HASH);
    }
    assert!(!CAVITY_TEXT.contains("physics"));
}

#[test]
fn a_transient_study_resolves() {
    let transient = |level: Option<u32>| {
        cavity_with(|v| {
            v["solver"]["solver"] = "FromPhysics".into();
            v["solver"]["end_time"] = json!({"value": 10, "unit": "s"});
            v["solver"]["write_interval"] = json!({"value": 1, "unit": "s"});
            v["physics"] = json!({
                "time": {"mode": "transient", "delta_t": {"value": 0.001, "unit": "s"}}
            });
            if let Some(level) = level {
                v["mesh"]["level"] = level.into();
            }
        })
    };
    let resolved = resolve(&transient(None)).unwrap();
    assert_eq!(
        resolved.physics.as_ref().map(|p| p.time),
        Some(ResolvedTime::Transient { delta_t: 0.001 })
    );
    assert!(text(&transient(None)).contains(r#""time":{"delta_t":0.001,"mode":"transient"}"#));
    for (level, wanted) in [(0, 0.001), (1, 0.0005), (2, 0.00025)] {
        let resolved = resolve(&transient(Some(level))).unwrap();
        let step = resolved.effective_delta_t().unwrap();
        assert!(close(step, wanted, 1e-12), "level {level}: {step}");
    }
    assert_eq!(
        resolve(&cavity_with(|_| {})).unwrap().effective_delta_t(),
        None
    );
}

#[test]
fn a_turbulence_model_resolves() {
    let study = |model: &str| {
        cavity_with(|v| {
            v["solver"]["solver"] = "FromPhysics".into();
            v["physics"] = json!({
                "turbulence": {"model": model, "intensity": 0.00061, "viscosity_ratio": 0.0003846}
            });
        })
    };
    for (word, model) in [
        ("laminar", TurbulenceModel::Laminar),
        ("k_omega_sst", TurbulenceModel::KOmegaSst),
        ("realizable_ke", TurbulenceModel::RealizableKe),
        ("rng_k_epsilon", TurbulenceModel::RngKEpsilon),
    ] {
        let resolved = resolve(&study(word)).unwrap();
        let turbulence = resolved.physics.unwrap().turbulence;
        assert_eq!(turbulence.model, model);
        assert_eq!(turbulence.intensity, Some(0.00061));
        assert_eq!(turbulence.viscosity_ratio, Some(0.0003846));
        assert_eq!(model.word(), word);
        assert_eq!(serde_json::to_value(model).unwrap(), json!(word));
    }
    let mut unknown: Value = serde_json::from_str(CAVITY).unwrap();
    unknown["physics"] = json!({"turbulence": {"model": "k_epsilon"}});
    assert!(Study::from_json(&unknown.to_string()).is_err());
}

#[test]
fn buoyancy_and_init_resolve() {
    let buoyancy = json!({
        "gravity": 9.81,
        "beta": 0.003,
        "reference_temperature": {"value": 300, "unit": "K"},
        "prandtl": 0.71,
        "turbulent_prandtl": 0.85
    });
    // A closed cavity heated from the left and cooled from the right.
    let heated = cavity_with(|v| {
        v["solver"]["solver"] = "FromPhysics".into();
        v["boundary_conditions"] = json!({
            "hot": {"faces": ["XMin"], "kind": "NoSlipWall",
                "thermal": {"kind": "fixed_temperature", "value": {"value": 301, "unit": "K"}}},
            "cold": {"faces": ["XMax"], "kind": "NoSlipWall",
                "thermal": {"kind": "fixed_temperature", "value": {"value": 299, "unit": "K"}}},
            "floor_and_roof": {"faces": ["YMin", "YMax"], "kind": "NoSlipWall",
                "thermal": {"kind": "adiabatic"}}
        });
        v["physics"] = json!({ "buoyancy": buoyancy });
    });
    let found = resolve(&heated).unwrap().physics.unwrap().buoyancy.unwrap();
    assert_eq!(
        (
            found.gravity,
            found.beta,
            found.prandtl,
            found.turbulent_prandtl
        ),
        (9.81, 0.003, 0.71, 0.85)
    );
    assert_eq!(found.reference_temperature, 300.0);
    // A flow through the domain can start from a potential-flow field.
    let mut channel: Value = serde_json::from_str(CHANNEL).unwrap();
    channel["solver"]["solver"] = "FromPhysics".into();
    channel["physics"] = json!({"init": "potential"});
    let study = Study::from_json(&channel.to_string()).unwrap();
    assert_eq!(
        resolve(&study).unwrap().physics.unwrap().init,
        InitMode::Potential
    );
}

#[test]
fn the_solver_kind_from_physics_is_accepted() {
    let study = cavity_with(|v| v["solver"]["solver"] = "FromPhysics".into());
    assert!(resolve(&study).is_ok());
    assert!(resolve(&cavity_with(|_| {})).is_ok());
    // The physics a study states is part of the study as it is read.
    let transient = cavity_with(|v| {
        v["solver"]["solver"] = "FromPhysics".into();
        v["physics"] = json!({
            "time": {"mode": "transient", "delta_t": {"value": 0.5, "unit": "s"}}
        });
    });
    assert!(matches!(
        transient.physics.as_ref().map(|p| p.time),
        Some(TimeMode::Transient { .. })
    ));
}

#[test]
fn the_inlet_values_reproduce_the_tutorial_numbers() {
    let found = inlet_turbulence(0.00061, 0.0003846, 44.2, 1.56e-5).unwrap();
    assert!(close(found.k, 1.0904e-3, 0.005), "{}", found.k);
    assert!(close(found.omega, 181_728.0, 0.005), "{}", found.omega);
    assert!(close(found.epsilon, 17.83, 0.005), "{}", found.epsilon);
    assert_eq!(inlet_turbulence(0.05, 0.0, 1.0, 1e-5), None);
    assert_eq!(inlet_turbulence(0.05, 10.0, 0.0, 1e-5), None);
    assert_eq!(inlet_turbulence(0.05, 10.0, 1.0, f64::NAN), None);
    assert_eq!(inlet_turbulence(0.0, 10.0, 1.0, 1e-5), None);
}
