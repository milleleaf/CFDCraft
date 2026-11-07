//! A resolved study becomes the files of a Foundation 14 case.
//!
//! The patch table, the mesh dictionary body and the field boundary text come from the public items
//! of the older writer, which are the same for both lines. Everything the 14 line does differently
//! is written in this crate: the control dictionary, the numerics, the property files, the heat
//! transfer and turbulence files, the run plan. A setting the writer cannot write is refused with
//! its name, never written wrong.

use crate::{
    control::control_dict,
    markers::{MOMENTUM_TRANSPORT, PHYSICAL_PROPERTIES, THERMOPHYSICAL_TRANSPORT},
    physical::{momentum_transport, physical_properties},
    solution::{Numerics, fv_schemes, fv_solution},
    thermo, turbulence,
};
use cfdcraft_backend::{CaseFile, CaseTree, CompileFailure, CompileOptions};
use cfdcraft_backend_esi::{
    block_mesh::block_mesh,
    dictionary, fields,
    patches::{layout_of, patch_plan_with},
    transport,
};
use cfdcraft_dictedit::set_entry;
use cfdcraft_model::{RawBackend, RawOverride, ResolvedStudy, SurfaceQuantity, TurbulenceModel};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn invalid(reason: &str) -> CompileFailure {
    CompileFailure::Invalid {
        reason: reason.to_owned(),
    }
}

fn not_supported(setting: &str, reason: &str) -> CompileFailure {
    CompileFailure::NotSupported {
        setting: setting.to_owned(),
        reason: reason.to_owned(),
    }
}

/// The first setting of the study this writer cannot write yet, named. Every turbulence model,
/// time mode, buoyancy and the potential-flow start are written; a heat-flux surface needs the
/// temperature field that only a study with buoyancy has.
fn first_unsupported(s: &ResolvedStudy) -> Option<CompileFailure> {
    let buoyant = s.physics.as_ref().is_some_and(|p| p.buoyancy.is_some());
    (!buoyant
        && s.monitors
            .surfaces
            .iter()
            .any(|m| m.quantity == SurfaceQuantity::HeatFlux))
    .then(|| not_supported("a heat-flux surface", "there is no temperature field"))
}

/// A relative path as `/`-joined text, the form a study names a file in.
fn name_of(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn raw_refusal(label: &str, reason: impl Into<String>) -> CompileFailure {
    CompileFailure::Raw {
        label: label.to_owned(),
        reason: reason.into(),
    }
}

/// Apply every override, in label order, to the file it names, with the dictionary editor. Refused,
/// naming the override: a backend tag other than `foundation`, a file the case does not have, an
/// entry another override already set, a file that is not text, and any refusal of the editor.
fn apply_raw(
    files: &mut [CaseFile],
    overrides: &BTreeMap<String, RawOverride>,
) -> Result<(), CompileFailure> {
    let mut set: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (label, item) in overrides {
        if item.backend != RawBackend::Foundation {
            return Err(raw_refusal(
                label,
                format!(
                    "it is written for the {} backend and this case is written for foundation",
                    item.backend
                ),
            ));
        }
        let Some(file) = files.iter_mut().find(|f| name_of(&f.rel_path) == item.file) else {
            return Err(raw_refusal(
                label,
                format!("the case has no file {}", item.file),
            ));
        };
        if !set.insert((item.file.as_str(), item.entry.as_str())) {
            return Err(raw_refusal(
                label,
                format!(
                    "another override already sets {} in {}",
                    item.entry, item.file
                ),
            ));
        }
        let text = std::str::from_utf8(&file.bytes)
            .map_err(|_| raw_refusal(label, format!("{} is not text", item.file)))?;
        let edited = set_entry(text, &item.entry, &item.value)
            .map_err(|e| raw_refusal(label, format!("{} in {}: {e}", item.entry, item.file)))?;
        file.bytes = edited.into_bytes();
    }
    Ok(())
}

/// The case files of a resolved study, in a fixed order: the fields, the property files, then the
/// system files, with the decomposition dictionary after them when more than one process runs.
pub fn compile(s: &ResolvedStudy, options: &CompileOptions) -> Result<CaseTree, CompileFailure> {
    if options.np == 0 {
        return Err(invalid("the process count must be at least one"));
    }
    if s.mesh.cells_z != 1 {
        return Err(invalid(
            "the case needs one cell through z for empty front and back patches",
        ));
    }
    if s.schema_version != 1 {
        return Err(invalid("schema version"));
    }
    for value in [
        s.domain.length_x,
        s.domain.length_y,
        s.domain.length_z,
        s.fluid.kinematic_viscosity,
        s.solver.end_time,
        s.solver.write_interval,
        s.solver.convergence.p,
        s.solver.convergence.u,
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(invalid("positive finite physical quantities required"));
        }
    }
    if s.mesh.cells_x == 0 || s.mesh.cells_y == 0 || s.monitors.centreline_u_sample_count < 2 {
        return Err(invalid("cell count or sample count"));
    }
    if let Some(refusal) = first_unsupported(s) {
        return Err(refusal);
    }
    let invalid_of = |e: cfdcraft_backend_esi::CompileError| CompileFailure::Invalid {
        reason: e.to_string(),
    };
    let layout = layout_of(s).map_err(invalid_of)?;
    let patches = patch_plan_with(s, &layout).map_err(invalid_of)?;
    let buoyancy = s.physics.as_ref().and_then(|p| p.buoyancy);
    let mut named: Vec<(&str, String)> = vec![("0/U", transport::velocity(s, &patches))];
    match buoyancy {
        Some(b) => {
            let turbulent = turbulence::model_of(s) != TurbulenceModel::Laminar;
            named.push(("0/p", thermo::p_field(&patches)));
            named.push(("0/p_rgh", thermo::p_rgh_field(&patches)));
            named.push(("0/T", fields::t_field(&patches, b.reference_temperature)));
            named.push((
                "0/alphat",
                thermo::alphat_field(&patches, turbulent.then_some(b.turbulent_prandtl)),
            ));
        }
        None => named.push(("0/p", transport::pressure(&patches))),
    }
    named.extend(turbulence::field_files(s, &patches)?);
    named.push((PHYSICAL_PROPERTIES, physical_properties(s)));
    if let Some(b) = buoyancy {
        named.extend(thermo::gravity(s).map(|g| ("constant/g", g)));
        named.push((
            THERMOPHYSICAL_TRANSPORT,
            thermo::thermophysical_transport(&b),
        ));
    }
    let numerics = Numerics::of(s);
    named.extend([
        (MOMENTUM_TRANSPORT, momentum_transport(s)),
        ("system/blockMeshDict", block_mesh(&layout, &patches)),
        ("system/controlDict", control_dict(s)),
        ("system/fvSchemes", fv_schemes(&numerics)),
        ("system/fvSolution", fv_solution(&numerics)),
    ]);
    let mut files: Vec<CaseFile> = named
        .into_iter()
        .map(|(name, text)| CaseFile {
            rel_path: name.into(),
            bytes: text.into_bytes(),
        })
        .collect();
    apply_raw(&mut files, &s.raw_overrides)?;
    if options.np > 1 {
        files.push(CaseFile {
            rel_path: "system/decomposeParDict".into(),
            bytes: dictionary(
                "decomposeParDict",
                "dictionary",
                &format!("numberOfSubdomains {};\nmethod scotch;\n", options.np),
            )
            .into_bytes(),
        });
    }
    Ok(CaseTree { files })
}
