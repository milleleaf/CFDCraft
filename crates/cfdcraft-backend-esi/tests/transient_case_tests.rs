//! A transient study is written for the transient solver with a fixed time step and a run-time
//! write control; a potential-flow start adds its own solver and controls. The steady cavity is
//! checked elsewhere to stay byte for byte what it was.

mod plan_studies;

use cfdcraft_backend_esi::{
    CaseTree, Dialect, SolverPlan, compile,
    export::{allrun_script_with, export_tree_with},
    guard::scan_tree,
};
use plan_studies::{TRANSIENT_ONLY, cavity};
use std::path::Path;

const HEADER: &str = "#!/bin/sh\ncd \"$(dirname \"$0\")\" || exit 1\n";

fn text(tree: &CaseTree, name: &str) -> String {
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    String::from_utf8(file.bytes.clone()).unwrap()
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().map(str::trim).collect()
}

#[test]
fn a_transient_allrun() {
    let study = cavity(TRANSIENT_ONLY, "8", "0.5", 0);
    let plan = SolverPlan::for_study(&study);
    let serial = format!(
        "{HEADER}blockMesh > log.blockMesh 2>&1 || exit 1\ncheckMesh > log.checkMesh 2>&1 || exit 1\npimpleFoam > log.pimpleFoam 2>&1 || exit 1\n"
    );
    assert_eq!(allrun_script_with(&plan, 1), serial);
    let parallel = format!(
        "{HEADER}blockMesh > log.blockMesh 2>&1 || exit 1\ncheckMesh > log.checkMesh 2>&1 || exit 1\ndecomposePar -force > log.decomposePar 2>&1 || exit 1\nmpirun -np 4 pimpleFoam -parallel > log.pimpleFoam 2>&1 || exit 1\nreconstructPar -latestTime > log.reconstructPar 2>&1 || exit 1\n"
    );
    assert_eq!(allrun_script_with(&plan, 4), parallel);

    let potential = SolverPlan::for_study(&cavity(
        r#"{"time": {"mode": "transient", "delta_t": {"value": 0.001, "unit": "s"}}, "init": "potential"}"#,
        "8",
        "0.5",
        0,
    ));
    let with_potential = format!(
        "{HEADER}blockMesh > log.blockMesh 2>&1 || exit 1\ncheckMesh > log.checkMesh 2>&1 || exit 1\npotentialFoam -initialiseUBCs > log.potentialFoam 2>&1 || exit 1\npimpleFoam > log.pimpleFoam 2>&1 || exit 1\n"
    );
    assert_eq!(allrun_script_with(&potential, 1), with_potential);
}

#[test]
fn a_transient_control_dict() {
    let steady = compile(&cavity("{}", "5000", "500", 0), Dialect::EsiV2512).unwrap();
    for (level, step) in [(0, "deltaT 0.001;"), (1, "deltaT 0.0005;")] {
        let tree = compile(
            &cavity(TRANSIENT_ONLY, "8", "0.5", level),
            Dialect::EsiV2512,
        )
        .unwrap();
        let control = text(&tree, "system/controlDict");
        let found = lines(&control);
        for wanted in [
            "application pimpleFoam;",
            step,
            "writeControl runTime;",
            "writeInterval 0.5;",
            "endTime 8.0;",
            "adjustTimeStep no;",
            "purgeWrite 2;",
            "runTimeModifiable true;",
        ] {
            assert!(
                found.contains(&wanted),
                "level {level}: {wanted}\n{control}"
            );
        }
        // The monitors below `functions` write every time step; the run itself writes by run time.
        let head = lines(&control[..control.find("functions").unwrap()]);
        assert!(!head.contains(&"writeControl timeStep;"));
        assert!(scan_tree(&tree).is_empty());
        // The sampled lines and monitors are the same text as in the steady case.
        let steady_control = text(&steady, "system/controlDict");
        let tail = |t: &str| t[t.find("functions").unwrap()..].to_string();
        assert_eq!(tail(&control), tail(&steady_control));
    }
}

#[test]
fn potential_flow_adds_the_phi_solver_and_block() {
    let potential = compile(
        &cavity(r#"{"init": "potential"}"#, "5000", "500", 0),
        Dialect::EsiV2512,
    )
    .unwrap();
    let solution = text(&potential, "system/fvSolution");
    assert!(solution.contains(
        "    Phi\n    {\n        solver GAMG;\n        smoother DIC;\n        tolerance 1e-06;\n        relTol 0.01;\n    }\n"
    ));
    assert!(solution.contains("potentialFlow\n{\n    nNonOrthogonalCorrectors 2;\n}\n"));
    // The Phi solver sits inside the solvers block, before the algorithm block.
    let solvers_end = solution.find("SIMPLE").unwrap();
    assert!(solution.find("Phi").unwrap() < solvers_end);

    let plain = compile(&cavity("{}", "5000", "500", 0), Dialect::EsiV2512).unwrap();
    let solution = text(&plain, "system/fvSolution");
    assert!(!solution.contains("Phi"));
    assert!(!solution.contains("potentialFlow"));
}

#[test]
fn the_potential_case_passes_the_guard() {
    let tree = compile(
        &cavity(r#"{"init": "potential"}"#, "5000", "500", 0),
        Dialect::EsiV2512,
    )
    .unwrap();
    assert!(scan_tree(&tree).is_empty());
    for file in &tree.files {
        let body = String::from_utf8(file.bytes.clone()).unwrap();
        let name = file.rel_path.display();
        assert!(!body.contains('#'), "{name}");
        assert!(!body.contains('$'), "{name}");
        assert!(
            body.split(|c: char| !c.is_alphanumeric())
                .all(|word| !word.starts_with("code")),
            "{name}"
        );
    }
}

#[test]
fn the_potential_allrun_runs_it_before_decomposition() {
    let study = cavity(r#"{"init": "potential"}"#, "5000", "500", 0);
    let plan = SolverPlan::for_study(&study);
    let tree = compile(&study, Dialect::EsiV2512).unwrap();
    let exported = export_tree_with(&tree, "h", 4, &plan);
    let allrun = text(&exported, "Allrun");
    let at = |needle: &str| allrun.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert!(at("checkMesh") < at("potentialFoam -initialiseUBCs"));
    assert!(at("potentialFoam -initialiseUBCs") < at("decomposePar -force"));
    assert!(!allrun.contains("-writep"));
}
