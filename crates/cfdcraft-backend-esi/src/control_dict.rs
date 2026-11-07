use crate::{dictionary, monitors::functions_block, scalar, solver_plan::SolverPlan};
use cfdcraft_model::ResolvedStudy;

pub fn control_dict(s: &ResolvedStudy) -> String {
    let x = scalar(s.domain.length_x);
    let y = scalar(s.domain.length_y);
    let cx = scalar(s.domain.length_x / 2.0);
    let cy = scalar(s.domain.length_y / 2.0);
    let cz = scalar(s.domain.length_z / 2.0);
    let end = scalar(s.solver.end_time);
    let interval = scalar(s.solver.write_interval);
    let samples = s.monitors.centreline_u_sample_count;
    let monitors = functions_block(s);
    let plan = SolverPlan::for_study(s);
    let binary = &plan.binary;
    // A steady run counts iterations; a transient one takes a fixed step and writes on the clock.
    let timing = match (plan.transient, s.effective_delta_t()) {
        (true, Some(step)) => format!(
            "deltaT {};\nadjustTimeStep no;\nwriteControl runTime;",
            scalar(step)
        ),
        _ => "deltaT 1;\nwriteControl timeStep;".to_string(),
    };
    dictionary(
        "controlDict",
        "dictionary",
        &format!(
            "application {binary};\nstartFrom startTime;\nstartTime 0;\nstopAt endTime;\nendTime {end};\n{timing}\nwriteInterval {interval};\npurgeWrite 2;\nwriteFormat ascii;\nwritePrecision 12;\nwriteCompression off;\ntimeFormat general;\ntimePrecision 12;\nrunTimeModifiable true;\n\nfunctions\n{{\n    centreline\n    {{\n        type sets;\n        libs (sampling);\n        writeControl writeTime;\n        setFormat raw;\n        interpolationScheme cellPoint;\n        fields (U);\n        sets\n        (\n            vertical\n            {{\n                type uniform;\n                axis y;\n                start ({cx} 0 {cz});\n                end ({cx} {y} {cz});\n                nPoints {samples};\n            }}\n            horizontal\n            {{\n                type uniform;\n                axis x;\n                start (0 {cy} {cz});\n                end ({x} {cy} {cz});\n                nPoints {samples};\n            }}\n        );\n    }}\n{monitors}}}\n"
        ),
    )
}

pub fn fv_schemes() -> String {
    dictionary(
        "fvSchemes",
        "dictionary",
        "ddtSchemes { default steadyState; }\ngradSchemes { default Gauss linear; }\ndivSchemes\n{\n    default none;\n    div(phi,U) Gauss linear;\n    div((nuEff*dev2(T(grad(U))))) Gauss linear;\n}\nlaplacianSchemes { default Gauss linear orthogonal; }\ninterpolationSchemes { default linear; }\nsnGradSchemes { default orthogonal; }\nfluxRequired { default no; p; }\n",
    )
}

/// The solver entry of the potential equation, inside the solvers block.
pub(crate) const PHI_SOLVER: &str = "    Phi\n    {\n        solver GAMG;\n        smoother DIC;\n        tolerance 1e-06;\n        relTol 0.01;\n    }\n";
/// The controls of the potential-flow step.
pub(crate) const POTENTIAL_FLOW: &str = "potentialFlow\n{\n    nNonOrthogonalCorrectors 2;\n}\n";

pub fn fv_solution(open: bool) -> String {
    fv_solution_with(open, &SolverPlan::steady_laminar())
}

/// The solution dictionary of a plan. A potential-flow start adds the solver of the potential
/// equation and the controls of the potential-flow step.
pub fn fv_solution_with(open: bool, plan: &SolverPlan) -> String {
    // A closed domain has no fixed-pressure boundary, so the pressure level needs a reference.
    let reference = if open {
        ""
    } else {
        "    pRefCell 0;\n    pRefValue 0;\n"
    };
    let potential_solver = if plan.potential { PHI_SOLVER } else { "" };
    let potential_flow = if plan.potential { POTENTIAL_FLOW } else { "" };
    let body = [
        "solvers\n{\n    p\n    {\n        solver PCG;\n        preconditioner DIC;\n        tolerance 1e-10;\n        relTol 0;\n    }\n    U\n    {\n        solver smoothSolver;\n        smoother symGaussSeidel;\n        tolerance 1e-10;\n        relTol 0;\n    }\n",
        potential_solver,
        "}\nSIMPLE\n{\n    nNonOrthogonalCorrectors 0;\n",
        reference,
        "    consistent no;\n}\n",
        potential_flow,
        "relaxationFactors\n{\n    fields { p 0.3; }\n    equations { U 0.7; }\n}\n",
    ]
    .concat();
    dictionary("fvSolution", "dictionary", &body)
}
