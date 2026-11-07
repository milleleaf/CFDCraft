use cfdcraft_model::diagnostics::{Diagnostic, is_known_code, setting_label};
use cfdcraft_model::{Capabilities, Study, diagnose, render_text};
use serde_json::{Value, json};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");

fn study(edit: impl FnOnce(&mut Value)) -> Study {
    let mut value: Value = serde_json::from_str(CAVITY).unwrap();
    edit(&mut value["monitors"]);
    Study::from_json(&value.to_string()).unwrap()
}

fn run(study: &Study) -> Vec<Diagnostic> {
    diagnose(study, &Capabilities::unrestricted())
}

fn with_code<'a>(found: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    found.iter().filter(|d| d.code == code).collect()
}

fn q(value: f64) -> Value {
    json!({"value": value, "unit": "m"})
}

fn probe(x: f64, y: f64) -> Value {
    json!({"x": q(x), "y": q(y), "field": "velocity"})
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64, samples: u64) -> Value {
    json!({"x0": q(x0), "y0": q(y0), "x1": q(x1), "y1": q(y1), "samples": samples, "field": "pressure"})
}

#[test]
fn a_bad_monitor_label_is_cfd_mon_001() {
    let long = format!("a{}", "b".repeat(32));
    let hostile = "evil marker$(zz)";
    for label in ["1probe", long.as_str(), hostile] {
        let s = study(|m| m["probes"] = json!({ label: probe(0.05, 0.05) }));
        let found = run(&s);
        let hits = with_code(&found, "CFD-MON-001");
        assert_eq!(hits.len(), 1, "{label}: {found:?}");
        assert_eq!(
            hits[0].pointer,
            format!(
                "/monitors/probes/{}",
                cfdcraft_model::diagnostics::pointer_token(label)
            )
        );
        // The rejected text is named by its setting, never repeated in the words.
        let words = format!("{} {} {:?}", hits[0].message, hits[0].setting, hits[0].hint);
        assert!(
            !words.contains("marker") && !words.contains("zz"),
            "{words}"
        );
        let other = found.iter().map(|d| format!("{} {}", d.message, d.setting));
        for text in other {
            assert!(!text.contains("marker"), "{text}");
        }
    }
    let s = study(|m| m["lines"] = json!({"1line": line(0.0, 0.05, 0.1, 0.05, 8)}));
    assert_eq!(with_code(&run(&s), "CFD-MON-001").len(), 1);
    let s = study(|m| m["surfaces"] = json!({"-s": {"boundary": "lid", "quantity": "flux"}}));
    assert_eq!(with_code(&run(&s), "CFD-MON-001").len(), 1);
}

#[test]
fn a_point_outside_the_domain_is_cfd_mon_002() {
    let s = study(|m| m["probes"] = json!({"p": probe(0.2, 0.05)}));
    let found = run(&s);
    let hits = with_code(&found, "CFD-MON-002");
    assert_eq!(hits.len(), 1, "{found:?}");
    assert_eq!(hits[0].pointer, "/monitors/probes/p/x");
    assert_eq!(hits[0].setting, "probe p x");

    let s = study(|m| m["probes"] = json!({"p": probe(0.05, -0.01)}));
    assert_eq!(
        with_code(&run(&s), "CFD-MON-002")[0].pointer,
        "/monitors/probes/p/y"
    );

    let s = study(|m| m["lines"] = json!({"l": line(0.0, 0.05, 0.2, 0.05, 8)}));
    let found = run(&s);
    let hits = with_code(&found, "CFD-MON-002");
    assert_eq!(hits.len(), 1, "{found:?}");
    assert_eq!(hits[0].pointer, "/monitors/lines/l/x1");

    // The edges of the domain count as inside.
    let s = study(|m| {
        m["probes"] = json!({"a": probe(0.0, 0.0), "b": probe(0.1, 0.1)});
        m["lines"] = json!({"l": line(0.0, 0.0, 0.1, 0.1, 8)});
    });
    assert!(with_code(&run(&s), "CFD-MON-002").is_empty());
}

#[test]
fn a_probe_position_in_the_wrong_unit_is_a_unit_error() {
    let s = study(|m| {
        m["probes"] = json!({"p": {
            "x": {"value": 0.05, "unit": "m/s"},
            "y": q(0.05),
            "field": "velocity"
        }});
    });
    let found = run(&s);
    let units = with_code(&found, "CFD-UNIT-001");
    assert_eq!(units.len(), 1, "{found:?}");
    assert_eq!(units[0].pointer, "/monitors/probes/p/x");
    assert!(with_code(&found, "CFD-MON-002").is_empty());
}

#[test]
fn a_bad_line_is_cfd_mon_003() {
    for samples in [1, 2001] {
        let s = study(|m| m["lines"] = json!({"l": line(0.0, 0.05, 0.1, 0.05, samples)}));
        let found = run(&s);
        let hits = with_code(&found, "CFD-MON-003");
        assert_eq!(hits.len(), 1, "{samples}: {found:?}");
        assert_eq!(hits[0].pointer, "/monitors/lines/l/samples");
    }
    let s = study(|m| m["lines"] = json!({"l": line(0.05, 0.05, 0.05, 0.05, 8)}));
    let found = run(&s);
    let hits = with_code(&found, "CFD-MON-003");
    assert_eq!(hits.len(), 1, "{found:?}");
    assert_eq!(hits[0].pointer, "/monitors/lines/l");
    for samples in [2, 2000] {
        let s = study(|m| m["lines"] = json!({"l": line(0.0, 0.05, 0.1, 0.05, samples)}));
        assert!(with_code(&run(&s), "CFD-MON-003").is_empty(), "{samples}");
    }
}

#[test]
fn a_surface_on_an_unknown_boundary_is_cfd_mon_004() {
    let s = study(|m| m["surfaces"] = json!({"s": {"boundary": "outlet", "quantity": "flux"}}));
    let found = run(&s);
    let hits = with_code(&found, "CFD-MON-004");
    assert_eq!(hits.len(), 1, "{found:?}");
    assert_eq!(hits[0].pointer, "/monitors/surfaces/s/boundary");
    assert!(!hits[0].message.contains("outlet"));
    let s = study(|m| m["surfaces"] = json!({"s": {"boundary": "lid", "quantity": "flux"}}));
    assert!(with_code(&run(&s), "CFD-MON-004").is_empty());
}

#[test]
fn too_many_monitors_is_cfd_mon_005() {
    let build = |probes: usize, lines: usize, surfaces: usize| {
        study(|m| {
            m["probes"] = (0..probes)
                .map(|i| (format!("p{i}"), probe(0.05, 0.05)))
                .collect::<serde_json::Map<_, _>>()
                .into();
            m["lines"] = (0..lines)
                .map(|i| (format!("l{i}"), line(0.0, 0.05, 0.1, 0.05, 8)))
                .collect::<serde_json::Map<_, _>>()
                .into();
            m["surfaces"] = (0..surfaces)
                .map(|i| {
                    (
                        format!("s{i}"),
                        json!({"boundary": "lid", "quantity": "flux"}),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
                .into();
        })
    };
    let pointers = |s: &Study| {
        let found = run(s);
        with_code(&found, "CFD-MON-005")
            .iter()
            .map(|d| d.pointer.clone())
            .collect::<Vec<_>>()
    };
    assert!(pointers(&build(16, 8, 8)).is_empty());
    assert_eq!(pointers(&build(17, 8, 8)), ["/monitors/probes"]);
    assert_eq!(pointers(&build(16, 9, 8)), ["/monitors/lines"]);
    assert_eq!(pointers(&build(16, 8, 9)), ["/monitors/surfaces"]);
    assert_eq!(pointers(&build(17, 9, 9)).len(), 3);
}

#[test]
fn a_settle_setting_outside_its_range_is_cfd_mon_006() {
    let bad = [
        ("tail_fraction", json!(0.0)),
        ("tail_fraction", json!(1.5)),
        ("tail_floor", json!(4)),
        ("tail_floor", json!(100_001)),
        ("spread_rel", json!(-0.01)),
        ("drift_rel", json!(0.0)),
        ("drift_rel", json!(2.0)),
    ];
    for (name, value) in bad {
        let s = study(|m| m["settle"] = json!({ name: value.clone() }));
        let found = run(&s);
        let hits = with_code(&found, "CFD-MON-006");
        assert_eq!(hits.len(), 1, "{name} {value}: {found:?}");
        assert_eq!(hits[0].pointer, format!("/monitors/settle/{name}"));
    }
    let good = json!({"tail_fraction": 1.0, "tail_floor": 5, "spread_rel": 0.5, "drift_rel": 1.0});
    let s = study(|m| m["settle"] = good);
    assert!(with_code(&run(&s), "CFD-MON-006").is_empty());
}

#[test]
fn a_study_with_good_monitors_has_no_monitor_finding() {
    let s = study(|m| {
        m["probes"] = json!({"centre": probe(0.05, 0.05)});
        m["lines"] = json!({"mid": line(0.0, 0.05, 0.1, 0.05, 32)});
        m["surfaces"] = json!({"lidflux": {"boundary": "lid", "quantity": "pressure_average"}});
        m["settle"] = json!({"tail_fraction": 0.3});
    });
    let found = run(&s);
    let monitor: Vec<_> = found
        .iter()
        .filter(|d| d.code.starts_with("CFD-MON"))
        .collect();
    assert!(monitor.is_empty(), "{}", render_text(&found));
}

#[test]
fn the_monitor_codes_are_listed() {
    for code in [
        "CFD-MON-001",
        "CFD-MON-002",
        "CFD-MON-003",
        "CFD-MON-004",
        "CFD-MON-005",
        "CFD-MON-006",
        "CFD-RUN-003",
        "CFD-RUN-004",
        "CFD-RUN-005",
        "CFD-RUN-006",
        "CFD-RUN-007",
        "CFD-RUN-008",
        "CFD-INFRA-007",
        "CFD-INFRA-008",
        "CFD-INFRA-009",
        // Plan 07-01 added the force, boundary probe and heat-flux monitor codes.
        "CFD-MON-007",
        "CFD-MON-008",
        "CFD-MON-009",
    ] {
        assert!(is_known_code(code), "{code}");
    }
    assert!(!is_known_code("CFD-MON-010") && !is_known_code("CFD-RUN-009"));
}

#[test]
fn setting_labels_read_well() {
    for (pointer, words) in [
        ("/monitors/probes/centre/x", "probe centre x"),
        ("/monitors/lines/mid/x0", "line mid x0"),
        ("/monitors/lines/mid/samples", "line mid samples"),
        (
            "/monitors/surfaces/lidflux/boundary",
            "surface lidflux boundary",
        ),
        ("/monitors/settle/tail_fraction", "settle tail fraction"),
        ("/monitors/probes/bad name/x", "probe (invalid name) x"),
    ] {
        assert_eq!(setting_label(pointer), words, "{pointer}");
    }
}
