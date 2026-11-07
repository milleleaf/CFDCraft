//! The heat transfer files of the heated cavity, written for the fluid solver module.
//!
//! The older line solves the heated cavity with a kinematic form; this line solves it with the
//! density-carrying energy form and a Boussinesq equation of state, so the two are different
//! formulations of one problem and the parity table shows the difference. The free constants of
//! the density-carrying form are chosen so that they cannot move the result, and they are stated
//! here once:
//!
//! | constant | value | why it cannot move the result |
//! |---|---|---|
//! | `rho0` | 1 | with `rho0` = 1 the dynamic viscosity equals the kinematic one (`mu = nu * rho0`) and the density in the equations is the study's kinematic form to within `beta (T - T0)` |
//! | `Cp` | 1005 J/(kg K) | the conductivity is `Cp * mu / Pr`, so the thermal diffusivity is `kappa / (rho0 * Cp) = nu / Pr` whatever `Cp` is; it only scales the enthalpy |
//! | molecular weight | 28.96 | used by the equation of state of a gas only; the Boussinesq form does not read it |
//! | enthalpy of formation | 0 | a constant offset of the enthalpy |
//!
//! `T0`, `beta`, `Pr` and the viscosity come from the study. A raw override tagged `foundation`
//! on `thermodynamics/Cp` lets a hosted run check the insensitivity to `Cp` on the real image.

use crate::markers::{
    ALPHAT_WALL_FUNCTION, ENERGY, EQUATION_OF_STATE, LAMINAR_HEAT_MODEL, THERMO_TYPE,
    TURBULENT_HEAT_MODEL,
};
use cfdcraft_backend_esi::{
    case::Patch,
    dictionary, scalar,
    transport::{self, boundary_field},
};
use cfdcraft_model::{BoundaryKind, ResolvedBuoyancy, ResolvedStudy};

/// The reference density in kg/m3. One, so the dynamic viscosity of the written case equals the
/// study's kinematic viscosity.
pub const REFERENCE_DENSITY: f64 = 1.0;
/// The specific heat in J/(kg K). A free choice: the conductivity is written through the Prandtl
/// number, so the thermal diffusivity does not depend on it.
pub const HEAT_CAPACITY: f64 = 1005.0;
/// The molecular weight in g/mol. A free choice, unused by the Boussinesq equation of state.
pub const MOLECULAR_WEIGHT: f64 = 28.96;
/// The enthalpy of formation in J/kg. A free choice: an offset of the enthalpy.
pub const ENTHALPY_OF_FORMATION: f64 = 0.0;

/// `constant/physicalProperties` of a heat transfer case: the thermodynamic type, the species, the
/// equation of state, the thermodynamics and the transport. The dynamic viscosity is the study's
/// kinematic viscosity times the reference density.
pub fn physical_properties(s: &ResolvedStudy, b: &ResolvedBuoyancy) -> String {
    dictionary(
        "physicalProperties",
        "dictionary",
        &format!(
            "thermoType\n{{\n    type {THERMO_TYPE};\n    mixture pureMixture;\n    transport const;\n    thermo hConst;\n    equationOfState {EQUATION_OF_STATE};\n    specie specie;\n    energy {ENERGY};\n}}\n\nspecie\n{{\n    molWeight {};\n}}\n\nequationOfState\n{{\n    rho0 {};\n    T0 {};\n    beta {};\n}}\n\nthermodynamics\n{{\n    Cp {};\n    hf {};\n}}\n\ntransport\n{{\n    mu {};\n    Pr {};\n}}\n",
            scalar(MOLECULAR_WEIGHT),
            scalar(REFERENCE_DENSITY),
            scalar(b.reference_temperature),
            scalar(b.beta),
            scalar(HEAT_CAPACITY),
            scalar(ENTHALPY_OF_FORMATION),
            scalar(s.fluid.kinematic_viscosity * REFERENCE_DENSITY),
            scalar(b.prandtl),
        ),
    )
}

/// `constant/thermophysicalTransport`: the conduction model of a laminar case and the eddy
/// diffusivity, with its turbulent Prandtl number, of a turbulent one. Both blocks are always
/// written, so the study's turbulent Prandtl number is always in the case.
pub fn thermophysical_transport(b: &ResolvedBuoyancy) -> String {
    dictionary(
        "thermophysicalTransport",
        "dictionary",
        &format!(
            "laminar\n{{\n    model {LAMINAR_HEAT_MODEL};\n}}\n\nRAS\n{{\n    model {TURBULENT_HEAT_MODEL};\n    Prt {};\n}}\n",
            scalar(b.turbulent_prandtl)
        ),
    )
}

/// `constant/g`: gravity along minus y, the same file the older line writes.
pub fn gravity(s: &ResolvedStudy) -> Option<String> {
    transport::gravity(s)
}

fn scalar_file(object: &str, dimensions: &str, entries: &str) -> String {
    dictionary(
        object,
        "volScalarField",
        &format!(
            "dimensions {dimensions};\ninternalField uniform 0;\nboundaryField\n{{\n{entries}}}\n"
        ),
    )
}

/// The pressure without the hydrostatic part, which the heat transfer solver solves for: fixed at
/// zero on an outlet, flux-compatible everywhere else.
pub fn p_rgh_field(patches: &[Patch]) -> String {
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(BoundaryKind::PressureOutlet) => "type fixedValue; value uniform 0;".into(),
        Some(BoundaryKind::Symmetry) => format!("type {};", p.symmetry_type()),
        Some(_) => "type fixedFluxPressure; value uniform 0;".into(),
    });
    scalar_file("p_rgh", "[0 2 -2 0 0 0 0]", &entries)
}

/// The static pressure the solver works out from `p_rgh`; it is never read as an input.
pub fn p_field(patches: &[Patch]) -> String {
    let entries = boundary_field(patches, |p| match p.kind {
        None => "type empty;".into(),
        Some(BoundaryKind::Symmetry) => format!("type {};", p.symmetry_type()),
        Some(_) => "type calculated; value uniform 0;".into(),
    });
    scalar_file("p", "[0 2 -2 0 0 0 0]", &entries)
}

/// The turbulent thermal diffusivity: read by the solver even when the flow is laminar. A
/// turbulence model puts the wall function with the turbulent Prandtl number on every wall.
pub fn alphat_field(patches: &[Patch], turbulent_prandtl: Option<f64>) -> String {
    let entries = boundary_field(patches, |p| match (p.kind, turbulent_prandtl) {
        (None, _) => "type empty;".into(),
        (Some(BoundaryKind::Symmetry), _) => format!("type {};", p.symmetry_type()),
        (Some(BoundaryKind::NoSlipWall | BoundaryKind::MovingWall), Some(prt)) => format!(
            "type {ALPHAT_WALL_FUNCTION}; Prt {}; value uniform 0;",
            scalar(prt)
        ),
        _ => "type calculated; value uniform 0;".into(),
    });
    scalar_file("alphat", "[0 2 -1 0 0 0 0]", &entries)
}
