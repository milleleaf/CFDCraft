//! A parabolic inlet is written as a polynomial profile and never as code; the cylinder's transient
//! numerics and a transient turbulent case are written in full. The steady cases keep their text.

mod field_studies;

use cfdcraft_backend_esi::control_dict;
use cfdcraft_model::{ResolvedShape, ResolvedStudy, Study, resolve};
use field_studies::{
    assert_clean, block, cylinder_json, entry, file, golden_differences, has_file, heated_json,
    lines, resolved, step_json, tree,
};

const TRANSIENT: &str = r#"{"time":{"mode":"transient","delta_t":{"value":0.001,"unit":"s"}}}"#;
const PARABOLIC: &str = r#","profile":"parabolic""#;

fn cylinder() -> ResolvedStudy {
    resolved(&cylinder_json(TRANSIENT, "8", "0.5", PARABOLIC))
}

/// The numbers of a text with the brackets taken out.
fn numbers(text: &str) -> Vec<f64> {
    text.replace(['(', ')', ';'], " ")
        .split_whitespace()
        .map(|t| t.parse().unwrap())
        .collect()
}

/// The coefficients and the origin of the profile in an inlet entry.
fn profile(entry: &str) -> (f64, f64, f64) {
    assert!(entry.contains("type fixedProfile;"), "{entry}");
    assert!(entry.contains("direction (0 1 0);"), "{entry}");
    let table = entry
        .split("polynomial ")
        .nth(1)
        .unwrap()
        .split("; direction")
        .next()
        .unwrap();
    let n = numbers(table);
    assert_eq!(n.len(), 12, "{table}");
    // Each term is (coefficient 0 0) (exponent 0 0); the exponents are 1 and 2.
    assert_eq!([n[3], n[9]], [1.0, 2.0]);
    assert_eq!([n[1], n[2], n[4], n[5], n[7], n[8], n[10], n[11]], [0.0; 8]);
    let origin = entry
        .split("origin ")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    (n[0], n[6], origin.parse().unwrap())
}

fn close(found: f64, wanted: f64) {
    assert!(
        (found - wanted).abs() <= 1e-12 * wanted.abs().max(1.0),
        "{found} against {wanted}"
    );
}

#[test]
fn the_parabolic_inlet_is_a_polynomial_profile() {
    // The channel: mean speed 1 m/s over a height of 0.41 m.
    let t = tree(&cylinder());
    let (linear, quadratic, origin) = profile(&entry(&t, "0/U", "inlet"));
    close(linear, 6.0 / 0.41);
    close(quadratic, -6.0 / (0.41 * 0.41));
    assert_eq!(origin, 0.0);
    // The profile peaks at 1.5 times the mean in the middle of the inlet.
    let middle = 0.41 / 2.0;
    close(linear * middle + quadratic * middle * middle, 1.5);
    close(linear * 0.41 + quadratic * 0.41 * 0.41, 0.0);

    // The step: the profile starts at the top of the step and spans the narrow channel.
    let s = resolved(&step_json("{}", "100", "100", PARABOLIC));
    let Some(ResolvedShape::BackwardStep { step_height, .. }) = s.domain.shape else {
        panic!("not a step");
    };
    let height = s.domain.length_y - step_height;
    let speed = s.boundary_conditions["inlet"].velocity.unwrap();
    let (linear, quadratic, origin) = profile(&entry(&tree(&s), "0/U", "inlet"));
    close(linear, 6.0 * speed / height);
    close(quadratic, -6.0 * speed / (height * height));
    close(origin, step_height);
    close(
        linear * (height / 2.0) + quadratic * (height / 2.0).powi(2),
        1.5 * speed,
    );

    // An inlet on the right face flows the other way, so the coefficients change sign.
    let right = r#"{"schema_version":1,"physics":{},"domain":{"length_x":{"value":1.0,"unit":"m"},"length_y":{"value":0.2,"unit":"m"},"length_z":{"value":0.05,"unit":"m"},"open":true},"fluid":{"kinematic_viscosity":{"value":0.001,"unit":"m2/s"}},"boundary_conditions":{"inlet":{"faces":["XMax"],"kind":"VelocityInlet","velocity":{"value":1.0,"unit":"m/s"},"profile":"parabolic"},"outlet":{"faces":["XMin"],"kind":"PressureOutlet"},"walls":{"faces":["YMin","YMax"],"kind":"NoSlipWall"}},"mesh":{"cells_x":40,"cells_y":20,"cells_z":1},"solver":{"solver":"FromPhysics","end_time":{"value":100,"unit":"s"},"write_interval":{"value":100,"unit":"s"},"convergence":{"p":1e-06,"U":1e-06}},"monitors":{"centreline_u_sample_count":64}}"#;
    let (linear, quadratic, origin) = profile(&entry(&tree(&resolved(right)), "0/U", "inlet"));
    close(linear, -30.0);
    close(quadratic, 150.0);
    assert_eq!(origin, 0.0);

    // A uniform inlet is still a fixed value.
    let uniform = tree(&resolved(&cylinder_json(TRANSIENT, "8", "0.5", "")));
    let line = entry(&uniform, "0/U", "inlet");
    assert!(
        line.contains("type fixedValue;") && !line.contains("fixedProfile"),
        "{line}"
    );
}

#[test]
fn the_cylinder_tree_has_no_code_and_passes_the_guard() {
    assert_clean(&tree(&cylinder()));
    let source = include_str!("../src/transport.rs");
    assert!(!source.contains("codedFixedValue") && !source.contains("codeStream"));
}

#[test]
fn the_cylinder_numerics() {
    let t = tree(&cylinder());
    let schemes = file(&t, "system/fvSchemes");
    let found = lines(&schemes);
    for want in [
        "ddtSchemes { default backward; }",
        "div(phi,U) Gauss linear;",
        "laplacianSchemes { default Gauss linear corrected; }",
        "snGradSchemes { default corrected; }",
    ] {
        assert!(found.contains(&want), "{want}\n{schemes}");
    }
    let solution = file(&t, "system/fvSolution");
    assert_eq!(
        block(&solution, "PIMPLE"),
        [
            "PIMPLE",
            "{",
            "momentumPredictor yes;",
            "nOuterCorrectors 1;",
            "nCorrectors 2;",
            "nNonOrthogonalCorrectors 1;",
            "}"
        ]
    );
    assert!(!solution.contains("pRefCell"));
    for name in ["\"(p)\"", "\"(p)Final\"", "\"(U)\"", "\"(U)Final\""] {
        assert_eq!(block(&solution, name).len(), 7, "{name}\n{solution}");
    }

    // Only the cylinder shape gets a non-orthogonal corrector.
    let step = tree(&resolved(&step_json(TRANSIENT, "0.5", "0.25", "")));
    let solution = file(&step, "system/fvSolution");
    assert!(block(&solution, "PIMPLE").contains(&"nNonOrthogonalCorrectors 0;".to_owned()));
}

#[test]
fn a_transient_turbulent_case_has_its_scalars_in_the_solver_regex() {
    let physics = r#"{"time":{"mode":"transient","delta_t":{"value":0.001,"unit":"s"}},"turbulence":{"model":"k_omega_sst","intensity":0.05,"viscosity_ratio":10.0}}"#;
    let t = tree(&resolved(&step_json(physics, "0.5", "0.25", "")));
    let schemes = file(&t, "system/fvSchemes");
    let found = lines(&schemes);
    for want in [
        "ddtSchemes { default backward; }",
        "div(phi,k) Gauss limitedLinear 1;",
        "div(phi,omega) Gauss limitedLinear 1;",
        "wallDist { method meshWave; }",
    ] {
        assert!(found.contains(&want), "{want}\n{schemes}");
    }
    let solution = file(&t, "system/fvSolution");
    for name in ["\"(U|k|omega)\"", "\"(U|k|omega)Final\""] {
        assert_eq!(block(&solution, name).len(), 7, "{name}\n{solution}");
    }
    // The same fields as the steady run of the model.
    let steady = tree(&resolved(&step_json(
        r#"{"turbulence":{"model":"k_omega_sst","intensity":0.05,"viscosity_ratio":10.0}}"#,
        "100",
        "100",
        "",
    )));
    for name in ["0/U", "0/k", "0/omega", "0/nut"] {
        assert!(has_file(&t, name), "{name}");
        assert_eq!(file(&t, name), file(&steady, name), "{name}");
    }
    assert!(!has_file(&t, "0/epsilon"));
    assert_clean(&t);
}

#[test]
fn the_cylinder_files_equal_their_goldens() {
    let t = tree(&cylinder());
    let difference = golden_differences(
        "cylinder-v2512",
        &t,
        &["0/U", "system/fvSchemes", "system/fvSolution"],
    );
    assert!(difference.is_empty(), "{difference}");
}

#[test]
fn the_steady_cases_are_unchanged() {
    // The lid-driven cavity keeps its dictionaries.
    let cavity = resolve(
        &Study::from_json(include_str!(
            "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
        ))
        .unwrap(),
    )
    .unwrap();
    let t = tree(&cavity);
    assert_eq!(file(&t, "system/fvSchemes"), control_dict::fv_schemes());
    assert_eq!(
        file(&t, "system/fvSolution"),
        control_dict::fv_solution(cavity.domain.open)
    );
    // The steady buoyant and turbulent runs march to a steady state and have no time scheme.
    let heated = tree(&resolved(&heated_json("", "1000", "100")));
    let step = tree(&resolved(&step_json(
        r#"{"turbulence":{"model":"k_omega_sst","intensity":0.05,"viscosity_ratio":10.0}}"#,
        "100",
        "100",
        "",
    )));
    for t in [heated, step] {
        let schemes = file(&t, "system/fvSchemes");
        assert!(schemes.contains("steadyState") && !schemes.contains("backward"));
        let solution = file(&t, "system/fvSolution");
        assert!(solution.contains("SIMPLE") && !solution.contains("PIMPLE"));
    }
}
