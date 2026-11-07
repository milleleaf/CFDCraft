//! The numerics files: the schemes and the solver settings.
//!
//! One `PIMPLE` block serves both time modes; the steady state is chosen by the time scheme. A
//! steady laminar study without buoyancy keeps the numerics of the older line's cavity (central
//! differencing, orthogonal correction), so the two lines can be compared. A transient run, a
//! turbulence model or buoyancy takes the numerics of the older line's runs of the same kind, in
//! the vocabulary of this line. Only keyword names and numbers are written, assembled from typed
//! values.

use crate::{
    markers::{
        ENERGY_FIELD, STRESS_DIVERGENCE_SCHEME, STRESS_DIVERGENCE_SCHEME_FLUID,
        TRANSIENT_TIME_SCHEME,
    },
    turbulence::{model_of, transported},
};
use cfdcraft_backend_esi::{
    control_dict::{PHI_SOLVER, POTENTIAL_FLOW},
    dictionary,
};
use cfdcraft_model::{InitMode, ResolvedShape, ResolvedStudy, ResolvedTime, TurbulenceModel};

/// What the numerics of a study depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Numerics {
    pub transient: bool,
    pub buoyant: bool,
    pub model: TurbulenceModel,
    /// A potential-flow start adds the solver of the potential equation and its controls.
    pub potential: bool,
    /// An open domain has a fixed-pressure boundary, so no pressure reference.
    pub open: bool,
    /// The extra pressure corrections of a mesh that is not orthogonal (the cylinder's).
    pub non_orthogonal: u32,
}

impl Numerics {
    pub fn of(s: &ResolvedStudy) -> Self {
        let physics = s.physics.as_ref();
        Self {
            transient: physics.is_some_and(|p| matches!(p.time, ResolvedTime::Transient { .. })),
            buoyant: physics.is_some_and(|p| p.buoyancy.is_some()),
            model: model_of(s),
            potential: physics.is_some_and(|p| p.init == InitMode::Potential),
            open: s.domain.open,
            non_orthogonal: u32::from(matches!(
                s.domain.shape,
                Some(ResolvedShape::CylinderChannel { .. })
            )),
        }
    }

    /// Steady, laminar and without buoyancy: the cavity's numerics.
    fn plain(&self) -> bool {
        !self.transient && !self.buoyant && self.model == TurbulenceModel::Laminar
    }
}

/// `system/fvSchemes`.
pub fn fv_schemes(n: &Numerics) -> String {
    if n.plain() {
        plain_schemes()
    } else {
        general_schemes(n)
    }
}

/// `system/fvSolution`.
pub fn fv_solution(n: &Numerics) -> String {
    if n.plain() {
        plain_solution(n.open, n.potential)
    } else {
        general_solution(n)
    }
}

fn plain_schemes() -> String {
    dictionary(
        "fvSchemes",
        "dictionary",
        &format!(
            "ddtSchemes {{ default steadyState; }}\ngradSchemes {{ default Gauss linear; }}\ndivSchemes\n{{\n    default none;\n    div(phi,U) Gauss linear;\n    {STRESS_DIVERGENCE_SCHEME} Gauss linear;\n}}\nlaplacianSchemes {{ default Gauss linear orthogonal; }}\ninterpolationSchemes {{ default linear; }}\nsnGradSchemes {{ default orthogonal; }}\n"
        ),
    )
}

/// One solver entry, written out in full.
fn solver(name: &str, settings: &str) -> String {
    format!("    {name}\n    {{\n{settings}    }}\n")
}

/// A closed domain has no fixed-pressure boundary, so the pressure level needs a reference; an
/// open domain has none.
fn pressure_reference(open: bool) -> &'static str {
    if open {
        ""
    } else {
        "    pRefCell 0;\n    pRefValue 0;\n"
    }
}

/// The solution settings of the cavity: one corrector, the final-iteration entries repeating the
/// plain ones.
fn plain_solution(open: bool, potential: bool) -> String {
    let pressure = "        solver PCG;\n        preconditioner DIC;\n        tolerance 1e-10;\n        relTol 0;\n";
    let velocity = "        solver smoothSolver;\n        smoother symGaussSeidel;\n        tolerance 1e-10;\n        relTol 0;\n";
    let reference = pressure_reference(open);
    let solvers = [
        solver("p", pressure),
        solver("pFinal", pressure),
        solver("U", velocity),
        solver("UFinal", velocity),
    ]
    .concat();
    let potential_solver = if potential { PHI_SOLVER } else { "" };
    let potential_flow = if potential { POTENTIAL_FLOW } else { "" };
    dictionary(
        "fvSolution",
        "dictionary",
        &format!(
            "solvers\n{{\n{solvers}{potential_solver}}}\nPIMPLE\n{{\n    nOuterCorrectors 1;\n    nCorrectors 1;\n    nNonOrthogonalCorrectors 0;\n    consistent yes;\n{reference}}}\n{potential_flow}relaxationFactors\n{{\n    fields {{ p 0.3; }}\n    equations {{ U 0.7; }}\n}}\n"
        ),
    )
}

fn general_schemes(n: &Numerics) -> String {
    let ddt = if n.transient {
        TRANSIENT_TIME_SCHEME
    } else {
        "steadyState"
    };
    // A steady run bounds the convection of every transported quantity.
    let bounded = if n.transient { "" } else { "bounded " };
    let velocity = if n.transient && !n.buoyant && n.model == TurbulenceModel::Laminar {
        "Gauss linear".to_owned()
    } else {
        format!("{bounded}Gauss linearUpwind grad(U)")
    };
    let mut divergence = format!("    div(phi,U) {velocity};\n");
    if n.buoyant {
        divergence.push_str(&format!(
            "    div(phi,{ENERGY_FIELD}) {bounded}Gauss linearUpwind grad({ENERGY_FIELD});\n    div(phi,K) {bounded}Gauss linear;\n    div(phiv,p) {bounded}Gauss linear;\n"
        ));
    }
    for name in transported(n.model) {
        divergence.push_str(&format!(
            "    div(phi,{name}) {bounded}Gauss limitedLinear 1;\n"
        ));
    }
    let stress = if n.buoyant {
        STRESS_DIVERGENCE_SCHEME_FLUID
    } else {
        STRESS_DIVERGENCE_SCHEME
    };
    divergence.push_str(&format!("    {stress} Gauss linear;\n"));
    let wall_distance = if n.model == TurbulenceModel::KOmegaSst {
        "wallDist { method meshWave; }\n"
    } else {
        ""
    };
    dictionary(
        "fvSchemes",
        "dictionary",
        &format!(
            "ddtSchemes {{ default {ddt}; }}\ngradSchemes {{ default Gauss linear; }}\ndivSchemes\n{{\n    default none;\n{divergence}}}\nlaplacianSchemes {{ default Gauss linear corrected; }}\ninterpolationSchemes {{ default linear; }}\nsnGradSchemes {{ default corrected; }}\n{wall_distance}"
        ),
    )
}

/// One linear solver entry: its name, the solver, the preconditioner or smoother with its keyword,
/// the absolute tolerance and the relative one.
fn solver_entry(
    name: &str,
    kind: &str,
    (option, value): (&str, &str),
    tolerance: &str,
    relative: &str,
) -> String {
    format!(
        "    {name}\n    {{\n        solver {kind};\n        {option} {value};\n        tolerance {tolerance};\n        relTol {relative};\n    }}\n"
    )
}

fn general_solution(n: &Numerics) -> String {
    let reference = pressure_reference(n.open);
    // The quantities the second linear solver covers, as a regular expression.
    let mut others = vec!["U"];
    if n.buoyant {
        others.push(ENERGY_FIELD);
    }
    others.extend(transported(n.model));
    let others = format!("({})", others.join("|"));
    let (pressure_solver, other_solver) = if n.buoyant {
        (
            ("PCG", ("preconditioner", "DIC")),
            ("PBiCGStab", ("preconditioner", "DILU")),
        )
    } else {
        (
            ("GAMG", ("smoother", "DICGaussSeidel")),
            ("smoothSolver", ("smoother", "symGaussSeidel")),
        )
    };
    let mut solvers = String::new();
    if n.transient {
        let tolerance = if n.buoyant { "1e-08" } else { "1e-06" };
        let pressure_relative = if n.buoyant { "0.01" } else { "0.05" };
        let (name, last) = if n.buoyant {
            ("p_rgh".to_owned(), "p_rghFinal".to_owned())
        } else {
            ("\"(p)\"".to_owned(), "\"(p)Final\"".to_owned())
        };
        solvers.push_str(&solver_entry(
            &name,
            pressure_solver.0,
            pressure_solver.1,
            tolerance,
            pressure_relative,
        ));
        solvers.push_str(&solver_entry(
            &last,
            pressure_solver.0,
            pressure_solver.1,
            tolerance,
            "0",
        ));
        for suffix in ["", "Final"] {
            solvers.push_str(&solver_entry(
                &format!("\"{others}{suffix}\""),
                other_solver.0,
                other_solver.1,
                tolerance,
                "0",
            ));
        }
    } else {
        let pressure_relative = if n.buoyant { "0.01" } else { "0.1" };
        let pressure_name = if n.buoyant { "p_rgh" } else { "p" };
        solvers.push_str(&solver_entry(
            pressure_name,
            pressure_solver.0,
            pressure_solver.1,
            "1e-10",
            pressure_relative,
        ));
        solvers.push_str(&solver_entry(
            &format!("\"{others}\""),
            other_solver.0,
            other_solver.1,
            "1e-10",
            "0.1",
        ));
    }
    if n.buoyant {
        // The density is explicit in the Boussinesq form: its equation has no linear system.
        solvers.push_str("    \"rho.*\"\n    {\n        solver diagonal;\n    }\n");
    }
    if n.potential {
        solvers.push_str(PHI_SOLVER);
    }
    let non_orthogonal = n.non_orthogonal;
    let algorithm = if n.transient {
        let predictor = if n.buoyant { "no" } else { "yes" };
        format!(
            "PIMPLE\n{{\n    momentumPredictor {predictor};\n    nOuterCorrectors 1;\n    nCorrectors 2;\n    nNonOrthogonalCorrectors {non_orthogonal};\n{reference}}}\n"
        )
    } else {
        let consistent = if n.buoyant {
            ""
        } else {
            "    consistent yes;\n"
        };
        format!(
            "PIMPLE\n{{\n    nOuterCorrectors 1;\n    nCorrectors 1;\n    nNonOrthogonalCorrectors {non_orthogonal};\n{consistent}{reference}}}\n"
        )
    };
    let potential_flow = if n.potential { POTENTIAL_FLOW } else { "" };
    let relaxation = match (n.transient, n.buoyant) {
        (true, _) => String::new(),
        (false, true) => format!(
            "relaxationFactors\n{{\n    fields {{ p_rgh 0.7; }}\n    equations {{ U 0.3; {ENERGY_FIELD} 0.5; \".*\" 0.5; }}\n}}\n"
        ),
        (false, false) => {
            "relaxationFactors\n{\n    equations { U 0.9; \".*\" 0.8; }\n}\n".to_owned()
        }
    };
    dictionary(
        "fvSolution",
        "dictionary",
        &format!("solvers\n{{\n{solvers}}}\n{algorithm}{potential_flow}{relaxation}"),
    )
}
