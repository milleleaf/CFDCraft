//! The function objects that watch a Foundation 14 run, in the key forms of this line: the probes,
//! sampled lines, force coefficients and surface integrals of the study, and one flux object per
//! patch so the mass balance can be judged whatever the study lists.
//!
//! Only numbers and fixed words are written. Objects are named by position (`probe_1`, `line_1`,
//! `force_1`, `surface_1`) and patches come from the fixed patch table, so no author text reaches
//! the case. Every object is typed out in full, and the only libraries loaded are the three the
//! safety scan lists. Residuals are read from the solver log: the residuals object lives in a
//! library the scan does not list.

use crate::markers::{
    LIB_FIELD_FUNCTIONS, LIB_FORCES, LIB_SAMPLING, LINE_SET_TYPE, SURFACE_PATCH_KEY,
};
use cfdcraft_backend_esi::{
    monitors::{HEAT_GRAD, balance_patches, short},
    patches::patch_of_label,
    scalar,
};
use cfdcraft_model::{MonitorField, PlannedForce, PlannedProbe, ResolvedStudy, SurfaceQuantity};

pub(crate) fn field_word(field: MonitorField) -> &'static str {
    match field {
        MonitorField::Velocity => "U",
        MonitorField::Pressure => "p",
    }
}

/// One sampled line of a sampling object.
pub(crate) struct SetLine<'a> {
    pub name: &'a str,
    pub axis: &'a str,
    pub start: [String; 3],
    pub end: [String; 3],
}

/// A sampling object of lines: the sets are a dictionary of named sets.
pub(crate) fn sets_object(fo: &str, field: &str, lines: &[SetLine<'_>], points: u64) -> String {
    let sets: String = lines
        .iter()
        .map(|l| {
            format!(
                "            {}\n            {{\n                type {LINE_SET_TYPE};\n                axis {};\n                start ({} {} {});\n                end ({} {} {});\n                nPoints {points};\n            }}\n",
                l.name, l.axis, l.start[0], l.start[1], l.start[2], l.end[0], l.end[1], l.end[2]
            )
        })
        .collect();
    format!(
        "    {fo}\n    {{\n        type sets;\n        libs (\"{LIB_SAMPLING}\");\n        writeControl writeTime;\n        setFormat raw;\n        interpolationScheme cellPoint;\n        fields ({field});\n        sets\n        {{\n{sets}        }}\n    }}\n"
    )
}

/// A probe in the flow (`patch` is `None`) or on a wall (`patch` is the written patch).
fn probe(p: &PlannedProbe, z: &str, patch: Option<&str>) -> String {
    let (fo, x, y, f) = (&p.fo, scalar(p.x), scalar(p.y), field_word(p.field));
    let (kind, on) = match patch {
        Some(patch) => ("patchProbes", format!("        patches ({patch});\n")),
        None => ("probes", String::new()),
    };
    format!(
        "    {fo}\n    {{\n        type {kind};\n        libs (\"{LIB_SAMPLING}\");\n        writeControl timeStep;\n        writeInterval 1;\n{on}        probeLocations\n        (\n            ({x} {y} {z})\n        );\n        fields ({f});\n    }}\n"
    )
}

/// Force coefficients on one wall, with the lift along y: the case lies in the x-y plane. The
/// reference area is the reference length times the depth of the domain. The ESI-only keys (an
/// origin and two axes) are not written: this line names the lift and drag directions and the
/// pitch axis.
fn force(f: &PlannedForce, patch: &str, depth: f64) -> String {
    let fo = &f.fo;
    let magnitude = short(f.reference_velocity);
    let length = short(f.reference_length);
    let area = short(f.reference_length * depth);
    format!(
        "    {fo}\n    {{\n        type forceCoeffs;\n        libs (\"{LIB_FORCES}\");\n        patches ({patch});\n        rho rhoInf;\n        rhoInf 1;\n        magUInf {magnitude};\n        lRef {length};\n        Aref {area};\n        CofR (0 0 0);\n        liftDir (0 1 0);\n        dragDir (1 0 0);\n        pitchAxis (0 0 1);\n        writeControl timeStep;\n        writeInterval 1;\n    }}\n"
    )
}

/// The temperature gradient a heat-flux surface integrates; computed, never written.
fn heat_gradient() -> String {
    format!(
        "    {HEAT_GRAD}\n    {{\n        type grad;\n        libs (\"{LIB_FIELD_FUNCTIONS}\");\n        field T;\n        result gradT;\n        writeControl none;\n    }}\n"
    )
}

/// A surface integral over one patch.
fn flux(fo: &str, patch: &str, operation: &str, field: &str) -> String {
    format!(
        "    {fo}\n    {{\n        type surfaceFieldValue;\n        libs (\"{LIB_FIELD_FUNCTIONS}\");\n        writeControl timeStep;\n        writeInterval 1;\n        log no;\n        writeFields no;\n        {SURFACE_PATCH_KEY} {patch};\n        operation {operation};\n        fields ({field});\n    }}\n"
    )
}

/// The objects after the centreline, inside `functions`: probes, lines, forces, the gradient
/// object when a heat-flux surface needs it, surfaces, then one flux object per patch. A monitor
/// whose boundary label has no written patch is left out.
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
        let line = SetLine {
            name: "line",
            axis: "xyz",
            start: [scalar(l.x0), scalar(l.y0), z.clone()],
            end: [scalar(l.x1), scalar(l.y1), z.clone()],
        };
        out.push_str(&sets_object(&l.fo, field_word(l.field), &[line], l.samples));
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
