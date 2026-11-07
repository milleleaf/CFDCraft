//! The function objects that watch a run: the probes, lines, force coefficients and surface
//! integrals of the study, and one flux object per patch so the mass balance can be judged
//! whatever the study lists.
//!
//! Only numbers and fixed words are written. No author text reaches the case: objects are named
//! by position (`probe_1`, `line_1`, `force_1`, `surface_1`) and patches by the fixed patch table,
//! found through the boundary's label. Every object is typed out in full.

use crate::{case::patch_plan, patches::patch_of_label, scalar};
use cfdcraft_model::{
    BoundaryKind, MonitorField, PlannedForce, PlannedProbe, ResolvedStudy, SurfaceQuantity,
};

/// A flux object written for one patch, whatever the study lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalancePatch {
    /// Function-object name: `balance_<patch>`.
    pub fo: String,
    /// The written patch name.
    pub patch: &'static str,
    /// True for the patch that takes the flow in, the reference of the mass balance.
    pub inlet: bool,
}

/// The non-empty patches of the case with the kind each holds, in the fixed patch order.
fn patches(s: &ResolvedStudy) -> Vec<(&'static str, BoundaryKind)> {
    patch_plan(s)
        .map(|plan| {
            plan.into_iter()
                .filter_map(|p| p.kind.map(|kind| (p.name, kind)))
                .collect()
        })
        .unwrap_or_default()
}

/// One entry per non-empty patch, in the fixed patch order.
pub fn balance_patches(s: &ResolvedStudy) -> Vec<BalancePatch> {
    patches(s)
        .into_iter()
        .map(|(patch, kind)| BalancePatch {
            fo: format!("balance_{patch}"),
            patch,
            inlet: kind == BoundaryKind::VelocityInlet,
        })
        .collect()
}

/// The name of the gradient object a heat-flux surface integrates.
pub const HEAT_GRAD: &str = "heat_grad";

/// The names of the monitor objects the case writes, without the centreline object: probes, then
/// lines, then forces, then the gradient object when a heat-flux surface needs it, then surfaces,
/// then the balances.
pub fn monitor_fo_names(s: &ResolvedStudy) -> Vec<String> {
    let plan = s.monitor_plan();
    let heat = plan
        .surfaces
        .iter()
        .any(|f| f.quantity == SurfaceQuantity::HeatFlux);
    plan.probes
        .into_iter()
        .map(|p| p.fo)
        .chain(plan.lines.into_iter().map(|l| l.fo))
        .chain(plan.forces.into_iter().map(|f| f.fo))
        .chain(heat.then(|| HEAT_GRAD.to_owned()))
        .chain(plan.surfaces.into_iter().map(|f| f.fo))
        .chain(balance_patches(s).into_iter().map(|b| b.fo))
        .collect()
}

fn field_word(field: MonitorField) -> &'static str {
    match field {
        MonitorField::Velocity => "U",
        MonitorField::Pressure => "p",
    }
}

/// The shortest decimal, with no `.0` on a whole number.
pub fn short(value: f64) -> String {
    let text = scalar(value);
    text.strip_suffix(".0").unwrap_or(&text).to_owned()
}

/// A probe in the flow (`patch` is `None`) or on a wall (`patch` is the written patch).
fn probe(p: &PlannedProbe, z: &str, patch: Option<&str>) -> String {
    let (fo, x, y, f) = (&p.fo, scalar(p.x), scalar(p.y), field_word(p.field));
    let (kind, on) = match patch {
        Some(patch) => ("patchProbes", format!("        patches ({patch});\n")),
        None => ("probes", String::new()),
    };
    format!(
        "    {fo}\n    {{\n        type {kind};\n        libs (sampling);\n        writeControl timeStep;\n        writeInterval 1;\n{on}        probeLocations\n        (\n            ({x} {y} {z})\n        );\n        fields ({f});\n    }}\n"
    )
}

/// Force coefficients on one wall. The lift axis is set to y: the case lies in the x-y plane and
/// the default lift axis is z, which would leave the lift coefficient at zero. The reference area
/// is the reference length times the depth of the domain.
fn force(f: &PlannedForce, patch: &str, depth: f64) -> String {
    let fo = &f.fo;
    let magnitude = short(f.reference_velocity);
    let length = short(f.reference_length);
    let area = short(f.reference_length * depth);
    format!(
        "    {fo}\n    {{\n        type forceCoeffs;\n        libs (forces);\n        patches ({patch});\n        rho rhoInf;\n        rhoInf 1;\n        magUInf {magnitude};\n        lRef {length};\n        Aref {area};\n        origin (0 0 0);\n        e1 (1 0 0);\n        e3 (0 1 0);\n        writeControl timeStep;\n        writeInterval 1;\n    }}\n"
    )
}

/// The temperature gradient field a heat-flux surface integrates; it is computed, not written.
fn heat_gradient() -> String {
    format!(
        "    {HEAT_GRAD}\n    {{\n        type grad;\n        libs (fieldFunctionObjects);\n        field T;\n        result gradT;\n        writeControl none;\n    }}\n"
    )
}

fn line(fo: &str, ends: [f64; 4], z: &str, samples: u64, field: MonitorField) -> String {
    let [x0, y0, x1, y1] = ends.map(scalar);
    let f = field_word(field);
    format!(
        "    {fo}\n    {{\n        type sets;\n        libs (sampling);\n        writeControl writeTime;\n        setFormat raw;\n        interpolationScheme cellPoint;\n        fields ({f});\n        sets\n        (\n            line\n            {{\n                type uniform;\n                axis xyz;\n                start ({x0} {y0} {z});\n                end ({x1} {y1} {z});\n                nPoints {samples};\n            }}\n        );\n    }}\n"
    )
}

fn flux(fo: &str, patch: &str, operation: &str, field: &str) -> String {
    format!(
        "    {fo}\n    {{\n        type surfaceFieldValue;\n        libs (fieldFunctionObjects);\n        writeControl timeStep;\n        writeInterval 1;\n        log no;\n        writeFields no;\n        regionType patch;\n        name {patch};\n        operation {operation};\n        fields ({field});\n    }}\n"
    )
}

/// The objects that follow `centreline` inside the `functions` block, each indented four spaces:
/// probes, lines, forces, the gradient object, surfaces, then the balances. A monitor whose
/// boundary label has no written patch is left out.
pub fn functions_block(s: &ResolvedStudy) -> String {
    let plan = s.monitor_plan();
    let z = scalar(s.domain.length_z / 2.0);
    let mut out = String::new();
    for p in &plan.probes {
        match &p.boundary {
            None => out.push_str(&probe(p, &z, None)),
            Some(label) => {
                if let Some(patch) = patch_of_label(s, label) {
                    out.push_str(&probe(p, &z, Some(patch)));
                }
            }
        }
    }
    for l in &plan.lines {
        out.push_str(&line(
            &l.fo,
            [l.x0, l.y0, l.x1, l.y1],
            &z,
            l.samples,
            l.field,
        ));
    }
    for f in &plan.forces {
        if let Some(patch) = patch_of_label(s, &f.boundary) {
            out.push_str(&force(f, patch, s.domain.length_z));
        }
    }
    if plan
        .surfaces
        .iter()
        .any(|f| f.quantity == SurfaceQuantity::HeatFlux)
    {
        out.push_str(&heat_gradient());
    }
    for surface in &plan.surfaces {
        let Some(patch) = patch_of_label(s, &surface.boundary) else {
            continue;
        };
        let (operation, field) = match surface.quantity {
            SurfaceQuantity::Flux => ("sum", "phi"),
            SurfaceQuantity::PressureAverage => ("areaAverage", "p"),
            SurfaceQuantity::PressureIntegral => ("areaIntegrate", "p"),
            SurfaceQuantity::HeatFlux => ("areaIntegrate", "gradT"),
        };
        out.push_str(&flux(&surface.fo, patch, operation, field));
    }
    for b in balance_patches(s) {
        out.push_str(&flux(&b.fo, b.patch, "sum", "phi"));
    }
    out
}
