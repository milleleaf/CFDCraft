//! The function objects that watch a run: the probes, lines and surface integrals of the study,
//! and one flux object per patch so the mass balance can be judged whatever the study lists.
//!
//! Only numbers and fixed words are written. No author text reaches the case: objects are named
//! by position (`probe_1`, `line_1`, `surface_1`) and patches by the fixed patch table.

use crate::{case::patch_plan, scalar};
use cfdcraft_model::{BoundaryKind, MonitorField, ResolvedStudy, SurfaceQuantity};

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

/// The names of the monitor objects the case writes, without the centreline object: probes, then
/// lines, then surfaces, then the balances.
pub fn monitor_fo_names(s: &ResolvedStudy) -> Vec<String> {
    let plan = s.monitor_plan();
    plan.probes
        .into_iter()
        .map(|p| p.fo)
        .chain(plan.lines.into_iter().map(|l| l.fo))
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

fn probe(fo: &str, x: f64, y: f64, z: &str, field: MonitorField) -> String {
    let (x, y, f) = (scalar(x), scalar(y), field_word(field));
    format!(
        "    {fo}\n    {{\n        type probes;\n        libs (sampling);\n        writeControl timeStep;\n        writeInterval 1;\n        probeLocations\n        (\n            ({x} {y} {z})\n        );\n        fields ({f});\n    }}\n"
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
/// probes, lines, surfaces, then the balances.
pub fn functions_block(s: &ResolvedStudy) -> String {
    let plan = s.monitor_plan();
    let z = scalar(s.domain.length_z / 2.0);
    let named = patches(s);
    let mut out = String::new();
    for p in &plan.probes {
        out.push_str(&probe(&p.fo, p.x, p.y, &z, p.field));
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
    for surface in &plan.surfaces {
        let Some((patch, _)) = named.iter().find(|(_, kind)| *kind == surface.kind) else {
            continue;
        };
        let (operation, field) = match surface.quantity {
            SurfaceQuantity::Flux => ("sum", "phi"),
            SurfaceQuantity::PressureAverage => ("areaAverage", "p"),
            SurfaceQuantity::PressureIntegral => ("areaIntegrate", "p"),
        };
        out.push_str(&flux(&surface.fo, patch, operation, field));
    }
    for b in balance_patches(s) {
        out.push_str(&flux(&b.fo, b.patch, "sum", "phi"));
    }
    out
}
