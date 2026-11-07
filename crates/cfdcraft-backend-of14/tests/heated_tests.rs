//! The heated cavity compiles to the heat transfer solver module with a Boussinesq equation of
//! state, the constants follow the study and the free choices are named, and the facts of the
//! study are read back from the written text.

mod common;

use cfdcraft_backend::Backend;
use cfdcraft_backend_esi::{patches::patch_plan, scalar};
use cfdcraft_backend_of14::thermo::{HEAT_CAPACITY, MOLECULAR_WEIGHT, REFERENCE_DENSITY};
use cfdcraft_model::{RawBackend, RawOverride, resolve};
use cfdcraft_templates::MeshPreset;
use common::{
    authored, backend, broken, compiled, edited, heated, line_of, names, number_after, options,
    text,
};

fn row() -> &'static dyn Backend {
    backend()
}

/// The relative difference of two numbers, for a stated tolerance.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs())
}

#[test]
fn the_heated_cavity_uses_the_fluid_solver_module() {
    let tree = compiled(&heated());
    assert_eq!(
        names(&tree),
        [
            "0/T",
            "0/U",
            "0/alphat",
            "0/p",
            "0/p_rgh",
            "constant/g",
            "constant/momentumTransport",
            "constant/physicalProperties",
            "constant/thermophysicalTransport",
            "system/blockMeshDict",
            "system/controlDict",
            "system/fvSchemes",
            "system/fvSolution",
        ]
    );
    let control = text(&tree, "system/controlDict");
    assert!(control.contains("solver fluid;"));
    assert!(!control.contains("application"));

    let physical = text(&tree, "constant/physicalProperties");
    for word in [
        "type heRhoThermo;",
        "equationOfState Boussinesq;",
        "energy sensibleEnthalpy;",
        "equationOfState\n{\n    rho0 ",
        "    T0 ",
        "    beta ",
        "thermodynamics\n{\n    Cp ",
        "    hf ",
        "transport\n{\n    mu ",
        "    Pr ",
    ] {
        assert!(
            physical.contains(word),
            "{word} is missing from\n{physical}"
        );
    }
    assert!(!physical.contains("viscosityModel"));

    let gravity = text(&tree, "constant/g");
    assert!(gravity.contains("uniformDimensionedVectorField"));
    assert!(gravity.contains("value (0 -9.81 0);"));

    let solution = text(&tree, "system/fvSolution");
    assert!(solution.contains("PIMPLE"));
    assert!(solution.contains("p_rgh 0.7;"));
    assert!(solution.contains("h 0.5;"));
    assert!(solution.contains("pRefCell 0;"));
    let schemes = text(&tree, "system/fvSchemes");
    assert!(schemes.contains("ddtSchemes { default steadyState; }"));
    assert!(schemes.contains("div(phi,h)"));
    assert!(text(&tree, "constant/momentumTransport").contains("simulationType laminar;"));
    assert!(text(&tree, "0/p_rgh").contains("fixedFluxPressure"));
    assert!(text(&tree, "0/alphat").contains("type calculated;"));
}

#[test]
fn the_constants_follow_the_study_and_the_free_choices_are_named() {
    let study = heated();
    let buoyancy = study.physics.as_ref().unwrap().buoyancy.unwrap();
    let tree = compiled(&study);
    let file = "constant/physicalProperties";
    let rho0 = number_after(&tree, file, "rho0");
    let mu = number_after(&tree, file, "mu");
    let cp = number_after(&tree, file, "Cp");
    let pr = number_after(&tree, file, "Pr");
    assert_eq!(rho0, 1.0);
    assert_eq!(rho0, REFERENCE_DENSITY);
    assert_eq!(mu, study.fluid.kinematic_viscosity * rho0);
    assert_eq!(pr, buoyancy.prandtl);
    assert_eq!(number_after(&tree, file, "beta"), buoyancy.beta);
    assert_eq!(
        number_after(&tree, file, "T0"),
        buoyancy.reference_temperature
    );
    // The free choices are the named constants.
    assert_eq!(cp, HEAT_CAPACITY);
    assert_eq!(number_after(&tree, file, "molWeight"), MOLECULAR_WEIGHT);
    // The Prandtl form: the conductivity is Cp mu / Pr, so the thermal diffusivity of the written
    // case is the study's nu / Pr for any Cp.
    for any_cp in [cp, 2.0 * cp, 0.5 * cp] {
        let conductivity = any_cp * mu / pr;
        let diffusivity = conductivity / (rho0 * any_cp);
        assert!(close(diffusivity, study.fluid.kinematic_viscosity / pr));
    }
    // The turbulent Prandtl number is in the case too.
    let transport = "constant/thermophysicalTransport";
    assert_eq!(
        number_after(&tree, transport, "Prt"),
        buoyancy.turbulent_prandtl
    );
}

#[test]
fn a_raw_override_can_change_cp_for_the_insensitivity_check() {
    let mut study = authored("heated-cavity", &options(MeshPreset::Coarse));
    study.raw_overrides.insert(
        "cp".into(),
        RawOverride {
            backend: RawBackend::Foundation,
            file: "constant/physicalProperties".into(),
            entry: "thermodynamics/Cp".into(),
            value: "2000".into(),
        },
    );
    let changed = compiled(&resolve(&study).unwrap());
    let base = compiled(&heated());
    let cp = format!("Cp {};", scalar(HEAT_CAPACITY));
    let original = text(&base, "constant/physicalProperties");
    let edited_text = text(&changed, "constant/physicalProperties");
    assert!(edited_text.contains("Cp 2000;"));
    assert_eq!(edited_text.replacen("Cp 2000;", &cp, 1), original);
    for name in names(&base) {
        if name != "constant/physicalProperties" {
            assert_eq!(text(&changed, &name), text(&base, &name), "{name}");
        }
    }
}

#[test]
fn the_heated_run_plan_and_round_trip() {
    let study = heated();
    let tree = compiled(&study);
    let plan = row().run_plan(&study, 1);
    assert_eq!(plan.solver, "foamRun");
    assert!(plan.buoyant);
    assert_eq!(plan.pressure_field, "p_rgh");
    assert!(plan.final_fields.contains(&"T".to_owned()));
    assert!(!plan.residual_checks.is_empty());
    // The fluid module solves the enthalpy, so the judge gates on `h`, never on a `T` residual.
    assert_eq!(plan.energy_field, "h");
    assert!(row().scan(&tree).is_empty());
    assert_eq!(row().read_back(&study, &tree), vec![]);

    let buoyancy = study.physics.as_ref().unwrap().buoyancy.unwrap();
    let file = "constant/physicalProperties";
    let beta = format!("beta {};", scalar(buoyancy.beta));
    assert!(broken(&edited(&tree, file, &beta, "beta 0.5;"), &study));
    let pr = format!("Pr {};", scalar(buoyancy.prandtl));
    assert!(broken(&edited(&tree, file, &pr, "Pr 9;"), &study));
    let prt = format!("Prt {};", scalar(buoyancy.turbulent_prandtl));
    assert!(broken(
        &edited(&tree, "constant/thermophysicalTransport", &prt, "Prt 9;"),
        &study
    ));
    // The hot wall temperature, found by the patch that holds it.
    let hot = patch_plan(&study)
        .unwrap()
        .into_iter()
        .find(|p| p.temperature == Some(300.5))
        .expect("the heated cavity has a hot wall");
    let line = line_of(&tree, "0/T", hot.name);
    let warmer = line.replacen("300.5", "301.5", 1);
    assert!(broken(&edited(&tree, "0/T", &line, &warmer), &study));
    // The gravity.
    let gravity = line_of(&tree, "constant/g", "value");
    assert!(broken(
        &edited(&tree, "constant/g", &gravity, "value (0 -5 0);"),
        &study
    ));
    // A study that asks for another equation of state is refused by the reader, not read as the
    // study's.
    let other = edited(
        &tree,
        file,
        "equationOfState Boussinesq;",
        "equationOfState perfectGas;",
    );
    assert!(broken(&other, &study));
}
