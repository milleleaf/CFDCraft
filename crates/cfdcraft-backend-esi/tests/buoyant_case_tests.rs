//! Boussinesq buoyancy is written with its transport values, gravity, the fields the buoyant
//! solvers read, a patch for each fixed wall temperature, and numerics for steady and transient
//! runs. The study is a closed square cavity heated from the left and cooled from the right.

mod field_studies;

use cfdcraft_backend_esi::{CompileError, Dialect, compile, patches::patch_of_label};
use cfdcraft_model::ResolvedStudy;
use field_studies::{
    assert_clean, block, entry, file, golden_differences, has_file, heated_json, heated_walls_json,
    lines, num, resolved, step_json, tree,
};

const TRANSIENT: &str = r#""time":{"mode":"transient","delta_t":{"value":0.001,"unit":"s"}},"#;
const SST: &str = r#""turbulence":{"model":"k_omega_sst","intensity":0.05,"viscosity_ratio":10.0,"reference_velocity":{"value":0.1,"unit":"m/s"}},"#;

fn steady() -> ResolvedStudy {
    resolved(&heated_json("", "1000", "100"))
}

/// The names of the patches in a mesh dictionary, in the order they are written.
fn patch_names(dict: &str) -> Vec<String> {
    let section = dict.split("\nboundary\n").nth(1).unwrap();
    section
        .lines()
        .filter(|l| l.starts_with("    ") && l.chars().nth(4).is_some_and(char::is_alphabetic))
        .map(|l| l.trim().to_owned())
        .collect()
}

/// The face list text of a patch of a mesh dictionary.
fn patch_faces(dict: &str, name: &str) -> String {
    let section = dict.split("\nboundary\n").nth(1).unwrap();
    let mut inside = false;
    for line in section.lines() {
        let text = line.trim();
        if line.starts_with("    ") && line.chars().nth(4).is_some_and(char::is_alphabetic) {
            inside = text == name;
        } else if inside && let Some(f) = text.strip_prefix("faces ") {
            return f.trim_end_matches(';').to_owned();
        }
    }
    panic!("no patch {name}");
}

#[test]
fn isothermal_walls_are_their_own_patches() {
    let s = steady();
    let dict = file(&tree(&s), "system/blockMeshDict");
    assert_eq!(
        patch_names(&dict),
        ["fixedWalls", "isothermal1", "isothermal2", "frontAndBack"]
    );
    assert_eq!(patch_of_label(&s, "cold"), Some("isothermal1"));
    assert_eq!(patch_of_label(&s, "hot"), Some("isothermal2"));
    assert_eq!(patch_of_label(&s, "adiabatic"), Some("fixedWalls"));
    // The right face holds the colder wall, the left face the warmer one.
    assert_eq!(patch_faces(&dict, "isothermal1").matches('(').count(), 2);
    assert_ne!(
        patch_faces(&dict, "isothermal1"),
        patch_faces(&dict, "isothermal2")
    );

    // Two walls at one temperature share a patch.
    let shared = resolved(&heated_walls_json("", "1000", "100", "300.0", "300.0"));
    let dict = file(&tree(&shared), "system/blockMeshDict");
    assert_eq!(
        patch_names(&dict),
        ["fixedWalls", "isothermal1", "frontAndBack"]
    );
    assert_eq!(patch_faces(&dict, "isothermal1").matches('(').count(), 3);
    assert_eq!(patch_of_label(&shared, "hot"), Some("isothermal1"));
    assert_eq!(patch_of_label(&shared, "cold"), Some("isothermal1"));

    // A fixed temperature on a wall that is not a no-slip wall is refused when the case is written.
    let slip = heated_json("", "1000", "100").replace(
        r#""hot":{"faces":["XMin"],"kind":"NoSlipWall""#,
        r#""hot":{"faces":["XMin"],"kind":"SlipWall""#,
    );
    assert!(matches!(
        compile(&resolved(&slip), Dialect::EsiV2512),
        Err(CompileError::Invalid(_))
    ));
}

#[test]
fn the_buoyant_transport_and_gravity_files() {
    let s = steady();
    let t = tree(&s);
    let transport = file(&t, "constant/transportProperties");
    let found = lines(&transport);
    for want in [
        "transportModel Newtonian;".to_owned(),
        format!("nu [0 2 -1 0 0 0 0] {};", num(s.fluid.kinematic_viscosity)),
        "beta [0 0 0 -1 0 0 0] 0.003;".to_owned(),
        "TRef [0 0 0 1 0 0 0] 300;".to_owned(),
        "Pr [0 0 0 0 0 0 0] 0.71;".to_owned(),
        "Prt [0 0 0 0 0 0 0] 0.85;".to_owned(),
    ] {
        assert!(found.contains(&want.as_str()), "{want}\n{transport}");
    }
    let gravity = file(&t, "constant/g");
    let found = lines(&gravity);
    for want in [
        "class uniformDimensionedVectorField;",
        "object g;",
        "dimensions [0 1 -2 0 0 0 0];",
        "value (0 -9.81 0);",
    ] {
        assert!(found.contains(&want), "{want}\n{gravity}");
    }

    // A study without buoyancy writes neither the extra values nor the gravity file.
    let plain = tree(&resolved(&step_json("{}", "100", "100", "")));
    assert!(!has_file(&plain, "constant/g"));
    let transport = file(&plain, "constant/transportProperties");
    assert_eq!(transport.lines().count(), 10);
    assert!(!transport.contains("beta") && !transport.contains("TRef"));
}

#[test]
fn the_buoyant_fields() {
    let t = tree(&steady());
    let temperature = file(&t, "0/T");
    assert!(lines(&temperature).contains(&"dimensions [0 0 0 1 0 0 0];"));
    assert!(
        lines(&temperature).contains(&format!("internalField uniform {};", num(300.0)).as_str())
    );
    assert_eq!(
        entry(&t, "0/T", "isothermal1"),
        format!(
            "isothermal1 {{ type fixedValue; value uniform {}; }}",
            num(299.5)
        )
    );
    assert_eq!(
        entry(&t, "0/T", "isothermal2"),
        format!(
            "isothermal2 {{ type fixedValue; value uniform {}; }}",
            num(300.5)
        )
    );
    assert_eq!(
        entry(&t, "0/T", "fixedWalls"),
        "fixedWalls { type zeroGradient; }"
    );
    assert_eq!(
        entry(&t, "0/T", "frontAndBack"),
        "frontAndBack { type empty; }"
    );

    let p_rgh = file(&t, "0/p_rgh");
    assert!(lines(&p_rgh).contains(&"dimensions [0 2 -2 0 0 0 0];"));
    for patch in ["fixedWalls", "isothermal1", "isothermal2"] {
        assert_eq!(
            entry(&t, "0/p_rgh", patch),
            format!("{patch} {{ type fixedFluxPressure; rho rhok; value uniform 0; }}")
        );
    }
    assert_eq!(
        entry(&t, "0/p_rgh", "frontAndBack"),
        "frontAndBack { type empty; }"
    );

    for patch in ["fixedWalls", "isothermal1", "isothermal2"] {
        assert_eq!(
            entry(&t, "0/p", patch),
            format!("{patch} {{ type calculated; value uniform 0; }}")
        );
        assert_eq!(
            entry(&t, "0/alphat", patch),
            format!("{patch} {{ type calculated; value uniform 0; }}")
        );
    }
    assert!(lines(&file(&t, "0/alphat")).contains(&"dimensions [0 2 -1 0 0 0 0];"));

    // A turbulence model puts the wall function with the turbulent Prandtl number on the walls.
    let rough = tree(&resolved(&heated_json(SST, "1000", "100")));
    for patch in ["fixedWalls", "isothermal1", "isothermal2"] {
        assert_eq!(
            entry(&rough, "0/alphat", patch),
            format!("{patch} {{ type alphatJayatillekeWallFunction; Prt 0.85; value uniform 0; }}")
        );
    }
    assert!(entry(&rough, "0/k", "isothermal1").contains("kqRWallFunction"));
    assert!(has_file(&rough, "0/omega") && has_file(&rough, "0/nut"));

    // None of the buoyant files is written for a study without buoyancy.
    let plain = tree(&resolved(&step_json("{}", "100", "100", "")));
    for name in ["0/p_rgh", "0/T", "0/alphat"] {
        assert!(!has_file(&plain, name), "{name}");
    }
    assert_eq!(
        entry(&plain, "0/p", "outlet"),
        "outlet { type fixedValue; value uniform 0; }"
    );
}

fn solver_block(
    name: &str,
    solver: &str,
    option: &str,
    tolerance: &str,
    relative: &str,
) -> Vec<String> {
    vec![
        name.to_owned(),
        "{".to_owned(),
        format!("solver {solver};"),
        option.to_owned(),
        format!("tolerance {tolerance};"),
        format!("relTol {relative};"),
        "}".to_owned(),
    ]
}

#[test]
fn the_buoyant_numerics() {
    // Steady.
    let t = tree(&steady());
    let schemes = file(&t, "system/fvSchemes");
    let found = lines(&schemes);
    for want in [
        "ddtSchemes { default steadyState; }",
        "div(phi,U) bounded Gauss linearUpwind grad(U);",
        "div(phi,T) bounded Gauss linearUpwind grad(T);",
        "fluxRequired { default no; p_rgh; }",
    ] {
        assert!(found.contains(&want), "{want}\n{schemes}");
    }
    let solution = file(&t, "system/fvSolution");
    assert_eq!(
        block(&solution, "p_rgh"),
        solver_block("p_rgh", "PCG", "preconditioner DIC;", "1e-10", "0.01")
    );
    assert_eq!(
        block(&solution, "\"(U|T)\""),
        solver_block(
            "\"(U|T)\"",
            "PBiCGStab",
            "preconditioner DILU;",
            "1e-10",
            "0.1"
        )
    );
    assert_eq!(
        block(&solution, "SIMPLE"),
        [
            "SIMPLE",
            "{",
            "nNonOrthogonalCorrectors 0;",
            "pRefCell 0;",
            "pRefValue 0;",
            "}"
        ]
    );
    let found = lines(&solution);
    assert!(found.contains(&"fields { p_rgh 0.7; }"));
    assert!(found.contains(&"equations { U 0.3; T 0.5; \".*\" 0.5; }"));

    // Steady with a turbulence model: its scalars join the schemes and the solver expression.
    let rough = tree(&resolved(&heated_json(SST, "1000", "100")));
    let schemes = file(&rough, "system/fvSchemes");
    assert!(lines(&schemes).contains(&"div(phi,omega) bounded Gauss limitedLinear 1;"));
    assert!(lines(&schemes).contains(&"wallDist { method meshWave; }"));
    assert_eq!(
        block(&file(&rough, "system/fvSolution"), "\"(U|T|k|omega)\"").len(),
        7
    );

    // Transient.
    let t = tree(&resolved(&heated_json(TRANSIENT, "1", "0.5")));
    let schemes = file(&t, "system/fvSchemes");
    let found = lines(&schemes);
    assert!(
        found.contains(&"ddtSchemes { default backward; }"),
        "{schemes}"
    );
    assert!(found.contains(&"div(phi,U) Gauss linearUpwind grad(U);"));
    assert!(found.contains(&"div(phi,T) Gauss linearUpwind grad(T);"));
    let solution = file(&t, "system/fvSolution");
    assert_eq!(
        block(&solution, "PIMPLE"),
        [
            "PIMPLE",
            "{",
            "momentumPredictor no;",
            "nOuterCorrectors 1;",
            "nCorrectors 2;",
            "nNonOrthogonalCorrectors 0;",
            "pRefCell 0;",
            "pRefValue 0;",
            "}"
        ]
    );
    for (name, relative) in [("p_rgh", "0.01"), ("p_rghFinal", "0")] {
        assert_eq!(
            block(&solution, name),
            solver_block(name, "PCG", "preconditioner DIC;", "1e-08", relative)
        );
    }
    for name in ["\"(U|T)\"", "\"(U|T)Final\""] {
        assert_eq!(
            block(&solution, name),
            solver_block(name, "PBiCGStab", "preconditioner DILU;", "1e-08", "0")
        );
    }
    assert!(!solution.contains("relaxationFactors"));
}

#[test]
fn the_buoyant_tree_is_guard_clean() {
    for json in [
        heated_json("", "1000", "100"),
        heated_json(TRANSIENT, "1", "0.5"),
        heated_json(SST, "1000", "100"),
    ] {
        let t = tree(&resolved(&json));
        assert_clean(&t);
        // The author's labels are not written anywhere.
        for f in &t.files {
            let text = String::from_utf8_lossy(&f.bytes);
            for word in ["hot", "cold", "adiabatic"] {
                assert!(!text.contains(word), "{word} in {}", f.rel_path.display());
            }
        }
    }
}

#[test]
fn the_heated_files_equal_their_goldens() {
    let t = tree(&steady());
    let difference = golden_differences(
        "heated-v2512",
        &t,
        &[
            "0/U",
            "0/T",
            "0/p_rgh",
            "0/p",
            "0/alphat",
            "constant/transportProperties",
            "constant/g",
            "constant/turbulenceProperties",
            "system/fvSchemes",
            "system/fvSolution",
        ],
    );
    assert!(difference.is_empty(), "{difference}");
}
