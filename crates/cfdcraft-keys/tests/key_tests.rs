use cfdcraft_keys::{
    CellsRecipe, GeometryRecipe, MeshRecipe, PatchRecipe, StageKey, StageKeys, StageRecipes,
    ToolIdentity,
};
use cfdcraft_model::{BoundaryKind, Face, Study, Unit, resolve};

fn cavity() -> Study {
    Study::from_json(include_str!(
        "../../cfdcraft-model/tests/fixtures/cavity_v1.json"
    ))
    .unwrap()
}

fn tool() -> ToolIdentity {
    ToolIdentity::new("sha256:aa", "esi-v2512", None, "0.1.0")
}

fn recipes(study: &Study) -> StageRecipes {
    StageRecipes::from_resolved(&resolve(study).unwrap(), &tool())
}

fn keys(study: &Study) -> StageKeys {
    recipes(study).keys()
}

#[test]
fn the_cavity_gives_a_mesh_key_and_a_result_key() {
    let StageKeys { mesh, result } = keys(&cavity());
    for key in [mesh, result] {
        let text = key.to_string();
        assert_eq!(text.len(), 64);
        assert!(
            text.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
        assert_eq!(StageKey::parse(&text).unwrap(), key);
        assert_eq!(text.parse::<StageKey>().unwrap(), key);
    }
    assert_ne!(mesh, result);
    let text = mesh.to_string();
    assert!(StageKey::parse(&text.to_uppercase()).is_err());
    assert!(StageKey::parse(&text[1..]).is_err());
    assert!(StageKey::parse("../../etc/passwd").is_err());
}

#[test]
fn the_same_study_gives_the_same_keys_every_time() {
    let study = cavity();
    let expected = keys(&study);
    assert_eq!(keys(&study), expected);
    let value = serde_json::to_value(&study).unwrap();
    for json in [
        serde_json::to_string(&value).unwrap(),
        serde_json::to_string_pretty(&value).unwrap(),
    ] {
        assert_eq!(keys(&Study::from_json(&json).unwrap()), expected);
    }
}

#[test]
fn equal_values_in_other_units_give_equal_keys() {
    let expected = keys(&cavity());
    let mut study = cavity();
    study.domain.length_x.value = 100.0;
    study.domain.length_x.unit = Unit::Millimetre;
    assert_eq!(keys(&study), expected);
}

#[test]
fn the_lid_speed_changes_the_result_key_and_not_the_mesh_key() {
    let before = keys(&cavity());
    let mut study = cavity();
    study
        .boundary_conditions
        .get_mut("lid")
        .unwrap()
        .velocity
        .as_mut()
        .unwrap()
        .value = 0.2;
    let after = keys(&study);
    assert_eq!(after.mesh, before.mesh);
    assert_ne!(after.result, before.result);
}

#[test]
fn the_cell_count_changes_both_keys() {
    let before = keys(&cavity());
    let mut study = cavity();
    study.mesh.cells_x = 256;
    let after = keys(&study);
    assert_ne!(after.mesh, before.mesh);
    assert_ne!(after.result, before.result);
}

#[test]
fn the_result_recipe_holds_the_mesh_key_not_the_mesh_parameters() {
    let recipes = recipes(&cavity());
    let value = serde_json::to_value(&recipes.result).unwrap();
    assert_eq!(
        value["mesh_key"].as_str(),
        Some(recipes.mesh.key().to_string().as_str())
    );
    assert!(value.get("cells").is_none());
    assert!(value.get("geometry").is_none());
}

#[test]
fn the_mesh_recipe_never_holds_a_boundary_label() {
    let mesh = recipes(&cavity()).mesh;
    let text = serde_json::to_string(&mesh).unwrap();
    assert!(!text.contains("lid"), "{text}");
    assert!(!text.contains("walls"), "{text}");
    assert_eq!(mesh.patches.len(), 2);
    assert!(mesh.patches.contains(&PatchRecipe {
        kind: BoundaryKind::MovingWall,
        faces: vec![Face::YMax],
        thermal: None,
    }));
    assert!(mesh.patches.contains(&PatchRecipe {
        kind: BoundaryKind::NoSlipWall,
        faces: vec![Face::XMin, Face::XMax, Face::YMin],
        thermal: None,
    }));
}

#[test]
fn a_tiny_recipe_has_the_pinned_text() {
    let recipe = MeshRecipe {
        tool: tool(),
        geometry: GeometryRecipe {
            length_x: 1.0,
            length_y: 0.5,
            length_z: 2.0,
            open: false,
        },
        patches: vec![PatchRecipe {
            kind: BoundaryKind::NoSlipWall,
            faces: vec![Face::XMin],
            thermal: None,
        }],
        cells: CellsRecipe { x: 2, y: 3, z: 1 },
        level: 0,
        grading: None,
        shape: None,
        raw: Vec::new(),
    };
    assert_eq!(
        serde_jcs::to_string(&recipe).unwrap(),
        r#"{"cells":{"x":2,"y":3,"z":1},"geometry":{"length_x":1,"length_y":0.5,"length_z":2,"open":false},"patches":[{"faces":["XMin"],"kind":"NoSlipWall"}],"tool":{"dialect":"esi-v2512","image_digest":"sha256:aa","sidecar_lock":null,"writer_version":"0.1.0"}}"#
    );
}

#[test]
fn the_geometry_hash_follows_only_the_geometry() {
    let base = recipes(&cavity()).mesh;
    let mut more_cells = cavity();
    more_cells.mesh.cells_y = 64;
    assert_eq!(
        recipes(&more_cells).mesh.geometry.hash(),
        base.geometry.hash()
    );
    let mut longer = cavity();
    longer.domain.length_y.value = 0.2;
    assert_ne!(recipes(&longer).mesh.geometry.hash(), base.geometry.hash());
}
