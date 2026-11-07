//! The fields, models and numerics a case needs beyond the laminar lid-driven cavity: the
//! turbulence models with their fields and wall functions, the Boussinesq buoyancy fields, and the
//! schemes and solution settings of steady, transient and buoyant runs.
//!
//! Every dictionary is written in full from typed values. Names are fixed words chosen by the
//! model and the patch table; the numbers come from the resolved study, and the three inflow
//! values of a turbulence model from `cfdcraft_model::inlet_turbulence` and nowhere else.

use crate::{
    case::{CompileError, Patch},
    control_dict::{PHI_SOLVER, POTENTIAL_FLOW},
    dictionary, scalar,
    solver_plan::SolverPlan,
    transport::{self, boundary_field},
};
use cfdcraft_model::{
    BoundaryKind, InletTurbulence, ResolvedShape, ResolvedStudy, TurbulenceModel, inlet_turbulence,
};

fn model_of(s: &ResolvedStudy) -> TurbulenceModel {
    s.physics
        .as_ref()
        .map_or(TurbulenceModel::Laminar, |p| p.turbulence.model)
}

/// The transported turbulence quantities of a model, besides the velocity.
fn model_scalars(model: TurbulenceModel) -> &'static [&'static str] {
    match model {
        TurbulenceModel::Laminar => &[],
        TurbulenceModel::KOmegaSst => &["k", "omega"],
        TurbulenceModel::RealizableKe | TurbulenceModel::RngKEpsilon => &["k", "epsilon"],
    }
}

/// The speed the inflow values are built from: the reference velocity of the study, else the
/// speed of a velocity inlet, else the speed of a moving wall.
fn reference_speed(s: &ResolvedStudy) -> Option<f64> {
    let driven = |kind: BoundaryKind| {
        s.boundary_conditions
            .values()
            .filter(|b| b.kind == kind)
            .find_map(|b| b.velocity.filter(|v| *v != 0.0))
            .map(f64::abs)
    };
    s.physics
        .as_ref()?
        .turbulence
        .reference_velocity
        .or_else(|| driven(BoundaryKind::VelocityInlet))
        .or_else(|| driven(BoundaryKind::MovingWall))
}

/// The inflow values of the turbulence fields; `None` for a laminar study or when they cannot be
/// worked out.
pub fn inlet_values(s: &ResolvedStudy) -> Option<InletTurbulence> {
    let t = &s.physics.as_ref()?.turbulence;
    if t.model == TurbulenceModel::Laminar {
        return None;
    }
    inlet_turbulence(
        t.intensity?,
        t.viscosity_ratio?,
        reference_speed(s)?,
        s.fluid.kinematic_viscosity,
    )
}

/// The three inflow values on one line for a report, `None` for a laminar study.
pub fn inlet_values_text(s: &ResolvedStudy) -> Option<String> {
    let v = inlet_values(s)?;
    Some(format!(
        "k {}, omega {}, epsilon {}",
        scalar(v.k),
        scalar(v.omega),
        scalar(v.epsilon)
    ))
}

/// The model and the inflow values the turbulence files are written from; `None` when laminar.
pub fn turbulence_inputs(
    s: &ResolvedStudy,
) -> Result<Option<(TurbulenceModel, InletTurbulence)>, CompileError> {
    let model = model_of(s);
    if model == TurbulenceModel::Laminar {
        return Ok(None);
    }
    inlet_values(s)
        .map(|v| Some((model, v)))
        .ok_or(CompileError::Invalid(
            "the turbulence inflow values cannot be worked out",
        ))
}

pub fn turbulence_properties_for(s: &ResolvedStudy) -> String {
    let word = match model_of(s) {
        TurbulenceModel::Laminar => return transport::turbulence_properties(),
        TurbulenceModel::KOmegaSst => "kOmegaSST",
        TurbulenceModel::RealizableKe => "realizableKE",
        TurbulenceModel::RngKEpsilon => "RNGkEpsilon",
    };
    dictionary(
        "turbulenceProperties",
        "dictionary",
        &format!(
            "simulationType RAS;\nRAS\n{{\n    RASModel {word};\n    turbulence on;\n    printCoeffs on;\n}}\n"
        ),
    )
}

fn scalar_file(object: &str, dimensions: &str, internal: &str, entries: &str) -> String {
    dictionary(
        object,
        "volScalarField",
        &format!(
            "dimensions {dimensions};\ninternalField uniform {internal};\nboundaryField\n{{\n{entries}}}\n"
        ),
    )
}

/// A transported turbulence quantity: held at the inflow value on an inlet, a wall function on a
/// wall, free elsewhere.
fn turbulent_field(
    object: &str,
    dimensions: &str,
    wall_function: &str,
    value: f64,
    patches: &[Patch],
) -> String {
    let v = scalar(value);
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(BoundaryKind::VelocityInlet) => format!("type fixedValue; value uniform {v};"),
        Some(BoundaryKind::PressureOutlet | BoundaryKind::SlipWall) => "type zeroGradient;".into(),
        Some(BoundaryKind::NoSlipWall | BoundaryKind::MovingWall) => {
            format!("type {wall_function}; value uniform {v};")
        }
        Some(BoundaryKind::Symmetry) => format!("type {};", p.symmetry_type()),
    });
    scalar_file(object, dimensions, &v, &entries)
}

pub fn k_field(patches: &[Patch], inlet: &InletTurbulence) -> String {
    turbulent_field("k", "[0 2 -2 0 0 0 0]", "kqRWallFunction", inlet.k, patches)
}

pub fn omega_field(patches: &[Patch], inlet: &InletTurbulence) -> String {
    turbulent_field(
        "omega",
        "[0 0 -1 0 0 0 0]",
        "omegaWallFunction",
        inlet.omega,
        patches,
    )
}

pub fn epsilon_field(patches: &[Patch], inlet: &InletTurbulence) -> String {
    turbulent_field(
        "epsilon",
        "[0 2 -3 0 0 0 0]",
        "epsilonWallFunction",
        inlet.epsilon,
        patches,
    )
}

/// The turbulent viscosity: worked out by the model, so it starts at zero and is only `calculated`
/// away from the walls.
pub fn nut_field(patches: &[Patch], model: TurbulenceModel) -> String {
    let wall_function = if model == TurbulenceModel::KOmegaSst {
        "nutUBlendedWallFunction"
    } else {
        "nutkWallFunction"
    };
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(
            BoundaryKind::VelocityInlet | BoundaryKind::PressureOutlet | BoundaryKind::SlipWall,
        ) => "type calculated; value uniform 0;".into(),
        Some(BoundaryKind::NoSlipWall | BoundaryKind::MovingWall) => {
            format!("type {wall_function}; value uniform 0;")
        }
        Some(BoundaryKind::Symmetry) => format!("type {};", p.symmetry_type()),
    });
    scalar_file("nut", "[0 2 -1 0 0 0 0]", "0", &entries)
}

/// The temperature: held on a wall with a fixed temperature, at the reference temperature on an
/// inlet, free elsewhere.
pub fn t_field(patches: &[Patch], reference: f64) -> String {
    let r = scalar(reference);
    let entries = boundary_field(patches, |p| match (p.kind, p.temperature) {
        (None, _) => "type empty;".into(),
        (Some(BoundaryKind::VelocityInlet), _) => format!("type fixedValue; value uniform {r};"),
        (Some(BoundaryKind::NoSlipWall), Some(t)) => {
            format!("type fixedValue; value uniform {};", scalar(t))
        }
        (Some(BoundaryKind::Symmetry), _) => format!("type {};", p.symmetry_type()),
        (Some(_), _) => "type zeroGradient;".into(),
    });
    scalar_file("T", "[0 0 0 1 0 0 0]", &r, &entries)
}

/// The pressure without the hydrostatic part, which the buoyant solvers solve for.
pub fn p_rgh_field(patches: &[Patch]) -> String {
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(BoundaryKind::PressureOutlet) => "type fixedValue; value uniform 0;".into(),
        Some(BoundaryKind::Symmetry) => format!("type {};", p.symmetry_type()),
        Some(_) => "type fixedFluxPressure; rho rhok; value uniform 0;".into(),
    });
    scalar_file("p_rgh", "[0 2 -2 0 0 0 0]", "0", &entries)
}

/// The pressure the buoyant solver works out from `p_rgh`; it is never read as an input.
pub fn p_calculated_field(patches: &[Patch]) -> String {
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(BoundaryKind::Symmetry) => format!("type {};", p.symmetry_type()),
        Some(_) => "type calculated; value uniform 0;".into(),
    });
    scalar_file("p", "[0 2 -2 0 0 0 0]", "0", &entries)
}

/// The turbulent thermal diffusivity. The buoyant solver reads it even when the flow is laminar.
/// A turbulence model puts a wall function with the turbulent Prandtl number on every wall.
pub fn alphat_field(patches: &[Patch], turbulent_prandtl: Option<f64>) -> String {
    let entries = boundary_field(patches, |p| match (p.kind, turbulent_prandtl) {
        (None, _) => "type empty;".into(),
        (Some(BoundaryKind::Symmetry), _) => format!("type {};", p.symmetry_type()),
        (Some(BoundaryKind::NoSlipWall | BoundaryKind::MovingWall), Some(prt)) => format!(
            "type alphatJayatillekeWallFunction; Prt {}; value uniform 0;",
            scalar(prt)
        ),
        _ => "type calculated; value uniform 0;".into(),
    });
    scalar_file("alphat", "[0 2 -1 0 0 0 0]", "0", &entries)
}

/// The patch fields the buoyant solvers read, as `(file, text)` pairs, in the order they are
/// written. Empty for a study without buoyancy.
pub fn buoyant_files(s: &ResolvedStudy, patches: &[Patch]) -> Vec<(&'static str, String)> {
    let Some(b) = s.physics.as_ref().and_then(|p| p.buoyancy) else {
        return Vec::new();
    };
    let prandtl = (model_of(s) != TurbulenceModel::Laminar).then_some(b.turbulent_prandtl);
    vec![
        ("0/p_rgh", p_rgh_field(patches)),
        ("0/T", t_field(patches, b.reference_temperature)),
        ("0/alphat", alphat_field(patches, prandtl)),
    ]
}

/// The turbulence files of a case, as `(file, text)` pairs, in the order they are written.
pub fn turbulence_files(
    patches: &[Patch],
    model: TurbulenceModel,
    inlet: &InletTurbulence,
) -> Vec<(&'static str, String)> {
    let mut files = vec![("0/k", k_field(patches, inlet))];
    if model == TurbulenceModel::KOmegaSst {
        files.push(("0/omega", omega_field(patches, inlet)));
    } else {
        files.push(("0/epsilon", epsilon_field(patches, inlet)));
    }
    files.push(("0/nut", nut_field(patches, model)));
    files
}

/// True for a study whose numerics are those of the lid-driven cavity since the first release:
/// steady, laminar, without buoyancy and on a plain box.
pub fn has_plain_numerics(s: &ResolvedStudy, plan: &SolverPlan) -> bool {
    !plan.transient
        && !plan.buoyant
        && model_of(s) == TurbulenceModel::Laminar
        && s.domain.shape.is_none()
}

pub fn fv_schemes_for(s: &ResolvedStudy, plan: &SolverPlan) -> String {
    let model = model_of(s);
    let ddt = if plan.transient {
        "backward"
    } else {
        "steadyState"
    };
    // A steady run bounds the convection of every transported quantity.
    let bounded = if plan.transient { "" } else { "bounded " };
    let velocity = if plan.transient && !plan.buoyant && model == TurbulenceModel::Laminar {
        "Gauss linear".to_owned()
    } else {
        format!("{bounded}Gauss linearUpwind grad(U)")
    };
    let mut divergence = format!("    div(phi,U) {velocity};\n");
    if plan.buoyant {
        divergence.push_str(&format!(
            "    div(phi,T) {bounded}Gauss linearUpwind grad(T);\n"
        ));
    }
    for name in model_scalars(model) {
        divergence.push_str(&format!(
            "    div(phi,{name}) {bounded}Gauss limitedLinear 1;\n"
        ));
    }
    divergence.push_str("    div((nuEff*dev2(T(grad(U))))) Gauss linear;\n");
    let wall_distance = if model == TurbulenceModel::KOmegaSst {
        "wallDist { method meshWave; }\n"
    } else {
        ""
    };
    let pressure = &plan.pressure_field;
    dictionary(
        "fvSchemes",
        "dictionary",
        &format!(
            "ddtSchemes {{ default {ddt}; }}\ngradSchemes {{ default Gauss linear; }}\ndivSchemes\n{{\n    default none;\n{divergence}}}\nlaplacianSchemes {{ default Gauss linear corrected; }}\ninterpolationSchemes {{ default linear; }}\nsnGradSchemes {{ default corrected; }}\n{wall_distance}fluxRequired {{ default no; {pressure}; }}\n"
        ),
    )
}

/// One linear solver entry: its name, the solver, the preconditioner or smoother with its keyword,
/// the absolute tolerance and the relative one.
fn solver_entry(
    name: &str,
    solver: &str,
    (option, value): (&str, &str),
    tolerance: &str,
    relative: &str,
) -> String {
    format!(
        "    {name}\n    {{\n        solver {solver};\n        {option} {value};\n        tolerance {tolerance};\n        relTol {relative};\n    }}\n"
    )
}

pub fn fv_solution_for(s: &ResolvedStudy, plan: &SolverPlan) -> String {
    let model = model_of(s);
    let cylinder = matches!(s.domain.shape, Some(ResolvedShape::CylinderChannel { .. }));
    let non_orthogonal = u32::from(cylinder);
    // A closed domain has no fixed-pressure boundary, so the pressure level needs a reference.
    let reference = if s.domain.open {
        ""
    } else {
        "    pRefCell 0;\n    pRefValue 0;\n"
    };
    // The quantities the second linear solver covers, as a regular expression.
    let mut transported = vec!["U"];
    if plan.buoyant {
        transported.push("T");
    }
    transported.extend(model_scalars(model));
    let others = format!("({})", transported.join("|"));
    let (pressure_solver, other_solver) = if plan.buoyant {
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
    if plan.transient {
        let tolerance = if plan.buoyant { "1e-08" } else { "1e-06" };
        let pressure_relative = if plan.buoyant { "0.01" } else { "0.05" };
        let (name, last) = if plan.buoyant {
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
        solvers.push_str(&solver_entry(
            &format!("\"{others}\""),
            other_solver.0,
            other_solver.1,
            tolerance,
            "0",
        ));
        solvers.push_str(&solver_entry(
            &format!("\"{others}Final\""),
            other_solver.0,
            other_solver.1,
            tolerance,
            "0",
        ));
    } else {
        let pressure_relative = if plan.buoyant { "0.01" } else { "0.1" };
        solvers.push_str(&solver_entry(
            &plan.pressure_field,
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
    if plan.potential {
        solvers.push_str(PHI_SOLVER);
    }
    let algorithm = if plan.transient {
        let predictor = if plan.buoyant { "no" } else { "yes" };
        format!(
            "PIMPLE\n{{\n    momentumPredictor {predictor};\n    nOuterCorrectors 1;\n    nCorrectors 2;\n    nNonOrthogonalCorrectors {non_orthogonal};\n{reference}}}\n"
        )
    } else {
        let consistent = if plan.buoyant {
            ""
        } else {
            "    consistent yes;\n"
        };
        format!(
            "SIMPLE\n{{\n    nNonOrthogonalCorrectors {non_orthogonal};\n{reference}{consistent}}}\n"
        )
    };
    let potential_flow = if plan.potential { POTENTIAL_FLOW } else { "" };
    let relaxation = match (plan.transient, plan.buoyant) {
        (true, _) => "",
        (false, true) => {
            "relaxationFactors\n{\n    fields { p_rgh 0.7; }\n    equations { U 0.3; T 0.5; \".*\" 0.5; }\n}\n"
        }
        (false, false) => "relaxationFactors\n{\n    equations { U 0.9; \".*\" 0.8; }\n}\n",
    };
    dictionary(
        "fvSolution",
        "dictionary",
        &format!("solvers\n{{\n{solvers}}}\n{algorithm}{potential_flow}{relaxation}"),
    )
}
