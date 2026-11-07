use crate::{case::Patch, dictionary, scalar};
use cfdcraft_model::BoundaryKind;
use cfdcraft_model::ResolvedStudy;

pub fn transport_properties(s: &ResolvedStudy) -> String {
    dictionary(
        "transportProperties",
        "dictionary",
        &format!(
            "transportModel Newtonian;\nnu [0 2 -1 0 0 0 0] {};\n",
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
/// `0` for an exact zero, the shortest round-trip decimal otherwise.
fn component(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        scalar(value)
    }
}

fn boundary_field(patches: &[Patch], entry: impl Fn(&Patch) -> String) -> String {
    patches
        .iter()
        .map(|p| format!("    {} {{ {} }}\n", p.name, entry(p)))
        .collect()
}

pub(crate) fn velocity(patches: &[Patch]) -> String {
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(BoundaryKind::VelocityInlet | BoundaryKind::MovingWall) => {
            let v = p.velocity.unwrap_or([0.0; 3]);
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
        Some(BoundaryKind::Symmetry) => "type symmetryPlane;".into(),
    });
    dictionary(
        "U",
        "volVectorField",
        &format!(
            "dimensions [0 1 -1 0 0 0 0];\ninternalField uniform (0 0 0);\nboundaryField\n{{\n{entries}}}\n"
        ),
    )
}
pub(crate) fn pressure(patches: &[Patch]) -> String {
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(BoundaryKind::PressureOutlet) => "type fixedValue; value uniform 0;".into(),
        Some(BoundaryKind::Symmetry) => "type symmetryPlane;".into(),
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
