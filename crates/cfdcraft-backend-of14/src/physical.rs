//! The property files of a case: the fluid properties and the momentum transport selection.

use crate::{
    markers::VISCOSITY_UNITS,
    thermo,
    turbulence::{model_of, model_word, momentum_transport_body},
};
use cfdcraft_backend_esi::{dictionary, scalar};
use cfdcraft_model::ResolvedStudy;

/// `constant/physicalProperties`: a constant viscosity model and the kinematic viscosity, or, for
/// a study with buoyancy, the thermophysical description of the fluid module.
pub fn physical_properties(s: &ResolvedStudy) -> String {
    if let Some(b) = s.physics.as_ref().and_then(|p| p.buoyancy) {
        return thermo::physical_properties(s, &b);
    }
    dictionary(
        "physicalProperties",
        "dictionary",
        &format!(
            "viscosityModel constant;\nnu {} {VISCOSITY_UNITS};\n",
            scalar(s.fluid.kinematic_viscosity)
        ),
    )
}

/// `constant/momentumTransport`: the laminar selection, or the Reynolds-averaged model of the
/// study with its switch on.
pub fn momentum_transport(s: &ResolvedStudy) -> String {
    let body = match model_word(model_of(s)) {
        Some(word) => momentum_transport_body(word),
        None => "simulationType laminar;\n".to_owned(),
    };
    dictionary("momentumTransport", "dictionary", &body)
}
