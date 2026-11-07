use cfdcraft_model::{
    InitMode, InletProfile, ResolvedShape, ResolvedStudy, ResolvedTime, TurbulenceModel,
    inlet_turbulence, resolve,
};
use cfdcraft_shapes::{DomainSpec, MeshRequest, ShapeSpec, layout, problems, reference_totals};
use cfdcraft_templates::{MeshPreset, TemplateError, TemplateOptions, TimeChoice, build};

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

#[test]
fn the_turbulent_step_resolves_with_the_tutorial_numbers() {
    let study = build("backward-step", &at(MeshPreset::Medium)).unwrap();
    let domain = DomainSpec {
        length_x: 1.0,
        length_y: 1.0,
        length_z: 1.0,
    };
    let shape = ShapeSpec::BackwardStep {
        step_height: 0.0127,
        upstream_length: 1.651,
        startup_length: 0.254,
    };
    let (cells_x, cells_y) = reference_totals(&shape, &domain).unwrap();
    assert_eq!((cells_x, cells_y), (220, 114));
    assert_eq!((study.mesh.cells_x, study.mesh.cells_y), (220, 114));
    assert_eq!(study.mesh.level, 1);

    let r = resolve(&study).unwrap();
    assert_eq!(
        (r.mesh.cells_x, r.mesh.cells_y, r.mesh.level),
        (440, 228, 1)
    );
    assert!(r.domain.open);
    assert!(close(r.domain.length_x, 2.286, 1e-12));
    assert!(close(r.domain.length_y, 0.1143, 1e-12));
    assert!(close(r.domain.length_z, 0.0127, 1e-12));
    let Some(ResolvedShape::BackwardStep {
        step_height,
        upstream_length,
        startup_length,
    }) = r.domain.shape
    else {
        panic!("a backward step");
    };
    assert!(close(step_height, 0.0127, 1e-12));
    assert!(close(upstream_length, 1.651, 1e-12));
    assert!(close(startup_length, 0.254, 1e-12));
    assert!(close(r.fluid.kinematic_viscosity, 1.56e-5, 1e-12));
    assert_eq!(r.boundary_conditions["inlet"].velocity, Some(44.2));

    let physics = r.physics.as_ref().unwrap();
    assert_eq!(physics.turbulence.model, TurbulenceModel::KOmegaSst);
    assert_eq!(physics.turbulence.intensity, Some(0.00061));
    assert_eq!(physics.turbulence.viscosity_ratio, Some(0.0003846));
    assert_eq!(physics.time, ResolvedTime::Steady);
    assert_eq!(r.solver.end_time, 2000.0);
    assert_eq!(r.solver.write_interval, 100.0);
    assert_eq!(r.solver.convergence.p, 1e-4);
    assert_eq!(r.solver.convergence.u, 1e-6);
}

#[test]
fn the_inlet_values_are_the_tutorials() {
    let r = resolved("backward-step", &at(MeshPreset::Medium));
    let turbulence = &r.physics.as_ref().unwrap().turbulence;
    let values = inlet_turbulence(
        turbulence.intensity.unwrap(),
        turbulence.viscosity_ratio.unwrap(),
        r.boundary_conditions["inlet"].velocity.unwrap(),
        r.fluid.kinematic_viscosity,
    )
    .unwrap();
    assert!(close(values.k, 1.09e-3, 1e-3), "k {}", values.k);
    assert!(
        close(values.omega, 181_728.0, 2e-3),
        "omega {}",
        values.omega
    );
    // The numbers are also what the function gives for the literal inputs.
    let literal = inlet_turbulence(0.00061, 3.846e-4, 44.2, 1.56e-5).unwrap();
    assert_eq!(literal.k, values.k);
    assert_eq!(literal.omega, values.omega);
}

#[test]
fn the_reynolds_number_is_36000() {
    let r = resolved("backward-step", &at(MeshPreset::Medium));
    let ResolvedShape::BackwardStep { step_height, .. } = r.domain.shape.unwrap() else {
        panic!("a backward step");
    };
    let speed = r.boundary_conditions["inlet"].velocity.unwrap();
    let reynolds = speed * step_height / r.fluid.kinematic_viscosity;
    assert!(close(reynolds, 35_985.0, 1e-4), "Re {reynolds}");
}

#[test]
fn the_labelled_monitors_are_present() {
    let r = resolved("backward-step", &at(MeshPreset::Medium));
    let line = |label: &str| {
        r.monitors
            .lines
            .iter()
            .find(|l| l.label == label)
            .unwrap_or_else(|| panic!("no line {label}"))
    };
    assert_eq!(r.monitors.lines.len(), 5);
    let step_x = 1.651;
    let height = 0.0127;
    let floor = line("floor");
    assert!(close(floor.y0, 0.01 * height, 1e-9));
    assert_eq!(floor.y0, floor.y1);
    assert!(close(floor.x0, step_x, 1e-9));
    assert!(close(floor.x1, 2.286, 1e-9));
    assert!(floor.samples >= 1000);
    for k in [1.0, 4.0, 6.0, 10.0] {
        let profile = line(&format!("profile_x{k}"));
        assert!(close(profile.x0, step_x + k * height, 1e-9), "x{k}");
        assert_eq!(profile.x0, profile.x1);
        assert_eq!(profile.y0, 0.0);
        assert!(close(profile.y1, 0.1143, 1e-9));
        assert!(profile.samples >= 400);
    }
    for l in &r.monitors.lines {
        assert_eq!(l.field, cfdcraft_model::MonitorField::Velocity);
    }
    let labels: Vec<&str> = r.boundary_conditions.keys().map(String::as_str).collect();
    assert_eq!(labels, ["floor_walls", "inlet", "outlet", "top_wall"]);
}

#[test]
fn the_laminar_step_resolves() {
    let r = resolved("backward-step-laminar", &at(MeshPreset::Medium));
    assert!(close(r.domain.length_x, 0.173, 1e-12));
    assert!(close(r.domain.length_y, 0.0101, 1e-12));
    let ResolvedShape::BackwardStep {
        step_height,
        upstream_length,
        startup_length,
    } = r.domain.shape.unwrap()
    else {
        panic!("a backward step");
    };
    assert!(close(step_height, 0.0049, 1e-12));
    assert!(close(upstream_length, 0.026, 1e-12));
    assert_eq!(startup_length, 0.0);
    assert!(close(r.fluid.kinematic_viscosity, 1.5e-5, 1e-12));
    assert_eq!(
        (r.mesh.cells_x, r.mesh.cells_y, r.mesh.level),
        (436, 228, 1)
    );
    assert!(r.physics.is_none(), "laminar steady is the plain physics");
    let inlet = &r.boundary_conditions["inlet"];
    assert_eq!(inlet.profile, Some(InletProfile::Parabolic));
    // U (2 h) / nu with h = 5.2 mm is 200.
    let reynolds = inlet.velocity.unwrap() * 2.0 * 0.0052 / r.fluid.kinematic_viscosity;
    assert!(close(reynolds, 200.0, 1e-9), "Re {reynolds}");
    assert_eq!(r.monitors.lines.len(), 1);
    assert_eq!(r.monitors.lines[0].label, "floor");

    // Only the inlet speed changes with the Reynolds number.
    let base = resolved("backward-step-laminar", &at(MeshPreset::Medium));
    for reynolds in [100.0, 400.0] {
        let options = TemplateOptions {
            reynolds: Some(reynolds),
            ..at(MeshPreset::Medium)
        };
        let mut other = resolved("backward-step-laminar", &options);
        let speed = other.boundary_conditions["inlet"].velocity;
        assert!(close(
            speed.unwrap(),
            base.boundary_conditions["inlet"].velocity.unwrap() * reynolds / 200.0,
            1e-9
        ));
        other.boundary_conditions.get_mut("inlet").unwrap().velocity =
            base.boundary_conditions["inlet"].velocity;
        assert_eq!(other, base);
    }
    for reynolds in [50.0, 600.0, f64::NAN] {
        let options = TemplateOptions {
            reynolds: Some(reynolds),
            ..at(MeshPreset::Medium)
        };
        assert_eq!(
            build("backward-step-laminar", &options).unwrap_err(),
            TemplateError::BadOption { option: "reynolds" }
        );
    }
    // The turbulent step and the Rayleigh number are not offered here.
    let options = TemplateOptions {
        reynolds: Some(36_000.0),
        ..at(MeshPreset::Medium)
    };
    assert_eq!(
        build("backward-step", &options).unwrap_err(),
        TemplateError::BadOption { option: "reynolds" }
    );
    let options = TemplateOptions {
        rayleigh: Some(1e5),
        ..at(MeshPreset::Medium)
    };
    for id in ["backward-step", "backward-step-laminar"] {
        assert_eq!(
            build(id, &options).unwrap_err(),
            TemplateError::BadOption { option: "rayleigh" }
        );
    }
}

#[test]
fn every_turbulence_model_and_the_time_and_init_options_resolve() {
    for id in ["backward-step", "backward-step-laminar"] {
        for mesh in MeshPreset::ALL {
            for turbulence in [
                None,
                Some(TurbulenceModel::Laminar),
                Some(TurbulenceModel::KOmegaSst),
                Some(TurbulenceModel::RealizableKe),
                Some(TurbulenceModel::RngKEpsilon),
            ] {
                for time in [None, Some(TimeChoice::Steady), Some(TimeChoice::Transient)] {
                    for init in [None, Some(InitMode::Potential)] {
                        let options = TemplateOptions {
                            mesh,
                            turbulence,
                            time,
                            init,
                            ..TemplateOptions::default()
                        };
                        let study = build(id, &options).unwrap();
                        resolve(&study).unwrap_or_else(|error| panic!("{id} {options:?}: {error}"));
                    }
                }
            }
        }
    }
    // The transient step is the one the template names, divided by two per level.
    for (id, delta_t, end, interval) in [
        ("backward-step", 1e-5, 0.5, 0.05),
        ("backward-step-laminar", 1e-4, 5.0, 0.5),
    ] {
        let options = TemplateOptions {
            time: Some(TimeChoice::Transient),
            ..at(MeshPreset::Coarse)
        };
        let r = resolved(id, &options);
        assert_eq!(
            r.physics.as_ref().unwrap().time,
            ResolvedTime::Transient { delta_t }
        );
        assert_eq!(r.solver.end_time, end);
        assert_eq!(r.solver.write_interval, interval);
        let options = TemplateOptions {
            time: Some(TimeChoice::Transient),
            ..at(MeshPreset::Fine)
        };
        let step = resolved(id, &options).effective_delta_t().unwrap();
        assert!(close(step, delta_t / 4.0, 1e-12));
    }
}

#[test]
fn the_shape_layouts_are_sound_at_each_preset() {
    for id in ["backward-step", "backward-step-laminar"] {
        for mesh in MeshPreset::ALL {
            assert_sound(&resolved(id, &at(mesh)));
        }
    }
}
