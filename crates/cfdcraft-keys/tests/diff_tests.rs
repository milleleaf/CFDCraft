use cfdcraft_keys::{Axis, Change, StageRecipes, ToolIdentity, ToolPart, diff_mesh, diff_result};
use cfdcraft_model::{ResolvedStudy, Study, resolve};

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

fn recipes(resolved: &ResolvedStudy, tool: &ToolIdentity) -> StageRecipes {
    StageRecipes::from_resolved(resolved, tool)
}

#[test]
fn a_lid_speed_change_is_one_boundary_velocity_change_on_the_result() {
    let old = cavity();
    let mut new = old.clone();
    let from = old.boundary_conditions["lid"].velocity;
    assert!(from.is_some());
    new.boundary_conditions.get_mut("lid").unwrap().velocity = Some(0.2);
    let (a, b) = (recipes(&old, &tool()), recipes(&new, &tool()));
    assert_eq!(
        diff_result(&a.result, &b.result),
        vec![Change::BoundaryVelocity {
            boundary: "lid".to_owned(),
            from,
            to: Some(0.2),
        }]
    );
    assert!(diff_mesh(&a.mesh, &b.mesh).is_empty());
}

#[test]
fn a_cell_count_change_is_a_cells_change_on_the_mesh_and_the_mesh_on_the_result() {
    let old = cavity();
    let mut new = old.clone();
    new.mesh.cells_x = 256;
    let (a, b) = (recipes(&old, &tool()), recipes(&new, &tool()));
    assert_eq!(
        diff_mesh(&a.mesh, &b.mesh),
        vec![Change::Cells {
            axis: Axis::X,
            from: 128,
            to: 256,
        }]
    );
    assert_eq!(diff_result(&a.result, &b.result), vec![Change::MeshChanged]);
}

#[test]
fn equal_recipes_have_an_empty_diff() {
    let a = recipes(&cavity(), &tool());
    let b = recipes(&cavity(), &tool());
    assert!(diff_mesh(&a.mesh, &a.mesh).is_empty());
    assert!(diff_result(&a.result, &a.result).is_empty());
    assert!(diff_mesh(&a.mesh, &b.mesh).is_empty());
    assert!(diff_result(&a.result, &b.result).is_empty());
}

/// A tool change also changes the mesh key, so the result recipe reports its mesh as changed too.
fn with_mesh(expected: &[Change]) -> Vec<Change> {
    let mut all = expected.to_vec();
    all.push(Change::MeshChanged);
    all
}

#[test]
fn a_tool_change_is_named_part_by_part() {
    let resolved = cavity();
    let old = recipes(&resolved, &tool());

    let mut image = tool();
    image.image_digest = "sha256:bb".to_owned();
    let new = recipes(&resolved, &image);
    let expected = vec![Change::Tool {
        part: ToolPart::Image,
        from: "sha256:aa".to_owned(),
        to: "sha256:bb".to_owned(),
    }];
    assert_eq!(diff_mesh(&old.mesh, &new.mesh), expected);
    assert_eq!(diff_result(&old.result, &new.result), with_mesh(&expected));

    let mut sidecar = tool();
    sidecar.sidecar_lock = Some("lock-1".to_owned());
    let new = recipes(&resolved, &sidecar);
    let expected = vec![Change::Tool {
        part: ToolPart::Sidecar,
        from: "none".to_owned(),
        to: "lock-1".to_owned(),
    }];
    assert_eq!(diff_mesh(&old.mesh, &new.mesh), expected);
    assert_eq!(diff_result(&old.result, &new.result), with_mesh(&expected));
}

#[test]
fn changes_serialise_with_stable_names() {
    let length = serde_json::to_value(Change::Length {
        axis: Axis::Y,
        from: 1.0,
        to: 2.0,
    })
    .unwrap();
    assert_eq!(length["kind"], "length");
    assert_eq!(length["axis"], "y");
    assert_eq!(length["from"], 1.0);
    assert_eq!(length["to"], 2.0);
    let mesh = serde_json::to_value(Change::MeshChanged).unwrap();
    assert!(mesh.is_object());
    assert_eq!(mesh["kind"], "mesh_changed");
    let back: Change = serde_json::from_value(length).unwrap();
    assert_eq!(
        back,
        Change::Length {
            axis: Axis::Y,
            from: 1.0,
            to: 2.0,
        }
    );
}
