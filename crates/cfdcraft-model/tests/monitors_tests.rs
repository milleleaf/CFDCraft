use cfdcraft_model::{
    BoundaryKind, LineMonitor, MonitorField, MonitorPlan, PlannedForce, PlannedLine, PlannedProbe,
    PlannedSurface, Probe, SettleSettings, Study, SurfaceMonitor, SurfaceQuantity, canonical_hash,
    resolve,
};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");
const CAVITY_V0: &str = include_str!("fixtures/cavity_v0.json");
const CHANNEL: &str = include_str!("fixtures/channel_v1.json");

fn cavity() -> Study {
    Study::from_json(CAVITY).unwrap()
}

/// The cavity with these members added to its `monitors` object.
fn with_monitors(extra: Value) -> Study {
    let mut value: Value = serde_json::from_str(CAVITY).unwrap();
    for (key, member) in extra.as_object().unwrap() {
        value["monitors"][key] = member.clone();
    }
    Study::from_json(&value.to_string()).unwrap()
}

fn probe_at(x: f64) -> Value {
    json!({"x": {"value": x, "unit": "m"}, "y": {"value": 0.05, "unit": "m"}, "field": "velocity"})
}

fn listed() -> Value {
    json!({
        "probes": {"centre": {
            "x": {"value": 50.0, "unit": "mm"},
            "y": {"value": 0.05, "unit": "m"},
            "field": "velocity"
        }},
        "lines": {"mid": {
            "x0": {"value": 0.0, "unit": "m"},
            "y0": {"value": 0.05, "unit": "m"},
            "x1": {"value": 0.1, "unit": "m"},
            "y1": {"value": 0.05, "unit": "m"},
            "samples": 16,
            "field": "velocity"
        }},
        "surfaces": {"lidflux": {"boundary": "lid", "quantity": "flux"}}
    })
}

fn near(found: f64, wanted: f64) {
    assert!((found - wanted).abs() < 1e-12, "{found} is not {wanted}");
}

#[test]
fn a_study_with_monitors_reads_and_round_trips() {
    let study = with_monitors(listed());
    assert_eq!(study.monitors.probes.len(), 1);
    assert_eq!(study.monitors.lines["mid"].samples, 16);
    assert_eq!(
        study.monitors.surfaces["lidflux"].quantity,
        SurfaceQuantity::Flux
    );
    let written = serde_json::to_string(&study).unwrap();
    assert_eq!(Study::from_json(&written).unwrap(), study);
    let monitors = serde_json::to_value(&study.monitors).unwrap();
    let mut wanted = listed();
    wanted["centreline_u_sample_count"] = json!(64);
    assert_eq!(monitors, wanted);

    let mut broken = listed();
    broken["probes"]["centre"]["colour"] = json!("red");
    let mut value: Value = serde_json::from_str(CAVITY).unwrap();
    value["monitors"]["probes"] = broken["probes"].clone();
    assert!(Study::from_json(&value.to_string()).is_err());
}

#[test]
fn a_study_without_monitors_has_the_same_canonical_hash_and_no_new_keys() {
    let resolved = resolve(&cavity()).unwrap();
    let value = serde_json::to_value(&resolved).unwrap();
    let monitors = value["monitors"].as_object().unwrap();
    for key in ["probes", "lines", "surfaces", "settle"] {
        assert!(!monitors.contains_key(key), "{key}");
    }
    assert_eq!(monitors.len(), 1);
    // The hash of the resolved value as it stood before the new members: the same text.
    let before = blake3::hash(serde_jcs::to_string(&value).unwrap().as_bytes())
        .to_hex()
        .to_string();
    assert_eq!(canonical_hash(&resolved), before);
}

#[test]
fn listing_a_monitor_changes_the_hash_and_a_settle_setting_too() {
    let base = canonical_hash(&resolve(&cavity()).unwrap());
    let with_all = canonical_hash(&resolve(&with_monitors(listed())).unwrap());
    assert_ne!(base, with_all);
    let settled = with_monitors(json!({"settle": {"spread_rel": 0.02}}));
    assert_ne!(canonical_hash(&resolve(&settled).unwrap()), base);
}

#[test]
fn positions_are_resolved_to_metres() {
    let resolved = resolve(&with_monitors(listed())).unwrap();
    let probe = &resolved.monitors.probes[0];
    assert_eq!(probe.label, "centre");
    near(probe.x, 0.05);
    near(probe.y, 0.05);
    let line = &resolved.monitors.lines[0];
    near(line.x1, 0.1);
    assert_eq!(line.samples, 16);
}

#[test]
fn the_plan_of_a_study_with_no_monitor_is_five_default_probes() {
    let plan = resolve(&cavity()).unwrap().monitor_plan();
    assert!(plan.defaulted);
    assert!(plan.lines.is_empty() && plan.surfaces.is_empty());
    let wanted = [
        (0.05, 0.05),
        (0.025, 0.025),
        (0.075, 0.025),
        (0.025, 0.075),
        (0.075, 0.075),
    ];
    assert_eq!(plan.probes.len(), 5);
    for (i, (probe, (x, y))) in plan.probes.iter().zip(wanted).enumerate() {
        assert_eq!(probe.fo, format!("probe_{}", i + 1));
        assert_eq!(probe.label, None);
        assert_eq!(probe.field, MonitorField::Velocity);
        near(probe.x, x);
        near(probe.y, y);
    }
}

#[test]
fn listing_any_monitor_replaces_the_default_set() {
    let plan = resolve(&with_monitors(listed())).unwrap().monitor_plan();
    assert!(!plan.defaulted);
    assert_eq!(
        (plan.probes.len(), plan.lines.len(), plan.surfaces.len()),
        (1, 1, 1)
    );
    assert_eq!(plan.probes[0].fo, "probe_1");
    assert_eq!(plan.lines[0].fo, "line_1");
    assert_eq!(plan.surfaces[0].fo, "surface_1");
    assert_eq!(plan.lines[0].label.as_deref(), Some("mid"));
    // Listing only a line still replaces the defaults: no probe is added behind the author.
    let only_line = json!({"lines": {"mid": listed()["lines"]["mid"].clone()}});
    let plan = resolve(&with_monitors(only_line)).unwrap().monitor_plan();
    assert!(!plan.defaulted && plan.probes.is_empty() && plan.lines.len() == 1);
}

#[test]
fn names_count_from_one_in_label_order_not_label_text() {
    let study = with_monitors(json!({"probes": {"zeta": probe_at(0.09), "alpha": probe_at(0.01)}}));
    let plan = resolve(&study).unwrap().monitor_plan();
    assert_eq!(plan.probes[0].fo, "probe_1");
    assert_eq!(plan.probes[0].label.as_deref(), Some("alpha"));
    near(plan.probes[0].x, 0.01);
    assert_eq!(plan.probes[1].fo, "probe_2");
    assert_eq!(plan.probes[1].label.as_deref(), Some("zeta"));
    let names = serde_json::to_string(&plan.probes.iter().map(|p| &p.fo).collect::<Vec<_>>());
    let names = names.unwrap();
    assert!(
        !names.contains("alpha") && !names.contains("zeta"),
        "{names}"
    );
}

#[test]
fn a_surface_resolves_to_the_kind_of_its_boundary() {
    let plan = resolve(&with_monitors(listed())).unwrap().monitor_plan();
    let surface = &plan.surfaces[0];
    assert_eq!(surface.kind, BoundaryKind::MovingWall);
    assert_eq!(surface.boundary, "lid");
    assert_eq!(surface.quantity, SurfaceQuantity::Flux);
}

#[test]
fn settle_settings_are_carried_and_leave_the_plan_alone() {
    let base = resolve(&cavity()).unwrap().monitor_plan();
    let study = with_monitors(json!({"settle": {"spread_rel": 0.02}}));
    let resolved = resolve(&study).unwrap();
    assert_eq!(resolved.monitor_plan(), base);
    let settle = resolved.monitors.settle.unwrap();
    assert_eq!(settle.spread_rel, Some(0.02));
    assert_eq!(settle.tail_fraction, None);
}

#[test]
fn every_public_monitor_type_is_exported() {
    let probe = Probe {
        x: cavity().domain.length_x,
        y: cavity().domain.length_y,
        field: MonitorField::Pressure,
        boundary: None,
    };
    let line = LineMonitor {
        x0: probe.x,
        y0: probe.y,
        x1: probe.x,
        y1: probe.y,
        samples: 2,
        field: probe.field,
    };
    let surface = SurfaceMonitor {
        boundary: "lid".to_owned(),
        quantity: SurfaceQuantity::PressureIntegral,
    };
    assert_eq!(line.samples, 2);
    assert_eq!(surface.quantity, SurfaceQuantity::PressureIntegral);
    assert_eq!(SettleSettings::default().tail_floor, None);
    let plan = MonitorPlan {
        probes: Vec::<PlannedProbe>::new(),
        lines: Vec::<PlannedLine>::new(),
        surfaces: Vec::<PlannedSurface>::new(),
        forces: Vec::<PlannedForce>::new(),
        defaulted: false,
    };
    assert!(plan.probes.is_empty() && !plan.defaulted);
}

#[test]
fn the_published_study_shape_has_the_new_members_as_optional() {
    let schema = serde_json::to_value(schemars::schema_for!(Study)).unwrap();
    let monitors = &schema["$defs"]["Monitors"];
    for member in ["probes", "lines", "surfaces", "settle"] {
        assert!(monitors["properties"][member].is_object(), "{member}");
    }
    let required: Vec<&str> = monitors["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(required, ["centreline_u_sample_count"]);
}

#[test]
fn a_pre_existing_study_file_still_reads() {
    for text in [CAVITY_V0, CHANNEL] {
        let study = Study::from_json(text).unwrap();
        assert!(study.monitors.probes.is_empty() && study.monitors.settle.is_none());
        assert!(
            !serde_json::to_string(&study)
                .unwrap()
                .contains("\"probes\"")
        );
    }
}
