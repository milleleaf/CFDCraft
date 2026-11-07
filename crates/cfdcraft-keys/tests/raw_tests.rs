//! A raw override is part of the identity of a result, and of a mesh when it touches a mesh
//! dictionary; a study without one keeps every recipe and key it had.

use cfdcraft_keys::{MeshRecipe, ResultRecipe, StageKeys, StageRecipes, ToolIdentity};
use cfdcraft_model::{RawBackend, RawOverride, ResolvedStudy, Study, resolve};

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

fn raw(file: &str, entry: &str, value: &str) -> RawOverride {
    RawOverride {
        backend: RawBackend::Esi,
        file: file.to_owned(),
        entry: entry.to_owned(),
        value: value.to_owned(),
    }
}

fn relax() -> RawOverride {
    raw("system/fvSolution", "relaxationFactors/equations/U", "0.5")
}

fn block_mesh() -> RawOverride {
    raw("system/blockMeshDict", "convertToMeters", "0.1")
}

fn with(label: &str, item: RawOverride) -> ResolvedStudy {
    let mut resolved = cavity();
    resolved.raw_overrides.insert(label.to_owned(), item);
    resolved
}

#[test]
fn a_study_without_raw_overrides_keeps_its_recipes_and_keys() {
    let both = recipes(&cavity());
    for text in [
        serde_jcs::to_string(&both.mesh).unwrap(),
        serde_jcs::to_string(&both.result).unwrap(),
    ] {
        assert!(!text.contains("raw"), "{text}");
    }
    assert!(both.mesh.raw.is_empty());
    assert!(both.result.raw.is_empty());
}

#[test]
fn a_raw_override_outside_the_mesh_moves_the_result_key_only() {
    let old = keys(&cavity());
    let new = keys(&with("relax-u", relax()));
    assert_eq!(new.mesh, old.mesh);
    assert_ne!(new.result, old.result);
}

#[test]
fn a_raw_override_in_a_mesh_dictionary_moves_both_keys() {
    let old = keys(&cavity());
    let new = keys(&with("scale", block_mesh()));
    assert_ne!(new.mesh, old.mesh);
    assert_ne!(new.result, old.result);
}

#[test]
fn the_mesh_recipe_holds_no_label_and_the_result_recipe_holds_every_override_by_label() {
    let (a, b) = (
        recipes(&with("alpha", block_mesh())),
        recipes(&with("beta", block_mesh())),
    );
    assert_eq!(a.mesh.key(), b.mesh.key());
    assert_ne!(a.result.key(), b.result.key());
    let text = serde_json::to_string(&a.mesh).unwrap();
    assert!(!text.contains("alpha"), "{text}");
    assert_eq!(a.mesh.raw, vec![block_mesh()]);
    assert_eq!(a.result.raw.get("alpha"), Some(&block_mesh()));
    let mut both = with("relax-u", relax());
    both.raw_overrides.insert("scale".to_owned(), block_mesh());
    let held = recipes(&both);
    assert_eq!(held.mesh.raw, vec![block_mesh()]);
    assert_eq!(held.result.raw.len(), 2);
}

#[test]
fn the_order_of_overrides_in_the_mesh_recipe_does_not_depend_on_labels() {
    let first = block_mesh();
    let second = raw("system/blockMeshDict", "vertices", "(0 0 0)");
    let mut a = cavity();
    a.raw_overrides.insert("a".to_owned(), first.clone());
    a.raw_overrides.insert("b".to_owned(), second.clone());
    let mut b = cavity();
    b.raw_overrides.insert("a".to_owned(), second);
    b.raw_overrides.insert("b".to_owned(), first);
    assert_eq!(recipes(&a).mesh.raw, recipes(&b).mesh.raw);
    assert_eq!(recipes(&a).mesh.key(), recipes(&b).mesh.key());
}

#[test]
fn the_value_and_the_backend_of_an_override_change_the_key() {
    let base = keys(&with("relax-u", relax())).result;
    type Edit = (&'static str, fn(&mut RawOverride));
    let edits: [Edit; 4] = [
        ("value", |r| r.value = "0.7".to_owned()),
        ("entry", |r| {
            r.entry = "relaxationFactors/equations/p".to_owned()
        }),
        ("file", |r| r.file = "system/fvSchemes".to_owned()),
        ("backend", |r| r.backend = RawBackend::Foundation),
    ];
    for (what, edit) in edits {
        let mut item = relax();
        edit(&mut item);
        assert_ne!(keys(&with("relax-u", item)).result, base, "{what}");
    }
}

#[test]
fn recipes_written_before_raw_overrides_still_read() {
    let both = recipes(&cavity());
    let mut mesh = serde_json::to_value(&both.mesh).unwrap();
    let mut result = serde_json::to_value(&both.result).unwrap();
    assert!(mesh.get("raw").is_none());
    assert!(result.get("raw").is_none());
    mesh.as_object_mut().unwrap().remove("raw");
    result.as_object_mut().unwrap().remove("raw");
    let mesh: MeshRecipe = serde_json::from_value(mesh).unwrap();
    let result: ResultRecipe = serde_json::from_value(result).unwrap();
    assert!(mesh.raw.is_empty());
    assert!(result.raw.is_empty());
    assert_eq!(mesh.key(), both.mesh.key());
    assert_eq!(result.key(), both.result.key());
}
