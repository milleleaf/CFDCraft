//! The solver plan names the program, the log, the pressure field, the fields of the end time and
//! the residuals that gate a steady run. The steady laminar plan must write exactly what the case
//! writer always wrote.

mod plan_studies;

use cfdcraft_backend_esi::{
    Dialect, ResidualLimit, SolverPlan, compile,
    export::{
        allrun_script, allrun_script_for, allrun_script_with, export_tree_for, export_tree_with,
        readme_text, readme_text_for, readme_text_with,
    },
};
use cfdcraft_model::{Study, resolve};
use plan_studies::{TRANSIENT, TRANSIENT_ONLY, cavity, heated};

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const GOLDEN_ALLRUN: &str = include_str!("golden/cavity-v2512-export/Allrun");

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn checks(items: &[(&str, ResidualLimit)]) -> Vec<(String, ResidualLimit)> {
    items.iter().map(|(f, l)| ((*f).to_string(), *l)).collect()
}

#[test]
fn the_four_plans() {
    use ResidualLimit::{Pressure, Velocity};
    let steady = SolverPlan::for_study(&cavity("{}", "5000", "500", 0));
    assert_eq!(steady.binary, "simpleFoam");
    assert_eq!(steady.log_file, "log.simpleFoam");
    assert_eq!(steady.pressure_field, "p");
    assert_eq!(steady.final_fields, strings(&["U", "p"]));
    assert_eq!(
        steady.residual_checks,
        checks(&[("p", Pressure), ("Ux", Velocity), ("Uy", Velocity)])
    );
    assert!(!steady.transient && !steady.buoyant && !steady.potential);

    let transient = SolverPlan::for_study(&cavity(TRANSIENT_ONLY, "8", "0.5", 0));
    assert_eq!(transient.binary, "pimpleFoam");
    assert_eq!(transient.log_file, "log.pimpleFoam");
    assert_eq!(transient.pressure_field, "p");
    assert_eq!(transient.final_fields, strings(&["U", "p"]));
    assert!(transient.residual_checks.is_empty());
    assert!(transient.transient && !transient.buoyant);

    let buoyant = SolverPlan::for_study(&heated("", "1000", "100"));
    assert_eq!(buoyant.binary, "buoyantBoussinesqSimpleFoam");
    assert_eq!(buoyant.log_file, "log.buoyantBoussinesqSimpleFoam");
    assert_eq!(buoyant.pressure_field, "p_rgh");
    assert_eq!(buoyant.final_fields, strings(&["U", "p_rgh", "T", "p"]));
    assert_eq!(
        buoyant.residual_checks,
        checks(&[
            ("p_rgh", Pressure),
            ("Ux", Velocity),
            ("Uy", Velocity),
            ("T", Velocity)
        ])
    );
    assert!(buoyant.buoyant && !buoyant.transient);

    let both = SolverPlan::for_study(&heated(TRANSIENT, "8", "0.5"));
    assert_eq!(both.binary, "buoyantBoussinesqPimpleFoam");
    assert_eq!(both.log_file, "log.buoyantBoussinesqPimpleFoam");
    assert_eq!(both.pressure_field, "p_rgh");
    assert_eq!(both.final_fields, strings(&["U", "p_rgh", "T"]));
    assert!(both.residual_checks.is_empty());
    assert!(both.buoyant && both.transient);
}

#[test]
fn turbulence_adds_fields() {
    let turbulent = |model: &str| {
        SolverPlan::for_study(&cavity(
            &format!(
                r#"{{"turbulence": {{"model": "{model}", "intensity": 0.05, "viscosity_ratio": 10.0}}}}"#
            ),
            "5000",
            "500",
            0,
        ))
    };
    assert_eq!(
        turbulent("k_omega_sst").final_fields,
        strings(&["U", "p", "k", "omega", "nut"])
    );
    for model in ["realizable_ke", "rng_k_epsilon"] {
        assert_eq!(
            turbulent(model).final_fields,
            strings(&["U", "p", "k", "epsilon", "nut"]),
            "{model}"
        );
    }
    assert_eq!(turbulent("laminar").final_fields, strings(&["U", "p"]));
    // A turbulent steady run is gated like a laminar one: the turbulence fields are not checked.
    assert_eq!(
        turbulent("k_omega_sst").residual_checks,
        turbulent("laminar").residual_checks
    );
}

#[test]
fn the_potential_step_follows_the_start_up_field() {
    let started = |physics: &str| SolverPlan::for_study(&cavity(physics, "5000", "500", 0));
    assert!(started(r#"{"init": "potential"}"#).potential);
    assert!(!started(r#"{"init": "uniform"}"#).potential);
    assert!(!started("{}").potential);
}

#[test]
fn the_steady_case_is_unchanged() {
    let study = resolve(&Study::from_json(CAVITY_V0).unwrap()).unwrap();
    let plan = SolverPlan::for_study(&study);
    assert_eq!(plan, SolverPlan::steady_laminar());
    assert_eq!(allrun_script(), GOLDEN_ALLRUN);
    assert_eq!(allrun_script_with(&plan, 1), GOLDEN_ALLRUN);
    assert_eq!(allrun_script_with(&plan, 0), GOLDEN_ALLRUN);
    let parallel = concat!(
        "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n",
        "blockMesh > log.blockMesh 2>&1 || exit 1\n",
        "checkMesh > log.checkMesh 2>&1 || exit 1\n",
        "decomposePar -force > log.decomposePar 2>&1 || exit 1\n",
        "mpirun -np 4 simpleFoam -parallel > log.simpleFoam 2>&1 || exit 1\n",
        "reconstructPar -latestTime > log.reconstructPar 2>&1 || exit 1\n",
    );
    assert_eq!(allrun_script_for(4), parallel);
    assert_eq!(allrun_script_with(&plan, 4), parallel);
    assert_eq!(readme_text_with("h", 1, &plan), readme_text("h"));
    assert_eq!(readme_text_with("h", 4, &plan), readme_text_for("h", 4));
    let tree = compile(&study, Dialect::EsiV2512).unwrap();
    assert_eq!(
        export_tree_with(&tree, "h", 4, &plan),
        export_tree_for(&tree, "h", 4)
    );
}

#[test]
fn the_readme_names_the_logs_of_the_plan() {
    let steady = readme_text_with("h", 1, &SolverPlan::steady_laminar());
    assert!(steady.contains(
        "each of the three logs (`log.blockMesh`, `log.checkMesh`, `log.simpleFoam`) ends with `End`"
    ));
    assert!(steady.contains("holds non-empty `U` and `p`."));

    let transient = SolverPlan::for_study(&cavity(TRANSIENT_ONLY, "8", "0.5", 0));
    let text = readme_text_with("h", 1, &transient);
    assert!(text.contains("(`log.blockMesh`, `log.checkMesh`, `log.pimpleFoam`)"));
    assert!(!text.contains("log.simpleFoam"));

    let potential = SolverPlan::for_study(&cavity(r#"{"init": "potential"}"#, "5000", "500", 0));
    let text = readme_text_with("h", 1, &potential);
    assert!(text.contains(
        "each of the four logs (`log.blockMesh`, `log.checkMesh`, `log.potentialFoam`, `log.simpleFoam`)"
    ));

    let buoyant = SolverPlan::for_study(&heated("", "1000", "100"));
    let text = readme_text_with("h", 1, &buoyant);
    assert!(text.contains("holds non-empty `U`, `p_rgh`, `T` and `p`."));
}
