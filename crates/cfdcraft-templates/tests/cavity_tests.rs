use cfdcraft_model::{
    InitMode, ResolvedTime, SolverKind, Study, TimeMode, TurbulenceModel, resolve,
};
use cfdcraft_templates::{MeshPreset, TemplateError, TemplateOptions, TimeChoice, build};

const EARLIER_CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn at(mesh: MeshPreset) -> TemplateOptions {
    TemplateOptions {
        mesh,
        ..TemplateOptions::default()
    }
}

#[test]
fn fine_is_the_phase_one_cavity() {
    let study = build("lid-cavity", &at(MeshPreset::Fine)).unwrap();
    assert_eq!((study.mesh.cells_x, study.mesh.cells_y), (32, 32));
    assert_eq!(study.mesh.level, 2);
    assert_eq!(study.solver.solver, SolverKind::SimpleLaminar);
    assert!(study.physics.is_none());

    let mut resolved = resolve(&study).unwrap();
    assert_eq!(
        (
            resolved.mesh.cells_x,
            resolved.mesh.cells_y,
            resolved.mesh.cells_z
        ),
        (128, 128, 1)
    );
    assert_eq!(resolved.mesh.level, 2);
    // Everything else is the earlier validated study.
    let earlier = resolve(&Study::from_json(EARLIER_CAVITY).unwrap()).unwrap();
    resolved.mesh.level = 0;
    assert_eq!(resolved, earlier);
}

#[test]
fn the_presets_are_levels_zero_one_two() {
    for (preset, cells, level) in [
        (MeshPreset::Coarse, 32, 0),
        (MeshPreset::Medium, 64, 1),
        (MeshPreset::Fine, 128, 2),
    ] {
        assert_eq!(preset.level(), level);
        let study = build("lid-cavity", &at(preset)).unwrap();
        let resolved = resolve(&study).unwrap();
        assert_eq!(resolved.mesh.cells_x, cells);
        assert_eq!(resolved.mesh.cells_y, cells);
        assert_eq!(resolved.mesh.level, level);
        let text = serde_json::to_string(&study).unwrap();
        assert_eq!(text.contains("\"level\""), level > 0, "level {level}");
    }
    assert_eq!(MeshPreset::default(), MeshPreset::Medium);
}

#[test]
fn options_switch_to_from_physics() {
    let with = |edit: fn(&mut TemplateOptions)| {
        let mut options = TemplateOptions::default();
        edit(&mut options);
        build("lid-cavity", &options).unwrap()
    };

    let turbulent = with(|o| o.turbulence = Some(TurbulenceModel::KOmegaSst));
    assert_eq!(turbulent.solver.solver, SolverKind::FromPhysics);
    let turbulence = turbulent.physics.as_ref().unwrap().turbulence.clone();
    assert_eq!(turbulence.model, TurbulenceModel::KOmegaSst);
    assert_eq!(turbulence.intensity, Some(0.05));
    assert_eq!(turbulence.viscosity_ratio, Some(10.0));
    resolve(&turbulent).unwrap();

    let transient = with(|o| o.time = Some(TimeChoice::Transient));
    assert_eq!(transient.solver.solver, SolverKind::FromPhysics);
    assert!(matches!(
        transient.physics.as_ref().unwrap().time,
        TimeMode::Transient { .. }
    ));
    let resolved = resolve(&transient).unwrap();
    assert_eq!(
        resolved.physics.unwrap().time,
        ResolvedTime::Transient { delta_t: 0.005 }
    );
    assert_eq!(resolved.solver.end_time, 20.0);
    assert_eq!(resolved.solver.write_interval, 5.0);

    let own_step = {
        let options = TemplateOptions {
            time: Some(TimeChoice::Transient),
            delta_t: Some(0.01),
            ..TemplateOptions::default()
        };
        resolve(&build("lid-cavity", &options).unwrap()).unwrap()
    };
    assert_eq!(
        own_step.physics.unwrap().time,
        ResolvedTime::Transient { delta_t: 0.01 }
    );

    let potential = with(|o| o.init = Some(InitMode::Potential));
    assert_eq!(potential.solver.solver, SolverKind::FromPhysics);
    assert_eq!(
        potential.physics.as_ref().unwrap().init,
        InitMode::Potential
    );
    resolve(&potential).unwrap();

    // A laminar choice is the plain study again.
    let laminar = with(|o| o.turbulence = Some(TurbulenceModel::Laminar));
    assert_eq!(laminar.solver.solver, SolverKind::SimpleLaminar);
    assert!(laminar.physics.is_none());

    // Every combination resolves, at every preset.
    for mesh in MeshPreset::ALL {
        for turbulence in [
            None,
            Some(TurbulenceModel::Laminar),
            Some(TurbulenceModel::KOmegaSst),
            Some(TurbulenceModel::RealizableKe),
            Some(TurbulenceModel::RngKEpsilon),
        ] {
            for time in [None, Some(TimeChoice::Steady), Some(TimeChoice::Transient)] {
                for init in [None, Some(InitMode::Uniform), Some(InitMode::Potential)] {
                    let options = TemplateOptions {
                        mesh,
                        turbulence,
                        time,
                        init,
                        ..TemplateOptions::default()
                    };
                    let study = build("lid-cavity", &options).unwrap();
                    resolve(&study).unwrap_or_else(|error| panic!("{options:?}: {error}"));
                }
            }
        }
    }
}

#[test]
fn an_unknown_id_or_a_bad_option_is_an_error() {
    let options = TemplateOptions::default();
    assert_eq!(
        build("nope", &options).unwrap_err(),
        TemplateError::UnknownTemplate {
            id: "nope".to_string()
        }
    );
    let bad_step = |delta_t: f64| {
        let options = TemplateOptions {
            time: Some(TimeChoice::Transient),
            delta_t: Some(delta_t),
            ..options
        };
        build("lid-cavity", &options).unwrap_err()
    };
    for delta_t in [0.0, -0.1, f64::NAN, f64::INFINITY] {
        assert_eq!(
            bad_step(delta_t),
            TemplateError::BadOption { option: "delta_t" }
        );
    }
    // A step means nothing for a steady run.
    let steady_with_step = TemplateOptions {
        delta_t: Some(0.01),
        ..options
    };
    assert_eq!(
        build("lid-cavity", &steady_with_step).unwrap_err(),
        TemplateError::BadOption { option: "delta_t" }
    );
    let rayleigh = TemplateOptions {
        rayleigh: Some(1e5),
        ..options
    };
    assert_eq!(
        build("lid-cavity", &rayleigh).unwrap_err(),
        TemplateError::BadOption { option: "rayleigh" }
    );
    let reynolds = TemplateOptions {
        reynolds: Some(100.0),
        ..options
    };
    assert_eq!(
        build("lid-cavity", &reynolds).unwrap_err(),
        TemplateError::BadOption { option: "reynolds" }
    );
}

#[test]
fn the_built_study_round_trips_through_json() {
    for mesh in MeshPreset::ALL {
        for turbulence in [None, Some(TurbulenceModel::RngKEpsilon)] {
            let options = TemplateOptions {
                mesh,
                turbulence,
                time: Some(TimeChoice::Transient),
                init: Some(InitMode::Potential),
                ..TemplateOptions::default()
            };
            let study = build("lid-cavity", &options).unwrap();
            let text = serde_json::to_string(&study).unwrap();
            assert_eq!(Study::from_json(&text).unwrap(), study);
        }
    }
}
