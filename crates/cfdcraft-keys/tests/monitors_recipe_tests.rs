//! What a case watches is part of the identity of its result, never of its mesh. The default
//! probes are in the recipe, so a study that lists none and a study that lists exactly them make
//! the same result key; labels and settle tolerances are in no key.

use cfdcraft_keys::{MonitorsRecipe, StageKeys, StageRecipes, ToolIdentity};
use cfdcraft_model::{
    MonitorField, ResolvedProbe, ResolvedStudy, ResolvedSurface, SettleSettings, Study,
    SurfaceQuantity, resolve,
};

fn cavity() -> ResolvedStudy {
    let study = Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap();
    resolve(&study).unwrap()
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn recipes(resolved: &ResolvedStudy) -> StageRecipes {
    StageRecipes::from_resolved(resolved, &tool())
}

fn keys(resolved: &ResolvedStudy) -> StageKeys {
    recipes(resolved).keys()
}

fn probe(label: &str, x: f64, y: f64) -> ResolvedProbe {
    ResolvedProbe {
        label: label.to_owned(),
        x,
        y,
        field: MonitorField::Velocity,
        boundary: None,
    }
}

/// The five default positions, computed as the plan computes them.
fn default_places(resolved: &ResolvedStudy) -> Vec<(f64, f64)> {
    let (lx, ly) = (resolved.domain.length_x, resolved.domain.length_y);
    [
        (0.5, 0.5),
        (0.25, 0.25),
        (0.75, 0.25),
        (0.25, 0.75),
        (0.75, 0.75),
    ]
    .iter()
    .map(|(fx, fy)| (fx * lx, fy * ly))
    .collect()
}

fn listed_defaults() -> ResolvedStudy {
    let mut resolved = cavity();
    resolved.monitors.probes = default_places(&resolved)
        .iter()
        .zip(["a", "b", "c", "d", "e"])
        .map(|((x, y), label)| probe(label, *x, *y))
        .collect();
    resolved
}

fn with_one_probe(label: &str) -> ResolvedStudy {
    let mut resolved = cavity();
    resolved.monitors.probes = vec![probe(label, 0.05, 0.05)];
    resolved
}

#[test]
fn a_study_that_lists_no_monitor_has_the_default_probes_in_its_recipe() {
    let resolved = cavity();
    let monitors = recipes(&resolved).result.monitors;
    let places = default_places(&resolved);
    assert_eq!(monitors.probes.len(), 5);
    for (recipe, (x, y)) in monitors.probes.iter().zip(places) {
        assert_eq!((recipe.x, recipe.y), (x, y));
        assert_eq!(recipe.field, MonitorField::Velocity);
    }
    assert!(monitors.lines.is_empty());
    assert!(monitors.surfaces.is_empty());
}

#[test]
fn listing_exactly_the_defaults_makes_the_same_result_key() {
    let listed = keys(&listed_defaults());
    let unlisted = keys(&cavity());
    assert_eq!(listed.result, unlisted.result);
    assert_eq!(listed.mesh, unlisted.mesh);
}

#[test]
fn an_added_probe_changes_the_result_key_and_not_the_mesh_key() {
    let old = keys(&listed_defaults());
    let mut more = listed_defaults();
    more.monitors.probes.push(probe("f", 0.05, 0.05));
    let new = keys(&more);
    assert_ne!(new.result, old.result);
    assert_eq!(new.mesh, old.mesh);
}

#[test]
fn a_moved_probe_changes_the_result_key() {
    let old = keys(&with_one_probe("centre"));
    let mut moved = with_one_probe("centre");
    moved.monitors.probes[0].x = 0.06;
    let new = keys(&moved);
    assert_ne!(new.result, old.result);
    assert_eq!(new.mesh, old.mesh);
}

#[test]
fn a_label_rename_that_keeps_the_order_changes_no_key() {
    assert_eq!(
        keys(&with_one_probe("centre")),
        keys(&with_one_probe("middle"))
    );
}

#[test]
fn a_settle_setting_changes_no_key() {
    let old = keys(&cavity());
    let mut settled = cavity();
    settled.monitors.settle = Some(SettleSettings {
        spread_rel: Some(0.02),
        ..SettleSettings::default()
    });
    assert_eq!(keys(&settled), old);
}

#[test]
fn a_surface_monitor_keys_on_the_boundary_kind_and_the_quantity() {
    let on = |boundary: &str, quantity| {
        let mut resolved = cavity();
        resolved.monitors.surfaces = vec![ResolvedSurface {
            label: "s".to_owned(),
            boundary: boundary.to_owned(),
            quantity,
        }];
        resolved
    };
    // Two boundaries of one kind, called differently, give one entry.
    let mut renamed = on("lid", SurfaceQuantity::Flux);
    let lid = renamed.boundary_conditions.remove("lid").unwrap();
    renamed.boundary_conditions.insert("top".to_owned(), lid);
    renamed.monitors.surfaces[0].boundary = "top".to_owned();
    let flux = recipes(&on("lid", SurfaceQuantity::Flux)).result.monitors;
    assert_eq!(flux.surfaces.len(), 1);
    assert_eq!(flux.surfaces, recipes(&renamed).result.monitors.surfaces);
    // A different quantity or a different kind of boundary differs.
    let average = on("lid", SurfaceQuantity::PressureAverage);
    assert_ne!(
        keys(&average).result,
        keys(&on("lid", SurfaceQuantity::Flux)).result
    );
    assert_ne!(
        flux.surfaces,
        recipes(&on("walls", SurfaceQuantity::Flux))
            .result
            .monitors
            .surfaces
    );
}

#[test]
fn a_recipe_without_the_new_lists_still_reads() {
    let monitors: MonitorsRecipe =
        serde_json::from_str(r#"{"centreline_u_sample_count":64}"#).unwrap();
    assert_eq!(monitors.centreline_u_sample_count, 64);
    assert!(monitors.probes.is_empty());
    assert!(monitors.lines.is_empty());
    assert!(monitors.surfaces.is_empty());
}

#[test]
fn a_recipe_serialises_without_empty_lists() {
    let monitors = MonitorsRecipe {
        centreline_u_sample_count: 64,
        ..MonitorsRecipe::default()
    };
    let text = serde_json::to_string(&monitors).unwrap();
    assert!(!text.contains("lines"), "{text}");
    assert!(!text.contains("surfaces"), "{text}");
    assert!(!text.contains("probes"), "{text}");
}
