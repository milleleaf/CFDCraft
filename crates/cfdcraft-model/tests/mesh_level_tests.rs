use cfdcraft_model::{Study, canonical_hash, resolve};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");

/// The canonical text of the cavity as it was before the mesh level existed.
const CAVITY_TEXT: &str = concat!(
    r#"{"boundary_conditions":{"lid":{"faces":["YMax"],"kind":"MovingWall","velocity":0.1},"#,
    r#""walls":{"faces":["XMin","XMax","YMin"],"kind":"NoSlipWall"}},"#,
    r#""domain":{"length_x":0.1,"length_y":0.1,"length_z":0.05,"open":false},"#,
    r#""fluid":{"kinematic_viscosity":0.0001},"#,
    r#""mesh":{"cells_x":128,"cells_y":128,"cells_z":1},"#,
    r#""monitors":{"centreline_u_sample_count":64},"schema_version":1,"#,
    r#""solver":{"convergence":{"U":0.000001,"p":0.000001},"end_time":5000,"write_interval":500}}"#,
);

const CAVITY_HASH: &str = "ace6717c154b77f06664534043296ec96e001fe8dc0b000b16af327c573bd1c0";

/// The cavity with `"level": level` in its mesh, or without the member for `None`.
fn study_at(level: Option<u32>) -> Study {
    let mut value: serde_json::Value = serde_json::from_str(CAVITY).unwrap();
    if let Some(level) = level {
        value["mesh"]["level"] = level.into();
    }
    Study::from_json(&value.to_string()).unwrap()
}

fn text(study: &Study) -> String {
    serde_jcs::to_string(&resolve(study).unwrap()).unwrap()
}

#[test]
fn a_study_without_a_level_keeps_its_hash() {
    let study = study_at(None);
    let canonical = text(&study);
    assert_eq!(canonical, CAVITY_TEXT);
    assert!(!canonical.contains("level") && !canonical.contains("grading"));
    assert_eq!(canonical_hash(&resolve(&study).unwrap()), CAVITY_HASH);
}

#[test]
fn level_two_multiplies_the_cells_by_four_per_direction_pair() {
    let study = study_at(Some(2));
    let resolved = resolve(&study).unwrap();
    assert_eq!(
        (
            resolved.mesh.cells_x,
            resolved.mesh.cells_y,
            resolved.mesh.cells_z
        ),
        (512, 512, 1)
    );
    assert_eq!(resolved.mesh.level, 2);
    assert!(text(&study).contains(r#""level":2"#));
    assert_ne!(canonical_hash(&resolved), CAVITY_HASH);
}

#[test]
fn z_stays_one_cell() {
    for level in 0..=4 {
        let resolved = resolve(&study_at(Some(level))).unwrap();
        assert_eq!(resolved.mesh.cells_z, 1, "level {level}");
        assert_eq!(resolved.mesh.cells_x, 128 << level, "level {level}");
        assert_eq!(resolved.mesh.cells_y, 128 << level, "level {level}");
    }
}

#[test]
fn level_zero_written_explicitly_is_the_same_study_hash() {
    let explicit = canonical_hash(&resolve(&study_at(Some(0))).unwrap());
    assert_eq!(explicit, CAVITY_HASH);
}

#[test]
fn the_level_is_part_of_the_study_text_only_when_set() {
    let plain = serde_json::to_value(study_at(Some(0))).unwrap();
    assert!(plain["mesh"].get("level").is_none());
    let refined = serde_json::to_value(study_at(Some(3))).unwrap();
    assert_eq!(refined["mesh"]["level"], 3);
}
