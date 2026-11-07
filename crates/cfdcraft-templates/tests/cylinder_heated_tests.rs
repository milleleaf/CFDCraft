use cfdcraft_model::{
    InitMode, InletProfile, MonitorField, ResolvedShape, ResolvedStudy, ResolvedThermal,
    ResolvedTime, SurfaceQuantity, TurbulenceModel, resolve,
};
use cfdcraft_shapes::{DomainSpec, MeshRequest, ShapeSpec, layout, problems};
use cfdcraft_templates::{
    MeshPreset, TemplateError, TemplateOptions, TimeChoice, build, viscosity_for_rayleigh,
};

fn at(mesh: MeshPreset) -> TemplateOptions {
    TemplateOptions {
        mesh,
        ..TemplateOptions::default()
    }
}

fn resolved(id: &str, options: &TemplateOptions) -> ResolvedStudy {
    resolve(&build(id, options).unwrap()).unwrap()
}

fn close(found: f64, wanted: f64, relative: f64) -> bool {
    (found - wanted).abs() <= relative * wanted.abs()
}

/// The shape of a resolved study as the shapes crate takes it.
fn shape_of(r: &ResolvedStudy) -> ShapeSpec {
    match r.domain.shape {
        None => ShapeSpec::Box,
        Some(ResolvedShape::BackwardStep {
            step_height,
            upstream_length,
            startup_length,
        }) => ShapeSpec::BackwardStep {
            step_height,
            upstream_length,
            startup_length,
        },
        Some(ResolvedShape::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        }) => ShapeSpec::CylinderChannel {
            diameter,
            centre_x,
            centre_y,
        },
    }
}

/// The block layout of a resolved study is sound.
fn assert_sound(r: &ResolvedStudy) {
    let domain = DomainSpec {
        length_x: r.domain.length_x,
        length_y: r.domain.length_y,
        length_z: r.domain.length_z,
    };
    let shape = shape_of(r);
    let request = MeshRequest {
        cells_x: r.mesh.cells_x,
        cells_y: r.mesh.cells_y,
        cells_z: r.mesh.cells_z,
        level: r.mesh.level,
        ratio_x: r.mesh.grading.and_then(|g| g.x),
        ratio_y: r.mesh.grading.and_then(|g| g.y),
    };
    let layout = layout(&domain, &shape, &request).unwrap();
    let found = problems(&layout, &domain, &shape);
    assert!(found.is_empty(), "{found:?}");
}

const MODELS: [Option<TurbulenceModel>; 5] = [
    None,
    Some(TurbulenceModel::Laminar),
    Some(TurbulenceModel::KOmegaSst),
    Some(TurbulenceModel::RealizableKe),
    Some(TurbulenceModel::RngKEpsilon),
];

#[test]
fn the_cylinder_resolves_as_schafer_turek_2d2() {
    let study = build("cylinder", &at(MeshPreset::Medium)).unwrap();
    assert_eq!((study.mesh.cells_x, study.mesh.cells_y), (114, 44));
    assert_eq!(study.mesh.level, 1);
    let r = resolve(&study).unwrap();
    assert!(close(r.domain.length_x, 2.2, 1e-12));
    assert!(close(r.domain.length_y, 0.41, 1e-12));
    assert!(close(r.domain.length_z, 0.01, 1e-12));
    assert!(r.domain.open);
    let Some(ResolvedShape::CylinderChannel {
        diameter,
        centre_x,
        centre_y,
    }) = r.domain.shape
    else {
        panic!("a cylinder channel");
    };
    assert!(close(diameter, 0.1, 1e-12));
    assert!(close(centre_x, 0.2, 1e-12));
    assert!(close(centre_y, 0.2, 1e-12));
    assert_eq!(r.fluid.kinematic_viscosity, 1e-3);

    let inlet = &r.boundary_conditions["inlet"];
    assert_eq!(inlet.velocity, Some(1.0));
    assert_eq!(inlet.profile, Some(InletProfile::Parabolic));
    let labels: Vec<&str> = r.boundary_conditions.keys().map(String::as_str).collect();
    assert_eq!(labels, ["channel_walls", "cylinder", "inlet", "outlet"]);
    // Reynolds number: U D / nu.
    let reynolds = inlet.velocity.unwrap() * diameter / r.fluid.kinematic_viscosity;
    assert!(close(reynolds, 100.0, 1e-9), "Re {reynolds}");

    assert_eq!((r.mesh.cells_x, r.mesh.cells_y, r.mesh.level), (228, 88, 1));
    // The step is 1e-3 s at level 0 and halves at Medium.
    assert_eq!(
        r.physics.as_ref().unwrap().time,
        ResolvedTime::Transient { delta_t: 1e-3 }
    );
    assert!(close(r.effective_delta_t().unwrap(), 5e-4, 1e-12));
    assert_eq!(r.solver.end_time, 8.0);
    assert_eq!(r.solver.write_interval, 0.5);

    assert_eq!(r.monitors.forces.len(), 1);
    let force = &r.monitors.forces[0];
    assert_eq!(force.label, "cylinder_forces");
    assert_eq!(force.boundary, "cylinder");
    assert_eq!(force.reference_velocity, 1.0);
    assert!(close(force.reference_length, 0.1, 1e-12));
    assert_eq!(r.monitors.probes.len(), 2);
    for (label, x) in [("front", 0.15), ("rear", 0.25)] {
        let probe = r
            .monitors
            .probes
            .iter()
            .find(|p| p.label == label)
            .unwrap_or_else(|| panic!("no probe {label}"));
        assert!(close(probe.x, x, 1e-12), "{label}");
        assert!(close(probe.y, 0.2, 1e-12), "{label}");
        assert_eq!(probe.field, MonitorField::Pressure);
        assert_eq!(probe.boundary.as_deref(), Some("cylinder"));
    }
}

#[test]
fn the_heated_cavity_derives_nu_from_the_rayleigh_number() {
    let by_formula = |ra: f64| (9.81_f64 * 3e-3 * 0.71 / ra).sqrt();
    let r = resolved("heated-cavity", &at(MeshPreset::Medium));
    assert!(close(r.fluid.kinematic_viscosity, by_formula(1e5), 1e-9));
    for ra in [1e3, 1e4, 1e5, 1e6] {
        let options = TemplateOptions {
            rayleigh: Some(ra),
            ..at(MeshPreset::Medium)
        };
        let r = resolved("heated-cavity", &options);
        assert!(
            close(r.fluid.kinematic_viscosity, by_formula(ra), 1e-9),
            "Ra {ra}"
        );
        assert!(close(viscosity_for_rayleigh(ra), by_formula(ra), 1e-9));
    }
    for ra in [99.0, 2e8, 0.0, -1e5, f64::NAN] {
        let options = TemplateOptions {
            rayleigh: Some(ra),
            ..at(MeshPreset::Medium)
        };
        assert_eq!(
            build("heated-cavity", &options).unwrap_err(),
            TemplateError::BadOption { option: "rayleigh" },
            "Ra {ra}"
        );
    }
    let options = TemplateOptions {
        reynolds: Some(100.0),
        ..at(MeshPreset::Medium)
    };
    assert_eq!(
        build("heated-cavity", &options).unwrap_err(),
        TemplateError::BadOption { option: "reynolds" }
    );
}

#[test]
fn the_heated_cavity_has_its_walls_and_monitors() {
    let study = build("heated-cavity", &at(MeshPreset::Medium)).unwrap();
    assert_eq!((study.mesh.cells_x, study.mesh.cells_y), (40, 40));
    assert_eq!(study.mesh.level, 1);
    let r = resolve(&study).unwrap();
    assert!(!r.domain.open);
    assert!(r.domain.shape.is_none());
    assert_eq!((r.mesh.cells_x, r.mesh.cells_y, r.mesh.level), (80, 80, 1));
    let grading = r.mesh.grading.unwrap();
    assert_eq!((grading.x, grading.y), (Some(1.088), Some(1.088)));

    let hot = &r.boundary_conditions["hot"];
    assert_eq!(hot.faces, [cfdcraft_model::Face::XMin]);
    assert_eq!(
        hot.thermal,
        Some(ResolvedThermal::FixedTemperature { value: 300.5 })
    );
    let cold = &r.boundary_conditions["cold"];
    assert_eq!(cold.faces, [cfdcraft_model::Face::XMax]);
    assert_eq!(
        cold.thermal,
        Some(ResolvedThermal::FixedTemperature { value: 299.5 })
    );
    let adiabatic = &r.boundary_conditions["adiabatic"];
    assert_eq!(
        adiabatic.faces,
        [cfdcraft_model::Face::YMin, cfdcraft_model::Face::YMax]
    );
    assert_eq!(adiabatic.thermal, Some(ResolvedThermal::Adiabatic));
    assert_eq!(r.boundary_conditions.len(), 3);

    let physics = r.physics.as_ref().unwrap();
    assert_eq!(physics.time, ResolvedTime::Steady);
    assert_eq!(physics.turbulence.model, TurbulenceModel::Laminar);
    let buoyancy = physics.buoyancy.unwrap();
    assert_eq!(buoyancy.gravity, 9.81);
    assert_eq!(buoyancy.beta, 3e-3);
    assert_eq!(buoyancy.reference_temperature, 300.0);
    assert_eq!(buoyancy.prandtl, 0.71);
    assert_eq!(buoyancy.turbulent_prandtl, 0.85);
    assert_eq!(r.solver.end_time, 20_000.0);
    assert_eq!(r.solver.write_interval, 1_000.0);

    assert_eq!(r.monitors.surfaces.len(), 2);
    for (label, boundary) in [("hot_flux", "hot"), ("cold_flux", "cold")] {
        let surface = r
            .monitors
            .surfaces
            .iter()
            .find(|s| s.label == label)
            .unwrap_or_else(|| panic!("no surface {label}"));
        assert_eq!(surface.boundary, boundary);
        assert_eq!(surface.quantity, SurfaceQuantity::HeatFlux);
    }
    assert_eq!(r.monitors.lines.len(), 2);
    let vertical = r
        .monitors
        .lines
        .iter()
        .find(|l| l.label == "mid_vertical")
        .unwrap();
    assert_eq!((vertical.x0, vertical.x1), (0.5, 0.5));
    assert_eq!((vertical.y0, vertical.y1), (0.0, 1.0));
    assert_eq!(vertical.samples, 201);
    let horizontal = r
        .monitors
        .lines
        .iter()
        .find(|l| l.label == "mid_horizontal")
        .unwrap();
    assert_eq!((horizontal.y0, horizontal.y1), (0.5, 0.5));
    assert_eq!((horizontal.x0, horizontal.x1), (0.0, 1.0));
    assert_eq!(horizontal.samples, 201);
}

#[test]
fn every_option_resolves_on_both() {
    // The buoyancy velocity scale, for the turbulence values of the closed cavity.
    let scale = (9.81_f64 * 3e-3).sqrt();
    for mesh in MeshPreset::ALL {
        for turbulence in MODELS {
            for time in [None, Some(TimeChoice::Steady), Some(TimeChoice::Transient)] {
                for id in ["cylinder", "heated-cavity"] {
                    for init in [None, Some(InitMode::Potential)] {
                        let options = TemplateOptions {
                            mesh,
                            turbulence,
                            time,
                            init,
                            ..TemplateOptions::default()
                        };
                        let built = build(id, &options);
                        if id == "heated-cavity" && init.is_some() {
                            assert_eq!(
                                built.unwrap_err(),
                                TemplateError::BadOption { option: "init" }
                            );
                            continue;
                        }
                        let study = built.unwrap();
                        let r = resolve(&study)
                            .unwrap_or_else(|error| panic!("{id} {options:?}: {error}"));
                        if id == "heated-cavity"
                            && !matches!(turbulence, None | Some(TurbulenceModel::Laminar))
                        {
                            let found = r.physics.unwrap().turbulence.reference_velocity;
                            assert!(close(found.unwrap(), scale, 1e-12));
                        }
                    }
                }
            }
        }
    }
    // The heated cavity marches in 0.1 s steps (level 0) over 3000 s.
    let options = TemplateOptions {
        time: Some(TimeChoice::Transient),
        ..at(MeshPreset::Coarse)
    };
    let r = resolved("heated-cavity", &options);
    assert_eq!(
        r.physics.as_ref().unwrap().time,
        ResolvedTime::Transient { delta_t: 0.1 }
    );
    assert_eq!(r.solver.end_time, 3000.0);
    assert_eq!(r.solver.write_interval, 300.0);
    // The cylinder is transient already; a steady choice counts iterations instead.
    let options = TemplateOptions {
        time: Some(TimeChoice::Steady),
        ..at(MeshPreset::Medium)
    };
    let r = resolved("cylinder", &options);
    assert!(r.effective_delta_t().is_none());
    assert_eq!(r.solver.end_time, 2000.0);
    // An own time step needs a transient run.
    let options = TemplateOptions {
        delta_t: Some(2e-3),
        ..at(MeshPreset::Coarse)
    };
    let r = resolved("cylinder", &options);
    assert_eq!(
        r.physics.as_ref().unwrap().time,
        ResolvedTime::Transient { delta_t: 2e-3 }
    );
    let options = TemplateOptions {
        delta_t: Some(2e-3),
        time: Some(TimeChoice::Steady),
        ..at(MeshPreset::Coarse)
    };
    assert_eq!(
        build("cylinder", &options).unwrap_err(),
        TemplateError::BadOption { option: "delta_t" }
    );
    let options = TemplateOptions {
        delta_t: Some(f64::NAN),
        ..at(MeshPreset::Coarse)
    };
    assert_eq!(
        build("cylinder", &options).unwrap_err(),
        TemplateError::BadOption { option: "delta_t" }
    );
}

#[test]
fn the_layouts_are_sound() {
    for mesh in MeshPreset::ALL {
        assert_sound(&resolved("cylinder", &at(mesh)));
        assert_sound(&resolved("heated-cavity", &at(mesh)));
    }
}
