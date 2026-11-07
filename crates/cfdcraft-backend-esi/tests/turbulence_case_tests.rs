//! The four turbulence choices are written with their model name, their fields, their wall
//! functions and the schemes and solvers that go with them. The backward step is the study.

mod field_studies;

use cfdcraft_backend_esi::{control_dict, fields::inlet_values_text};
use cfdcraft_model::{ResolvedStudy, Study, inlet_turbulence, resolve};
use field_studies::{
    assert_clean, block, entry, file, golden_differences, has_file, lines, num, resolved,
    step_json, tree,
};

fn physics(model: &str, extra: &str) -> String {
    format!(
        r#"{{"turbulence":{{"model":"{model}","intensity":0.05,"viscosity_ratio":10.0{extra}}}}}"#
    )
}

fn step(model: &str, extra: &str) -> ResolvedStudy {
    resolved(&step_json(&physics(model, extra), "100", "100", ""))
}

/// The inflow values the files must hold, from the model's own function and the resolved study.
fn expected_values(s: &ResolvedStudy, speed: f64) -> (String, String, String) {
    let v = inlet_turbulence(0.05, 10.0, speed, s.fluid.kinematic_viscosity).unwrap();
    (num(v.k), num(v.omega), num(v.epsilon))
}

fn inlet_speed(s: &ResolvedStudy) -> f64 {
    s.boundary_conditions["inlet"].velocity.unwrap()
}

#[test]
fn k_omega_sst_writes_its_model_and_fields() {
    let s = step("k_omega_sst", "");
    let t = tree(&s);
    let properties = file(&t, "constant/turbulenceProperties");
    assert_eq!(
        block(&properties, "RAS"),
        [
            "RAS",
            "{",
            "RASModel kOmegaSST;",
            "turbulence on;",
            "printCoeffs on;",
            "}"
        ]
    );
    assert!(lines(&properties).contains(&"simulationType RAS;"));
    for name in ["0/k", "0/omega", "0/nut"] {
        assert!(has_file(&t, name), "{name}");
    }
    assert!(!has_file(&t, "0/epsilon"));

    let (k, omega, _) = expected_values(&s, inlet_speed(&s));
    let k_text = file(&t, "0/k");
    assert!(lines(&k_text).contains(&"dimensions [0 2 -2 0 0 0 0];"));
    assert!(lines(&k_text).contains(&format!("internalField uniform {k};").as_str()));
    assert_eq!(
        entry(&t, "0/k", "inlet"),
        format!("inlet {{ type fixedValue; value uniform {k}; }}")
    );
    assert_eq!(entry(&t, "0/k", "outlet"), "outlet { type zeroGradient; }");
    assert_eq!(
        entry(&t, "0/k", "fixedWalls"),
        format!("fixedWalls {{ type kqRWallFunction; value uniform {k}; }}")
    );
    assert_eq!(entry(&t, "0/k", "startup"), "startup { type symmetry; }");
    assert_eq!(
        entry(&t, "0/k", "frontAndBack"),
        "frontAndBack { type empty; }"
    );

    let omega_text = file(&t, "0/omega");
    assert!(lines(&omega_text).contains(&"dimensions [0 0 -1 0 0 0 0];"));
    assert!(lines(&omega_text).contains(&format!("internalField uniform {omega};").as_str()));
    assert_eq!(
        entry(&t, "0/omega", "fixedWalls"),
        format!("fixedWalls {{ type omegaWallFunction; value uniform {omega}; }}")
    );
    assert_eq!(
        entry(&t, "0/omega", "inlet"),
        format!("inlet {{ type fixedValue; value uniform {omega}; }}")
    );

    let nut = file(&t, "0/nut");
    assert!(lines(&nut).contains(&"internalField uniform 0;"));
    for patch in ["inlet", "outlet"] {
        assert_eq!(
            entry(&t, "0/nut", patch),
            format!("{patch} {{ type calculated; value uniform 0; }}")
        );
    }
    assert_eq!(
        entry(&t, "0/nut", "fixedWalls"),
        "fixedWalls { type nutUBlendedWallFunction; value uniform 0; }"
    );
}

#[test]
fn the_k_epsilon_models_write_epsilon() {
    for (model, word) in [
        ("realizable_ke", "RASModel realizableKE;"),
        ("rng_k_epsilon", "RASModel RNGkEpsilon;"),
    ] {
        let s = step(model, "");
        let t = tree(&s);
        assert!(
            lines(&file(&t, "constant/turbulenceProperties")).contains(&word),
            "{model}"
        );
        for name in ["0/k", "0/epsilon", "0/nut"] {
            assert!(has_file(&t, name), "{model} {name}");
        }
        assert!(!has_file(&t, "0/omega"), "{model}");
        let (_, _, epsilon) = expected_values(&s, inlet_speed(&s));
        let text = file(&t, "0/epsilon");
        assert!(lines(&text).contains(&"dimensions [0 2 -3 0 0 0 0];"));
        assert_eq!(
            entry(&t, "0/epsilon", "fixedWalls"),
            format!("fixedWalls {{ type epsilonWallFunction; value uniform {epsilon}; }}")
        );
        assert_eq!(
            entry(&t, "0/nut", "fixedWalls"),
            "fixedWalls { type nutkWallFunction; value uniform 0; }"
        );
    }
}

#[test]
fn laminar_writes_no_turbulence_fields() {
    let s = resolved(&step_json("{}", "100", "100", ""));
    let t = tree(&s);
    for name in ["0/k", "0/omega", "0/epsilon", "0/nut"] {
        assert!(!has_file(&t, name), "{name}");
    }
    assert_eq!(
        file(&t, "constant/turbulenceProperties"),
        "FoamFile\n{\n    version 2.0;\n    format ascii;\n    class dictionary;\n    object turbulenceProperties;\n}\n\nsimulationType laminar;\n"
    );
    assert!(inlet_values_text(&s).is_none());
}

#[test]
fn the_inlet_values_are_the_models_function() {
    let s = step("k_omega_sst", "");
    let (k, omega, epsilon) = expected_values(&s, inlet_speed(&s));
    let t = tree(&s);
    assert!(lines(&file(&t, "0/k")).contains(&format!("internalField uniform {k};").as_str()));
    assert_eq!(
        inlet_values_text(&s).unwrap(),
        format!("k {k}, omega {omega}, epsilon {epsilon}")
    );

    // A reference velocity replaces the speed of the inlet.
    let given = r#","reference_velocity":{"value":10.0,"unit":"m/s"}"#;
    let s = step("k_omega_sst", given);
    let (k, omega, _) = expected_values(&s, 10.0);
    let t = tree(&s);
    assert!(lines(&file(&t, "0/k")).contains(&format!("internalField uniform {k};").as_str()));
    assert!(
        lines(&file(&t, "0/omega")).contains(&format!("internalField uniform {omega};").as_str())
    );
}

#[test]
fn the_schemes_and_solvers_carry_the_model() {
    let sst = tree(&step("k_omega_sst", ""));
    let schemes = file(&sst, "system/fvSchemes");
    let found = lines(&schemes);
    for want in [
        "ddtSchemes { default steadyState; }",
        "div(phi,U) bounded Gauss linearUpwind grad(U);",
        "div(phi,k) bounded Gauss limitedLinear 1;",
        "div(phi,omega) bounded Gauss limitedLinear 1;",
        "wallDist { method meshWave; }",
        "laplacianSchemes { default Gauss linear corrected; }",
        "snGradSchemes { default corrected; }",
    ] {
        assert!(found.contains(&want), "{want}\n{schemes}");
    }
    assert!(!schemes.contains("epsilon"));

    let solution = file(&sst, "system/fvSolution");
    assert_eq!(
        block(&solution, "p"),
        [
            "p",
            "{",
            "solver GAMG;",
            "smoother DICGaussSeidel;",
            "tolerance 1e-10;",
            "relTol 0.1;",
            "}"
        ]
    );
    assert_eq!(
        block(&solution, "\"(U|k|omega)\""),
        [
            "\"(U|k|omega)\"",
            "{",
            "solver smoothSolver;",
            "smoother symGaussSeidel;",
            "tolerance 1e-10;",
            "relTol 0.1;",
            "}"
        ]
    );
    assert_eq!(
        block(&solution, "SIMPLE"),
        [
            "SIMPLE",
            "{",
            "nNonOrthogonalCorrectors 0;",
            "consistent yes;",
            "}"
        ]
    );
    assert!(lines(&solution).contains(&"equations { U 0.9; \".*\" 0.8; }"));

    for model in ["realizable_ke", "rng_k_epsilon"] {
        let t = tree(&step(model, ""));
        let schemes = file(&t, "system/fvSchemes");
        assert!(
            lines(&schemes).contains(&"div(phi,epsilon) bounded Gauss limitedLinear 1;"),
            "{model}"
        );
        assert!(!schemes.contains("omega") && !schemes.contains("wallDist"));
        let solution = file(&t, "system/fvSolution");
        assert_eq!(
            block(&solution, "\"(U|k|epsilon)\"").len(),
            7,
            "{model}\n{solution}"
        );
    }

    // The steady laminar cavity keeps the dictionaries it always had.
    let cavity = Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap();
    let cavity = resolve(&cavity).unwrap();
    let t = tree(&cavity);
    assert_eq!(file(&t, "system/fvSchemes"), control_dict::fv_schemes());
    assert_eq!(
        file(&t, "system/fvSolution"),
        control_dict::fv_solution(cavity.domain.open)
    );
}

#[test]
fn every_turbulent_tree_is_guard_clean_and_has_no_directive() {
    for model in ["laminar", "k_omega_sst", "realizable_ke", "rng_k_epsilon"] {
        let t = if model == "laminar" {
            tree(&resolved(&step_json("{}", "100", "100", "")))
        } else {
            tree(&step(model, ""))
        };
        assert_clean(&t);
    }
}

#[test]
fn the_step_sst_files_equal_their_goldens() {
    let t = tree(&step("k_omega_sst", ""));
    let difference = golden_differences(
        "step-sst-v2512",
        &t,
        &[
            "0/U",
            "0/k",
            "0/omega",
            "0/nut",
            "constant/turbulenceProperties",
            "system/fvSchemes",
            "system/fvSolution",
        ],
    );
    assert!(difference.is_empty(), "{difference}");
}
