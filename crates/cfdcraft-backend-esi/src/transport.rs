use crate::{dictionary, scalar};
use cfdcraft_model::ResolvedStudy;

pub fn transport_properties(s: &ResolvedStudy) -> String {
    dictionary(
        "transportProperties",
        "dictionary",
        &format!(
            "transportModel Newtonian;\nnu [0 2 -1 0 0 0 0] {};\n",
            scalar(s.nu)
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
pub fn velocity(s: &ResolvedStudy) -> String {
    dictionary(
        "U",
        "volVectorField",
        &format!(
            "dimensions [0 1 -1 0 0 0 0];\ninternalField uniform (0 0 0);\nboundaryField\n{{\n    movingWall {{ type fixedValue; value uniform ({} 0 0); }}\n    fixedWalls {{ type noSlip; }}\n    frontAndBack {{ type empty; }}\n}}\n",
            scalar(s.lid_velocity)
        ),
    )
}
pub fn pressure() -> String {
    dictionary(
        "p",
        "volScalarField",
        "dimensions [0 2 -2 0 0 0 0];\ninternalField uniform 0;\nboundaryField\n{\n    movingWall { type zeroGradient; }\n    fixedWalls { type zeroGradient; }\n    frontAndBack { type empty; }\n}\n",
    )
}
