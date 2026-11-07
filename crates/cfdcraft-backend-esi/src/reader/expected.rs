//! What the written case must hold for the parts of a study the model cannot expect on its own:
//! the geometry and the per-patch conditions of a shaped study, which come from the block layout and
//! the patch table, and the patch a force or a boundary probe reads, which only the writer decides.
//!
//! This is the expected side of the round-trip check. It is the one part of the reader that knows
//! the patch table; the reader proper never does, so a fault shared by the writer and the table
//! shows up as a difference between the bytes and this list.

use crate::patches::{Patch, layout_of, patch_of_label, patch_plan_with};
use cfdcraft_model::{
    BoundaryKind, InletProfile, ResolvedStudy,
    diagnostics::{kind_name, pointer_token},
    roundtrip::{EMPTY_KIND, Expected, FactValue, grading_text},
};
use cfdcraft_shapes::Layout;

fn expected(key: String, value: FactValue, sources: &[String], setting: String) -> Expected {
    Expected {
        key,
        value,
        source_pointers: sources.to_vec(),
        setting,
    }
}

/// The facts to pass, with the facts of the model, to `check_roundtrip_with`: the patch each force
/// and each boundary probe reads for every study, and for a shaped study the geometry of its
/// layout and the kind, velocity, profile and thermal condition of each of its patches. A box
/// study with no force and no boundary probe has none. The claimed source pointers are the leaves
/// the model leaves to this list.
pub fn shaped_expected_facts(study: &ResolvedStudy) -> Vec<Expected> {
    let mut out = monitor_patches(study);
    if study.domain.shape.is_some()
        && let Ok(layout) = layout_of(study)
        && let Ok(patches) = patch_plan_with(study, &layout)
    {
        geometry(study, &layout, &mut out);
        patch_facts(study, &patches, &mut out);
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// The number a function-object name ends with: `force_3` gives `3`.
fn number(fo: &str) -> &str {
    fo.rsplit('_').next().unwrap_or(fo)
}

/// The written patch of each force and each boundary probe. A monitor whose boundary has no
/// written patch is left out of the case, so it is left out here.
fn monitor_patches(study: &ResolvedStudy) -> Vec<Expected> {
    let plan = study.monitor_plan();
    let mut out = Vec::new();
    for force in &plan.forces {
        if let Some(patch) = patch_of_label(study, &force.boundary) {
            let n = number(&force.fo);
            out.push(expected(
                format!("/monitors/force/{n}/patch"),
                FactValue::Text(patch.into()),
                &[],
                format!("force {n} patch"),
            ));
        }
    }
    for probe in &plan.probes {
        if let Some(label) = &probe.boundary
            && let Some(patch) = patch_of_label(study, label)
        {
            let n = number(&probe.fo);
            out.push(expected(
                format!("/monitors/probe/{n}/patch"),
                FactValue::Text(patch.into()),
                &[],
                format!("probe {n} patch"),
            ));
        }
    }
    out
}

fn words(items: impl IntoIterator<Item = impl ToString>) -> String {
    items
        .into_iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The bounding box, the vertices, the blocks and the curved edges of the layout.
fn geometry(study: &ResolvedStudy, layout: &Layout, out: &mut Vec<Expected>) {
    for (axis, length) in [
        ("x", study.domain.length_x),
        ("y", study.domain.length_y),
        ("z", study.domain.length_z),
    ] {
        out.push(expected(
            format!("/domain/length_{axis}"),
            FactValue::Number(length),
            &[format!("/domain/length_{axis}")],
            format!("domain length in {axis}"),
        ));
        out.push(expected(
            format!("/domain/origin_{axis}"),
            FactValue::Number(0.0),
            &[],
            format!("domain origin in {axis}"),
        ));
    }
    let shape = "/domain/shape".to_owned();
    let mesh = "/mesh".to_owned();
    let both = [shape.clone(), mesh.clone()];
    out.push(expected(
        "/mesh/cells_total".into(),
        FactValue::Count(layout.cell_count),
        std::slice::from_ref(&mesh),
        "total cell count".into(),
    ));
    out.push(expected(
        "/mesh/vertices".into(),
        FactValue::Count(layout.vertices.len() as u64),
        &both,
        "vertex count".into(),
    ));
    for (i, vertex) in layout.vertices.iter().enumerate() {
        for (axis, value) in ["x", "y", "z"].into_iter().zip(*vertex) {
            out.push(expected(
                format!("/mesh/vertex/{i}/{axis}"),
                FactValue::Number(value),
                std::slice::from_ref(&shape),
                format!("vertex {i} {axis}"),
            ));
        }
    }
    out.push(expected(
        "/mesh/blocks".into(),
        FactValue::Count(layout.blocks.len() as u64),
        &both,
        "block count".into(),
    ));
    for (j, block) in layout.blocks.iter().enumerate() {
        let text = [
            ("vertices", words(block.vertices)),
            ("cells", words(block.cells)),
            ("grading", grading_text(&block.grading)),
        ];
        for (name, value) in text {
            out.push(expected(
                format!("/mesh/block/{j}/{name}"),
                FactValue::Text(value),
                std::slice::from_ref(&mesh),
                format!("block {j} {name}"),
            ));
        }
    }
    out.push(expected(
        "/mesh/arcs".into(),
        FactValue::Count(layout.arcs.len() as u64),
        std::slice::from_ref(&shape),
        "curved edge count".into(),
    ));
    for (k, arc) in layout.arcs.iter().enumerate() {
        out.push(expected(
            format!("/mesh/arc/{k}/ends"),
            FactValue::Text(format!("{} {}", arc.from, arc.to)),
            std::slice::from_ref(&shape),
            format!("curved edge {k} ends"),
        ));
        for (axis, value) in ["x", "y", "z"].into_iter().zip(arc.through) {
            out.push(expected(
                format!("/mesh/arc/{k}/through/{axis}"),
                FactValue::Number(value),
                std::slice::from_ref(&shape),
                format!("curved edge {k} through point {axis}"),
            ));
        }
    }
}

/// For each patch of the table: its mesh type and faces, its kind, and the velocity, profile and
/// thermal condition the boundaries on it impose.
fn patch_facts(study: &ResolvedStudy, patches: &[Patch], out: &mut Vec<Expected>) {
    let buoyant = study
        .physics
        .as_ref()
        .is_some_and(|physics| physics.buoyancy.is_some());
    for patch in patches {
        let name = patch.name;
        let labels: Vec<&String> = study
            .boundary_conditions
            .keys()
            .filter(|label| patch_of_label(study, label.as_str()) == Some(name))
            .collect();
        let sources: Vec<String> = labels
            .iter()
            .map(|label| format!("/boundary_conditions/{}", pointer_token(label)))
            .collect();
        let boundaries = || {
            labels
                .iter()
                .map(|l| &study.boundary_conditions[l.as_str()])
        };
        let mut put = |key: String, value: FactValue, sources: &[String], what: &str| {
            out.push(expected(
                key,
                value,
                sources,
                format!("{what} of the patch {name}"),
            ));
        };
        put(
            format!("/mesh/patch/{name}/type"),
            FactValue::Text(patch.mesh_type().into()),
            sources.as_slice(),
            "mesh type",
        );
        put(
            format!("/mesh/patch/{name}/faces"),
            FactValue::Text(patch.faces_text()),
            sources.as_slice(),
            "faces",
        );
        put(
            format!("/patch/{name}/kind"),
            FactValue::Text(patch.kind.map_or(EMPTY_KIND, kind_name).into()),
            sources.as_slice(),
            "kind",
        );
        let moving = matches!(
            patch.kind,
            Some(BoundaryKind::VelocityInlet | BoundaryKind::MovingWall)
        );
        if moving && let Some(speed) = boundaries().find_map(|b| b.velocity) {
            put(
                format!("/patch/{name}/velocity"),
                FactValue::Number(speed),
                sources.as_slice(),
                "velocity",
            );
        }
        let parabolic = boundaries().any(|b| {
            b.kind == BoundaryKind::VelocityInlet && b.profile == Some(InletProfile::Parabolic)
        });
        if parabolic {
            put(
                format!("/patch/{name}/profile"),
                FactValue::Text("parabolic".into()),
                sources.as_slice(),
                "inlet profile",
            );
        }
        let wall = matches!(
            patch.kind,
            Some(BoundaryKind::NoSlipWall | BoundaryKind::SlipWall | BoundaryKind::MovingWall)
        );
        if buoyant && wall {
            match patch.temperature {
                Some(temperature) => {
                    put(
                        format!("/patch/{name}/thermal"),
                        FactValue::Text("fixed".into()),
                        sources.as_slice(),
                        "thermal condition",
                    );
                    put(
                        format!("/patch/{name}/temperature"),
                        FactValue::Number(temperature),
                        sources.as_slice(),
                        "wall temperature",
                    );
                }
                None => put(
                    format!("/patch/{name}/thermal"),
                    FactValue::Text("adiabatic".into()),
                    sources.as_slice(),
                    "thermal condition",
                ),
            }
        }
    }
}
