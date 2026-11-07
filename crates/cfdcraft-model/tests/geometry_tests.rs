use cfdcraft_model::diagnostics::CODES;
use cfdcraft_model::{
    Capabilities, Diagnostic, Grading, ResolvedGrading, ResolvedShape, Severity, Study, diagnose,
    is_known_code, resolve,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("fixtures/channel_v1.json");

fn value(json: &str) -> Value {
    serde_json::from_str(json).unwrap()
}

fn study(value: &Value) -> Study {
    Study::from_json(&value.to_string()).unwrap()
}

fn errors(study: &Study) -> Vec<Diagnostic> {
    diagnose(study, &Capabilities::unrestricted())
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn has(found: &[Diagnostic], code: &str, pointer: &str) -> bool {
    found.iter().any(|d| d.code == code && d.pointer == pointer)
}

fn mm(value: f64) -> Value {
    json!({"value": value, "unit": "mm"})
}

fn m(value: f64) -> Value {
    json!({"value": value, "unit": "m"})
}

/// The reference backward step: 12.7 mm step in a 2.286 m by 0.1143 m channel.
fn step() -> Value {
    let mut v = value(CHANNEL);
    v["domain"]["length_x"] = m(2.286);
    v["domain"]["length_y"] = m(0.1143);
    v["domain"]["shape"] = json!({
        "kind": "backward_step",
        "step_height": mm(12.7),
        "upstream_length": m(1.651),
        "startup_length": m(0.254),
    });
    // A step lays out its own blocks: 220 cells along x with the start-up section, 114 along y.
    v["mesh"]["cells_x"] = 220.into();
    v["mesh"]["cells_y"] = 114.into();
    v
}

/// A cylinder channel: 0.1 m across at (0.2, 0.2) in 2.2 m by 0.41 m, with its own wall.
fn cylinder() -> Value {
    let mut v = value(CHANNEL);
    v["domain"]["length_x"] = m(2.2);
    v["domain"]["length_y"] = m(0.41);
    v["domain"]["shape"] = json!({
        "kind": "cylinder_channel",
        "diameter": m(0.1),
        "centre_x": m(0.2),
        "centre_y": m(0.2),
    });
    v["boundary_conditions"]["cylinder"] = json!({"faces": ["Obstacle"], "kind": "NoSlipWall"});
    // A cylinder channel lays out its own blocks: 114 cells along x and 44 along y.
    v["mesh"]["cells_x"] = 114.into();
    v["mesh"]["cells_y"] = 44.into();
    v
}

#[test]
fn a_box_with_wall_grading_resolves() {
    let mut v = value(CAVITY);
    v["mesh"]["cells_x"] = 40.into();
    v["mesh"]["cells_y"] = 40.into();
    v["mesh"]["grading"] = json!({"x": 1.05, "y": 1.05});
    let s = study(&v);
    assert!(errors(&s).is_empty());
    let resolved = resolve(&s).unwrap();
    assert_eq!(
        resolved.mesh.grading,
        Some(ResolvedGrading {
            x: Some(1.05),
            y: Some(1.05)
        })
    );
    assert!(
        serde_jcs::to_string(&resolved)
            .unwrap()
            .contains(r#""grading":{"x":1.05,"y":1.05}"#)
    );
    v["mesh"]["cells_x"] = 41.into();
    assert!(has(&errors(&study(&v)), "CFD-MESH-002", "/mesh/grading/x"));
}

#[test]
fn a_ratio_of_one_is_no_grading() {
    let mut v = value(CAVITY);
    v["mesh"]["grading"] = json!({"x": 1.0});
    let resolved = resolve(&study(&v)).unwrap();
    assert_eq!(resolved.mesh.grading, None);
}

#[test]
fn grading_outside_one_to_one_point_five_is_refused() {
    for bad in [0.9, 1.6] {
        let mut v = value(CAVITY);
        v["mesh"]["grading"] = json!({"x": bad});
        let found = errors(&study(&v));
        assert!(has(&found, "CFD-MESH-002", "/mesh/grading/x"), "{bad}");
    }
    let mut s = study(&value(CAVITY));
    s.mesh.grading = Some(Grading {
        x: None,
        y: Some(f64::NAN),
    });
    assert!(has(&errors(&s), "CFD-MESH-002", "/mesh/grading/y"));
    let mut v = step();
    v["mesh"]["grading"] = json!({"y": 1.1});
    assert!(has(&errors(&study(&v)), "CFD-MESH-002", "/mesh/grading"));
}

#[test]
fn level_above_four_or_too_many_cells_is_refused() {
    let mut v = value(CAVITY);
    v["mesh"]["level"] = 5.into();
    assert!(has(&errors(&study(&v)), "CFD-MESH-002", "/mesh/level"));
    let mut v = value(CAVITY);
    v["mesh"]["cells_x"] = 40_000.into();
    v["mesh"]["cells_y"] = 40_000.into();
    assert!(has(&errors(&study(&v)), "CFD-MESH-002", "/mesh"));
    let mut v = value(CAVITY);
    v["mesh"]["cells_x"] = u64::MAX.into();
    assert!(has(&errors(&study(&v)), "CFD-MESH-002", "/mesh"));
    // The level counts: 8192 by 8192 cells is exactly the limit (2^26), twice that is over it.
    let mut v = value(CAVITY);
    v["mesh"]["cells_x"] = 512.into();
    v["mesh"]["cells_y"] = 512.into();
    v["mesh"]["level"] = 4.into();
    assert!(errors(&study(&v)).is_empty());
    v["mesh"]["cells_x"] = 1024.into();
    v["mesh"]["cells_y"] = 1024.into();
    assert!(has(&errors(&study(&v)), "CFD-MESH-002", "/mesh"));
}

#[test]
fn a_backward_step_study_resolves() {
    let s = study(&step());
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    let resolved = resolve(&s).unwrap();
    assert_eq!(
        resolved.domain.shape,
        Some(ResolvedShape::BackwardStep {
            step_height: 0.0127,
            upstream_length: 1.651,
            startup_length: 0.254,
        })
    );
    let text = serde_jcs::to_string(&resolved).unwrap();
    assert!(text.contains(r#""kind":"backward_step""#) && text.contains("0.0127"));
}

#[test]
fn step_sizes_that_do_not_fit_are_refused() {
    for (member, bad, pointer) in [
        ("step_height", m(0.0), "/domain/shape/step_height"),
        ("step_height", m(0.1143), "/domain/shape/step_height"),
        ("upstream_length", m(2.286), "/domain/shape/upstream_length"),
        ("startup_length", m(1.7), "/domain/shape/startup_length"),
        ("startup_length", m(-0.1), "/domain/shape/startup_length"),
    ] {
        let mut v = step();
        v["domain"]["shape"][member] = bad;
        assert!(
            has(&errors(&study(&v)), "CFD-SHAPE-001", pointer),
            "{member}"
        );
    }
    // No start-up section is allowed.
    let mut v = step();
    v["domain"]["shape"]["startup_length"] = m(0.0);
    v["mesh"]["cells_x"] = 218.into();
    assert!(errors(&study(&v)).is_empty());
    for mut v in [step(), cylinder()] {
        v["domain"]["open"] = false.into();
        assert!(has(&errors(&study(&v)), "CFD-SHAPE-002", "/domain/open"));
    }
}

#[test]
fn a_cylinder_channel_resolves() {
    let s = study(&cylinder());
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    let resolved = resolve(&s).unwrap();
    assert_eq!(
        resolved.domain.shape,
        Some(ResolvedShape::CylinderChannel {
            diameter: 0.1,
            centre_x: 0.2,
            centre_y: 0.2,
        })
    );
}

#[test]
fn the_cylinder_must_leave_room_for_its_mesh() {
    for (member, bad) in [
        ("centre_x", 0.1),
        ("centre_x", 2.15),
        ("centre_y", 0.05),
        ("centre_y", 0.35),
    ] {
        let mut v = cylinder();
        v["domain"]["shape"][member] = m(bad);
        let pointer = format!("/domain/shape/{member}");
        assert!(
            has(&errors(&study(&v)), "CFD-SHAPE-001", &pointer),
            "{member} {bad}"
        );
    }
    let mut v = cylinder();
    v["domain"]["shape"]["diameter"] = m(0.0);
    assert!(has(
        &errors(&study(&v)),
        "CFD-SHAPE-001",
        "/domain/shape/diameter"
    ));
}

#[test]
fn the_obstacle_face_needs_a_cylinder_and_a_wall() {
    for mut v in [value(CHANNEL), step()] {
        v["boundary_conditions"]["floor"]["faces"] = json!(["YMin", "Obstacle"]);
        let found = errors(&study(&v));
        assert!(has(
            &found,
            "CFD-SHAPE-003",
            "/boundary_conditions/floor/faces"
        ));
        assert!(!found.iter().any(|d| d.code == "CFD-BC-002"), "{found:?}");
    }
    let mut v = cylinder();
    v["boundary_conditions"]
        .as_object_mut()
        .unwrap()
        .remove("cylinder");
    let found = errors(&study(&v));
    assert!(has(&found, "CFD-SHAPE-003", "/boundary_conditions"));
    assert!(!found.iter().any(|d| d.code == "CFD-BC-001"), "{found:?}");
    let mut v = cylinder();
    v["boundary_conditions"]["cylinder"]["kind"] = "PressureOutlet".into();
    assert!(has(
        &errors(&study(&v)),
        "CFD-SHAPE-003",
        "/boundary_conditions/cylinder/kind"
    ));
    // Obstacle is a face to be covered exactly once, on a cylinder channel only.
    let mut v = cylinder();
    v["boundary_conditions"]["second"] = json!({"faces": ["Obstacle"], "kind": "NoSlipWall"});
    assert!(errors(&study(&v)).iter().any(|d| d.code == "CFD-BC-002"));
}

#[test]
fn the_catalogue_has_one_hundred_and_twenty_four_entries() {
    assert_eq!(CODES.len(), 124);
    for code in [
        "CFD-MESH-002",
        "CFD-SHAPE-001",
        "CFD-SHAPE-002",
        "CFD-SHAPE-003",
        "CFD-SHAPE-004",
        "CFD-BC-007",
        "CFD-PHYS-001",
        "CFD-PHYS-002",
        "CFD-PHYS-003",
        "CFD-PHYS-004",
        "CFD-PHYS-005",
        "CFD-PHYS-006",
        "CFD-PHYS-007",
        "CFD-MON-007",
        "CFD-MON-008",
        "CFD-MON-009",
        "CFD-BENCH-001",
        "CFD-BENCH-002",
        "CFD-BENCH-003",
        "CFD-BENCH-004",
        "CFD-GCI-001",
        "CFD-GCI-002",
    ] {
        assert!(is_known_code(code), "{code}");
    }
}
