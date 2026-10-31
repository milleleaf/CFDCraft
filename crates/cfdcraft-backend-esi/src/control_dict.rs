use crate::{dictionary, scalar};
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
    dictionary(
        "controlDict",
        "dictionary",
        &format!(
            "application simpleFoam;\nstartFrom startTime;\nstartTime 0;\nstopAt endTime;\nendTime {end};\ndeltaT 1;\nwriteControl timeStep;\nwriteInterval {interval};\npurgeWrite 0;\nwriteFormat ascii;\nwritePrecision 12;\nwriteCompression off;\ntimeFormat general;\ntimePrecision 12;\nrunTimeModifiable false;\n\nfunctions\n{{\n    centreline\n    {{\n        type sets;\n        libs (sampling);\n        writeControl writeTime;\n        setFormat raw;\n        interpolationScheme cellPoint;\n        fields (U);\n        sets\n        (\n            vertical\n            {{\n                type uniform;\n                axis y;\n                start ({cx} 0 {cz});\n                end ({cx} {y} {cz});\n                nPoints {samples};\n            }}\n            horizontal\n            {{\n                type uniform;\n                axis x;\n                start (0 {cy} {cz});\n                end ({x} {cy} {cz});\n                nPoints {samples};\n            }}\n        );\n    }}\n}}\n"
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

pub fn fv_solution() -> String {
    dictionary(
        "fvSolution",
        "dictionary",
        "solvers\n{\n    p\n    {\n        solver PCG;\n        preconditioner DIC;\n        tolerance 1e-10;\n        relTol 0;\n    }\n    U\n    {\n        solver smoothSolver;\n        smoother symGaussSeidel;\n        tolerance 1e-10;\n        relTol 0;\n    }\n}\nSIMPLE\n{\n    nNonOrthogonalCorrectors 0;\n    pRefCell 0;\n    pRefValue 0;\n    consistent no;\n}\nrelaxationFactors\n{\n    fields { p 0.3; }\n    equations { U 0.7; }\n}\n",
    )
}
