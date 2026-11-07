use cfdcraft_keys::{StageKeys, StageRecipes, ToolIdentity};
use cfdcraft_model::{BoundaryKind, ResolvedStudy, Study, resolve};

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

fn keys(resolved: &ResolvedStudy, tool: &ToolIdentity) -> StageKeys {
    StageRecipes::from_resolved(resolved, tool).keys()
}

/// Both keys of the cavity move when only the tool identity changes from `a` to `b`.
fn assert_both_move(a: &ToolIdentity, b: &ToolIdentity) {
    let resolved = cavity();
    let (a, b) = (keys(&resolved, a), keys(&resolved, b));
    assert_ne!(a.mesh, b.mesh);
    assert_ne!(a.result, b.result);
}

#[test]
fn a_changed_image_digest_changes_both_keys() {
    let mut other = tool();
    other.image_digest = "sha256:bb".to_owned();
    assert_both_move(&tool(), &other);
}

#[test]
fn a_changed_dialect_changes_both_keys() {
    let mut other = tool();
    other.dialect = "esi-v2606".to_owned();
    assert_both_move(&tool(), &other);
}

#[test]
fn a_changed_sidecar_lock_changes_both_keys() {
    let mut one = tool();
    one.sidecar_lock = Some("lock-1".to_owned());
    let mut two = tool();
    two.sidecar_lock = Some("lock-2".to_owned());
    assert_both_move(&tool(), &one);
    assert_both_move(&one, &two);
    assert_both_move(&tool(), &two);
}

#[test]
fn a_changed_writer_version_changes_both_keys() {
    let mut other = tool();
    other.writer_version = "0.2.0".to_owned();
    assert_both_move(&tool(), &other);
}

#[test]
fn an_absent_sidecar_lock_differs_from_a_present_empty_one() {
    let mut empty = tool();
    empty.sidecar_lock = Some(String::new());
    assert_both_move(&tool(), &empty);
}

type Row = (&'static str, fn(&mut ResolvedStudy), bool);

#[test]
fn every_study_setting_moves_exactly_the_keys_it_should() {
    let rows: [Row; 21] = [
        ("length x", |r| r.domain.length_x *= 2.0, true),
        ("length y", |r| r.domain.length_y *= 2.0, true),
        ("length z", |r| r.domain.length_z *= 2.0, true),
        ("domain open", |r| r.domain.open = !r.domain.open, true),
        ("cells x", |r| r.mesh.cells_x += 1, true),
        ("cells y", |r| r.mesh.cells_y += 1, true),
        ("cells z", |r| r.mesh.cells_z += 1, true),
        (
            "walls kind",
            |r| {
                r.boundary_conditions.get_mut("walls").unwrap().kind = BoundaryKind::SlipWall;
            },
            true,
        ),
        (
            "walls faces",
            |r| {
                r.boundary_conditions.get_mut("walls").unwrap().faces.pop();
            },
            true,
        ),
        ("viscosity", |r| r.fluid.kinematic_viscosity *= 2.0, false),
        (
            "lid velocity",
            |r| {
                let lid = r.boundary_conditions.get_mut("lid").unwrap();
                lid.velocity = lid.velocity.map(|v| v * 2.0);
            },
            false,
        ),
        (
            "lid velocity removed",
            |r| r.boundary_conditions.get_mut("lid").unwrap().velocity = None,
            false,
        ),
        ("end time", |r| r.solver.end_time *= 2.0, false),
        ("write interval", |r| r.solver.write_interval *= 2.0, false),
        ("convergence p", |r| r.solver.convergence.p *= 2.0, false),
        ("convergence u", |r| r.solver.convergence.u *= 2.0, false),
        (
            "monitor samples",
            |r| r.monitors.centreline_u_sample_count += 1,
            false,
        ),
        (
            "lid kind",
            |r| r.boundary_conditions.get_mut("lid").unwrap().kind = BoundaryKind::NoSlipWall,
            true,
        ),
        (
            "lid faces",
            |r| r.boundary_conditions.get_mut("lid").unwrap().faces.clear(),
            true,
        ),
        (
            "walls velocity added",
            |r| r.boundary_conditions.get_mut("walls").unwrap().velocity = Some(1.0),
            false,
        ),
        (
            "a boundary added",
            |r| {
                let lid = r.boundary_conditions["lid"].clone();
                r.boundary_conditions.insert("extra".to_owned(), lid);
            },
            true,
        ),
    ];
    let original = cavity();
    let before = keys(&original, &tool());
    for (name, change, moves_mesh) in rows {
        let mut changed = original.clone();
        change(&mut changed);
        let after = keys(&changed, &tool());
        assert_ne!(after.result, before.result, "{name}: the result key");
        assert_eq!(
            after.mesh != before.mesh,
            moves_mesh,
            "{name}: the mesh key"
        );
    }
}

#[test]
fn a_boundary_rename_stales_the_result_and_not_the_mesh() {
    let original = cavity();
    let before = keys(&original, &tool());
    let mut renamed = original.clone();
    let lid = renamed.boundary_conditions.remove("lid").unwrap();
    renamed.boundary_conditions.insert("top".to_owned(), lid);
    let after = keys(&renamed, &tool());
    assert_eq!(after.mesh, before.mesh);
    assert_ne!(after.result, before.result);
}

#[test]
fn mutating_the_tool_identity_inside_a_recipe_changes_its_own_key() {
    let mut recipes = StageRecipes::from_resolved(&cavity(), &tool());
    let (mesh, result) = (recipes.mesh.key(), recipes.result.key());
    recipes.mesh.tool.writer_version = "9.9.9".to_owned();
    recipes.result.tool.writer_version = "9.9.9".to_owned();
    assert_ne!(recipes.mesh.key(), mesh);
    assert_ne!(recipes.result.key(), result);
}
