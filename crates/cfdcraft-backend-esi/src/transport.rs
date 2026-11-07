use crate::{case::Patch, dictionary, monitors::short, scalar};
use cfdcraft_model::{BoundaryKind, InletProfile, ResolvedShape, ResolvedStudy};

pub fn transport_properties(s: &ResolvedStudy) -> String {
    // Boussinesq buoyancy adds the expansion coefficient, the reference temperature and the two
    // Prandtl numbers to the viscosity.
    let buoyancy = match s.physics.as_ref().and_then(|p| p.buoyancy) {
        Some(b) => format!(
            "beta [0 0 0 -1 0 0 0] {};\nTRef [0 0 0 1 0 0 0] {};\nPr [0 0 0 0 0 0 0] {};\nPrt [0 0 0 0 0 0 0] {};\n",
            short(b.beta),
            short(b.reference_temperature),
            short(b.prandtl),
            short(b.turbulent_prandtl)
        ),
        None => String::new(),
    };
    dictionary(
        "transportProperties",
        "dictionary",
        &format!(
            "transportModel Newtonian;\nnu [0 2 -1 0 0 0 0] {};\n{buoyancy}",
            scalar(s.fluid.kinematic_viscosity)
        ),
    )
}
pub fn turbulence_properties() -> String {
    dictionary(
        "turbulenceProperties",
        "dictionary",
        "simulationType laminar;\n",
    )
}
/// The gravity file of a buoyant study; gravity acts along -y.
pub fn gravity(s: &ResolvedStudy) -> Option<String> {
    let b = s.physics.as_ref()?.buoyancy?;
    Some(dictionary(
        "g",
        "uniformDimensionedVectorField",
        &format!(
            "dimensions [0 1 -2 0 0 0 0];\nvalue (0 {} 0);\n",
            scalar(-b.gravity)
        ),
    ))
}
/// `0` for an exact zero, the shortest round-trip decimal otherwise.
pub fn component(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        scalar(value)
    }
}

/// One line per patch, `name { entry }`, in the order of the patch table.
pub fn boundary_field(patches: &[Patch], entry: impl Fn(&Patch) -> String) -> String {
    patches
        .iter()
        .map(|p| format!("    {} {{ {} }}\n", p.name, entry(p)))
        .collect()
}

/// The extent of the inlet across the flow: where it starts on y and how tall it is. The inlet of
/// a step is the narrow channel above the step; any other inlet spans the whole height.
pub fn inlet_extent(s: &ResolvedStudy) -> (f64, f64) {
    match s.domain.shape {
        Some(ResolvedShape::BackwardStep { step_height, .. }) => {
            (step_height, s.domain.length_y - step_height)
        }
        _ => (0.0, s.domain.length_y),
    }
}

/// The inflow of a parabolic profile as a polynomial in the distance `s` along y from the start
/// of the inlet: `6 U s / h - 6 U s^2 / h^2` for the mean speed `U` and the height `h`, which is
/// zero at both walls and `1.5 U` in the middle. The signed speed makes an inlet on the right
/// face flow in the negative direction. No code is involved: the profile is a plain table.
fn parabolic_entry(s: &ResolvedStudy, mean: f64) -> String {
    let (origin, height) = inlet_extent(s);
    let linear = scalar(6.0 * mean / height);
    let quadratic = scalar(-6.0 * mean / (height * height));
    format!(
        "type fixedProfile; profile polynomial ((({linear} 0 0) (1 0 0)) (({quadratic} 0 0) (2 0 0))); direction (0 1 0); origin {};",
        component(origin)
    )
}

fn parabolic_inlet(s: &ResolvedStudy) -> bool {
    s.boundary_conditions.values().any(|b| {
        b.kind == BoundaryKind::VelocityInlet && b.profile == Some(InletProfile::Parabolic)
    })
}

pub fn velocity(s: &ResolvedStudy, patches: &[Patch]) -> String {
    let parabolic = parabolic_inlet(s);
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(kind @ (BoundaryKind::VelocityInlet | BoundaryKind::MovingWall)) => {
            let v = p.velocity.unwrap_or([0.0; 3]);
            if parabolic && kind == BoundaryKind::VelocityInlet {
                return parabolic_entry(s, v[0]);
            }
            format!(
                "type fixedValue; value uniform ({} {} {});",
                component(v[0]),
                component(v[1]),
                component(v[2])
            )
        }
        Some(BoundaryKind::PressureOutlet) => "type zeroGradient;".into(),
        Some(BoundaryKind::NoSlipWall) => "type noSlip;".into(),
        Some(BoundaryKind::SlipWall) => "type slip;".into(),
        Some(BoundaryKind::Symmetry) => format!("type {};", p.symmetry_type()),
    });
    dictionary(
        "U",
        "volVectorField",
        &format!(
            "dimensions [0 1 -1 0 0 0 0];\ninternalField uniform (0 0 0);\nboundaryField\n{{\n{entries}}}\n"
        ),
    )
}
pub fn pressure(patches: &[Patch]) -> String {
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(BoundaryKind::PressureOutlet) => "type fixedValue; value uniform 0;".into(),
        Some(BoundaryKind::Symmetry) => format!("type {};", p.symmetry_type()),
        Some(_) => "type zeroGradient;".into(),
    });
    dictionary(
        "p",
        "volScalarField",
        &format!(
            "dimensions [0 2 -2 0 0 0 0];\ninternalField uniform 0;\nboundaryField\n{{\n{entries}}}\n"
        ),
    )
}
