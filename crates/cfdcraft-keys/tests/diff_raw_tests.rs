//! The difference between two recipes names a raw override by its label and shows both sides.

use cfdcraft_keys::{
    Change, ResultRecipe, StageRecipes, ToolIdentity, diff_mesh, diff_result, raw_text,
};
use cfdcraft_model::{RawBackend, RawOverride, ResolvedStudy, Study, resolve};

fn cavity() -> ResolvedStudy {
    let study = Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap();
    resolve(&study).unwrap()
}

fn recipes(resolved: &ResolvedStudy) -> StageRecipes {
    let tool = ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0");
    StageRecipes::from_resolved(resolved, &tool)
}

fn raw(file: &str, entry: &str, value: &str) -> RawOverride {
    RawOverride {
        backend: RawBackend::Esi,
        file: file.to_owned(),
        entry: entry.to_owned(),
        value: value.to_owned(),
    }
}

fn relax(value: &str) -> RawOverride {
    raw("system/fvSolution", "relaxationFactors/equations/U", value)
}

fn with(items: &[(&str, RawOverride)]) -> StageRecipes {
    let mut resolved = cavity();
    for (label, item) in items {
        resolved
            .raw_overrides
            .insert((*label).to_owned(), item.clone());
    }
    recipes(&resolved)
}

fn side(value: &str) -> Option<String> {
    Some(format!(
        "esi system/fvSolution relaxationFactors/equations/U = {value}"
    ))
}

fn change(id: &str, from: Option<String>, to: Option<String>) -> Change {
    Change::RawOverride {
        id: id.to_owned(),
        from,
        to,
    }
}

#[test]
fn a_raw_override_added_changed_and_removed_names_its_label() {
    let none = with(&[]);
    let half = with(&[("relax-u", relax("0.5"))]);
    let most = with(&[("relax-u", relax("0.7"))]);
    assert_eq!(
        diff_result(&none.result, &half.result),
        vec![change("relax-u", None, side("0.5"))]
    );
    assert_eq!(
        diff_result(&half.result, &most.result),
        vec![change("relax-u", side("0.5"), side("0.7"))]
    );
    assert_eq!(
        diff_result(&half.result, &none.result),
        vec![change("relax-u", side("0.5"), None)]
    );
    assert!(diff_result(&half.result, &half.result).is_empty());
    assert_eq!(
        raw_text(&relax("0.5")),
        "esi system/fvSolution relaxationFactors/equations/U = 0.5"
    );
}

#[test]
fn a_mesh_override_change_has_the_mesh_change() {
    let block = raw("system/blockMeshDict", "convertToMeters", "0.1");
    let other = raw("system/blockMeshDict", "convertToMeters", "0.2");
    let none = with(&[]);
    let one = with(&[("scale", block.clone())]);
    let two = with(&[("scale", other)]);
    assert_eq!(diff_mesh(&none.mesh, &one.mesh), vec![Change::RawMesh]);
    assert_eq!(diff_mesh(&one.mesh, &two.mesh), vec![Change::RawMesh]);
    assert!(diff_mesh(&one.mesh, &one.mesh).is_empty());
    // A rename moves no mesh change, only the result's.
    let renamed = with(&[("size", block)]);
    assert!(diff_mesh(&one.mesh, &renamed.mesh).is_empty());
    let found = diff_result(&one.result, &renamed.result);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(matches!(&found[0], Change::RawOverride { id, to: None, .. } if id == "scale"));
    assert!(matches!(&found[1], Change::RawOverride { id, from: None, .. } if id == "size"));
    // An override outside the mesh leaves the mesh diff empty.
    let outside = with(&[("relax-u", relax("0.5"))]);
    assert!(diff_mesh(&none.mesh, &outside.mesh).is_empty());
}

#[test]
fn raw_changes_come_last_and_in_label_order() {
    let mut old = cavity();
    old.raw_overrides.insert("b".to_owned(), relax("0.5"));
    old.raw_overrides.insert("c".to_owned(), relax("0.6"));
    let mut new = old.clone();
    new.solver.end_time *= 2.0;
    new.raw_overrides.remove("c");
    new.raw_overrides.insert("a".to_owned(), relax("0.4"));
    new.raw_overrides.insert("b".to_owned(), relax("0.9"));
    let (a, b) = (recipes(&old), recipes(&new));
    let found = diff_result(&a.result, &b.result);
    assert_eq!(found.len(), 4, "{found:?}");
    assert!(matches!(found[0], Change::EndTime { .. }));
    assert_eq!(found[1], change("a", None, side("0.4")));
    assert_eq!(found[2], change("b", side("0.5"), side("0.9")));
    assert_eq!(found[3], change("c", side("0.6"), None));
}

#[test]
fn a_raw_change_is_json_with_kind_raw_override() {
    let value = serde_json::to_value(change("relax-u", None, side("0.5"))).unwrap();
    assert_eq!(value["kind"], "raw_override");
    assert_eq!(value["id"], "relax-u");
    assert!(value["from"].is_null());
    assert_eq!(value["to"], side("0.5").unwrap());
    let mesh = serde_json::to_value(Change::RawMesh).unwrap();
    assert_eq!(mesh["kind"], "raw_mesh");
    let back: Change = serde_json::from_value(value).unwrap();
    assert_eq!(back, change("relax-u", None, side("0.5")));
}

#[test]
fn the_raw_member_of_a_result_recipe_is_read_back_by_its_label() {
    let both = with(&[("relax-u", relax("0.5"))]);
    let text = serde_json::to_string(&both.result).unwrap();
    let back: ResultRecipe = serde_json::from_str(&text).unwrap();
    assert_eq!(back, both.result);
    assert!(back.raw.contains_key("relax-u"));
}
