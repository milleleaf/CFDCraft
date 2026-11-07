//! The mesh level, wall grading and shape are part of the mesh recipe, the diff names them, and a
//! study that states none of them keeps its recipe text.

use cfdcraft_keys::{Axis, Change, MeshRecipe, StageRecipes, ToolIdentity, diff_mesh};
use cfdcraft_model::{Study, resolve};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

/// The mesh recipe text of the cavity before the level, grading and shape existed.
const CAVITY_MESH_TEXT: &str = concat!(
    r#"{"cells":{"x":128,"y":128,"z":1},"#,
    r#""geometry":{"length_x":0.1,"length_y":0.1,"length_z":0.05,"open":false},"#,
    r#""patches":[{"faces":["YMax"],"kind":"MovingWall"},"#,
    r#"{"faces":["XMin","XMax","YMin"],"kind":"NoSlipWall"}],"#,
    r#""tool":{"dialect":"esi-v2512","image_digest":"sha256:aa","sidecar_lock":null,"writer_version":"0.1.0"}}"#,
);

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn recipes(value: &Value) -> StageRecipes {
    let study = Study::from_json(&value.to_string()).unwrap();
    StageRecipes::from_resolved(&resolve(&study).unwrap(), &tool())
}

fn mesh(value: &Value) -> MeshRecipe {
    recipes(value).mesh
}

fn cavity() -> Value {
    serde_json::from_str(CAVITY).unwrap()
}

/// The channel with a 12.7 mm backward step in a 0.1143 m high domain.
fn step(height_mm: f64) -> Value {
    let mut v: Value = serde_json::from_str(CHANNEL).unwrap();
    v["domain"]["length_y"] = json!({"value": 0.1143, "unit": "m"});
    v["domain"]["shape"] = json!({
        "kind": "backward_step",
        "step_height": {"value": height_mm, "unit": "mm"},
        "upstream_length": {"value": 0.4, "unit": "m"},
        "startup_length": {"value": 0.0, "unit": "m"},
    });
    // A step lays out its own blocks: 218 cells along x without a start-up section, 114 along y.
    v["mesh"]["cells_x"] = 218.into();
    v["mesh"]["cells_y"] = 114.into();
    v
}

#[test]
fn a_study_without_the_new_members_has_the_same_mesh_recipe_text() {
    let text = serde_jcs::to_string(&mesh(&cavity())).unwrap();
    assert_eq!(text, CAVITY_MESH_TEXT);
    for word in ["level", "grading", "shape"] {
        assert!(!text.contains(word), "{word}");
    }
}

#[test]
fn a_level_changes_the_mesh_key_and_only_the_mesh_key_inputs() {
    let base = recipes(&cavity());
    let mut refined = cavity();
    refined["mesh"]["level"] = 1.into();
    let refined = recipes(&refined);
    assert_ne!(base.keys().mesh, refined.keys().mesh);
    assert_eq!(refined.mesh.level, 1);
    assert_eq!(refined.mesh.cells.x, 256);
    let mut longer = cavity();
    longer["solver"]["end_time"]["value"] = 10_000.into();
    assert_eq!(base.keys().mesh, recipes(&longer).keys().mesh);
    assert_ne!(base.keys().result, recipes(&longer).keys().result);
}

#[test]
fn grading_and_shape_change_the_mesh_key() {
    let mut even = cavity();
    even["mesh"]["grading"] = json!({"x": 1.0});
    let mut graded = cavity();
    graded["mesh"]["grading"] = json!({"x": 1.05});
    // A ratio of 1 is an even mesh, the same as no grading at all.
    assert_eq!(mesh(&even).key(), mesh(&cavity()).key());
    assert_ne!(mesh(&graded).key(), mesh(&even).key());
    let channel: Value = serde_json::from_str(CHANNEL).unwrap();
    let mut stepped = step(12.7);
    stepped["domain"]["length_y"] = channel["domain"]["length_y"].clone();
    assert_ne!(mesh(&stepped).key(), mesh(&channel).key());
    assert_ne!(mesh(&step(12.7)).key(), mesh(&step(10.0)).key());
}

#[test]
fn the_diff_names_a_level_grading_or_shape_change() {
    let base = mesh(&cavity());
    assert!(diff_mesh(&base, &base).is_empty());
    let mut refined = cavity();
    refined["mesh"]["level"] = 1.into();
    let changes = diff_mesh(&base, &mesh(&refined));
    assert!(changes.contains(&Change::MeshLevel { from: 0, to: 1 }));
    assert!(changes.contains(&Change::Cells {
        axis: Axis::X,
        from: 128,
        to: 256
    }));
    let mut graded = cavity();
    graded["mesh"]["grading"] = json!({"x": 1.05});
    assert_eq!(
        diff_mesh(&base, &mesh(&graded)),
        [Change::Grading {
            axis: Axis::X,
            from: 1.0,
            to: 1.05
        }]
    );
    let channel: Value = serde_json::from_str(CHANNEL).unwrap();
    let mut stepped = step(12.7);
    stepped["domain"]["length_y"] = channel["domain"]["length_y"].clone();
    let changes = diff_mesh(&mesh(&channel), &mesh(&stepped));
    let Some(Change::Shape { from, to }) =
        changes.iter().find(|c| matches!(c, Change::Shape { .. }))
    else {
        panic!("{changes:?}");
    };
    assert_eq!(*from, None);
    assert!(to.as_deref().unwrap().contains("0.0127 m"), "{to:?}");
}
