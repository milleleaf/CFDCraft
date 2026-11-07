use cfdcraft_model::diagnostics::{ALL_KINDS, CODES, Diagnostic, is_known_code};
use cfdcraft_model::{
    BoundaryCondition, BoundaryKind, Capabilities, Face, ResolveError, Severity, Study, StudyError,
    Unit, diagnose, has_errors, render_text, resolve, units,
};

const CAVITY: &str = include_str!("fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("fixtures/channel_v1.json");

fn study(json: &str) -> Study {
    Study::from_json(json).unwrap()
}

fn codes(study: &Study, capabilities: &Capabilities) -> Vec<String> {
    diagnose(study, capabilities)
        .into_iter()
        .map(|d| d.code)
        .collect()
}

#[test]
fn a_channel_without_an_outlet_names_the_uncovered_side() {
    let mut channel = study(CHANNEL);
    channel.boundary_conditions.remove("outlet");
    let found = diagnose(&channel, &Capabilities::unrestricted());
    let missing: Vec<_> = found.iter().filter(|d| d.code == "CFD-BC-001").collect();
    assert_eq!(missing.len(), 1, "{found:?}");
    let d = missing[0];
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.pointer, "/boundary_conditions");
    assert_eq!(d.setting, "the right side of the domain (XMax)");
    assert!(d.message.contains("exactly one"), "{}", d.message);
    assert!(d.hint.as_deref().unwrap().contains("XMax"));
    assert!(
        found
            .iter()
            .all(|d| !d.code.starts_with("CFD-BC-00") || d.code == "CFD-BC-001")
    );
    let text = render_text(&found);
    assert!(
        text.contains("error CFD-BC-001:")
            && text.contains("[the right side of the domain (XMax); /boundary_conditions]")
    );
    assert!(text.contains("\n  hint: "));
}

#[test]
fn the_shipped_studies_have_no_errors() {
    let esi = Capabilities::unrestricted();
    for json in [CAVITY, CHANNEL] {
        assert!(!has_errors(&diagnose(&study(json), &esi)));
    }
}

#[test]
fn a_side_claimed_twice_names_both_boundaries() {
    let mut cavity = study(CAVITY);
    cavity
        .boundary_conditions
        .get_mut("lid")
        .unwrap()
        .faces
        .push(Face::XMin);
    let found = diagnose(&cavity, &Capabilities::unrestricted());
    let both: Vec<_> = found.iter().filter(|d| d.code == "CFD-BC-002").collect();
    assert_eq!(both.len(), 1, "{found:?}");
    assert!(both[0].message.contains("\"lid\"") && both[0].message.contains("\"walls\""));
    assert_eq!(both[0].pointer, "/boundary_conditions/lid/faces");
    // The same face listed twice by one boundary is not a conflict.
    let mut cavity = study(CAVITY);
    cavity
        .boundary_conditions
        .get_mut("walls")
        .unwrap()
        .faces
        .push(Face::XMin);
    assert!(codes(&cavity, &Capabilities::unrestricted()).is_empty());
}

fn errors(study: &Study) -> Vec<(String, String)> {
    diagnose(study, &Capabilities::unrestricted())
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| (d.code, d.pointer))
        .collect()
}

fn pair(code: &str, pointer: &str) -> (String, String) {
    (code.to_owned(), pointer.to_owned())
}

#[test]
fn five_independent_mistakes_are_reported_together_in_pointer_order() {
    let mut channel = study(CHANNEL);
    channel.domain.length_x.unit = Unit::Second;
    channel.fluid.kinematic_viscosity.value = 0.0;
    channel.mesh.cells_y = 0;
    channel.solver.write_interval.value = 7.0;
    channel
        .boundary_conditions
        .get_mut("inlet")
        .unwrap()
        .velocity = None;
    assert_eq!(
        errors(&channel),
        [
            pair("CFD-BC-004", "/boundary_conditions/inlet/velocity"),
            pair("CFD-UNIT-001", "/domain/length_x"),
            pair("CFD-VAL-001", "/fluid/kinematic_viscosity"),
            pair("CFD-VAL-003", "/mesh/cells_y"),
            pair("CFD-VAL-004", "/solver/write_interval"),
        ]
    );
}

fn lid(study: &mut Study) -> &mut cfdcraft_model::Quantity {
    study
        .boundary_conditions
        .get_mut("lid")
        .unwrap()
        .velocity
        .as_mut()
        .unwrap()
}

#[test]
fn every_other_code_the_model_emits_has_a_triggering_study() {
    let base = || study(CAVITY);
    let mut cases: Vec<(Study, (String, String))> = Vec::new();
    let mut s = base();
    s.schema_version = 2;
    cases.push((s, pair("CFD-STUDY-002", "/schema_version")));
    let mut s = base();
    s.domain.length_y.value = f64::NAN;
    cases.push((s, pair("CFD-UNIT-002", "/domain/length_y")));
    let mut s = base();
    lid(&mut s).value = 0.0;
    cases.push((s, pair("CFD-VAL-002", "/boundary_conditions/lid/velocity")));
    let mut s = base();
    s.monitors.centreline_u_sample_count = 1;
    cases.push((
        s,
        pair("CFD-VAL-005", "/monitors/centreline_u_sample_count"),
    ));
    let mut s = base();
    s.solver.convergence.p = 0.0;
    cases.push((s, pair("CFD-VAL-006", "/solver/convergence/p")));
    let mut s = base();
    s.solver.convergence.u = f64::INFINITY;
    cases.push((s, pair("CFD-VAL-006", "/solver/convergence/U")));
    let mut s = base();
    s.boundary_conditions
        .get_mut("walls")
        .unwrap()
        .faces
        .clear();
    cases.push((s, pair("CFD-BC-003", "/boundary_conditions/walls/faces")));
    let mut s = base();
    s.boundary_conditions.get_mut("walls").unwrap().velocity =
        s.boundary_conditions["lid"].velocity;
    cases.push((s, pair("CFD-BC-005", "/boundary_conditions/walls/velocity")));
    let mut s = base();
    let walls = s.boundary_conditions.remove("walls").unwrap();
    s.boundary_conditions.insert("a/b".to_owned(), walls);
    cases.push((s, pair("CFD-BC-006", "/boundary_conditions/a~1b")));
    for (case, expected) in cases {
        let found = errors(&case);
        assert!(found.contains(&expected), "{expected:?} not in {found:?}");
    }
    // Malformed text and an unsupported version arrive as study errors.
    let error = Study::from_json("{ \"schema_version\": ").unwrap_err();
    let d = Diagnostic::from_study_error(&error);
    assert_eq!(
        (d.code.as_str(), d.severity),
        ("CFD-STUDY-001", Severity::Error)
    );
    assert!(
        d.message.contains("line 1") && d.message.contains("column"),
        "{}",
        d.message
    );
    let error = Study::from_json(&CAVITY.replace("\"schema_version\": 1", "\"schema_version\": 9"))
        .unwrap_err();
    assert!(matches!(
        error,
        StudyError::UnsupportedSchemaVersion { found: 9 }
    ));
    let d = Diagnostic::from_study_error(&error);
    assert_eq!(
        (d.code.as_str(), d.pointer.as_str()),
        ("CFD-STUDY-002", "/schema_version")
    );
    assert!(d.message.contains('9'));
}

#[test]
fn an_authored_outlet_pressure_names_the_unknown_field() {
    let mut value: serde_json::Value = serde_json::from_str(CHANNEL).unwrap();
    value["boundary_conditions"]["outlet"]["pressure"] =
        serde_json::json!({"value": 5, "unit": "Pa"});
    let error = Study::from_json(&value.to_string()).unwrap_err();
    let d = Diagnostic::from_study_error(&error);
    assert_eq!(d.code, "CFD-STUDY-001");
    assert!(
        d.message.contains("pressure") && d.message.contains("unknown field"),
        "{}",
        d.message
    );
}

#[test]
fn the_catalogue_is_unique_well_formed_and_covers_what_the_model_emits() {
    assert_eq!(CODES.len(), 71);
    let mut seen = std::collections::BTreeSet::new();
    for (code, meaning) in CODES {
        let parts: Vec<_> = code.split('-').collect();
        assert!(
            parts.len() == 3
                && parts[0] == "CFD"
                && parts[1].chars().all(|c| c.is_ascii_uppercase())
                && parts[2].len() == 3
                && parts[2].chars().all(|c| c.is_ascii_digit()),
            "{code}"
        );
        assert!(!meaning.is_empty() && seen.insert(*code), "{code}");
        assert!(is_known_code(code));
    }
    assert!(!is_known_code("CFD-BC-999") && !is_known_code("BC-001"));
    // Every code literal outside the catalogue table must be one the tests above trigger.
    let source = include_str!("../src/diagnostics.rs");
    let emitters = source.split("// END OF CATALOGUE").nth(1).expect("marker");
    let triggered = [
        "CFD-STUDY-001",
        "CFD-STUDY-002",
        "CFD-UNIT-001",
        "CFD-UNIT-002",
        "CFD-VAL-001",
        "CFD-VAL-002",
        "CFD-VAL-003",
        "CFD-VAL-004",
        "CFD-VAL-005",
        "CFD-VAL-006",
        "CFD-BC-001",
        "CFD-BC-002",
        "CFD-BC-003",
        "CFD-BC-004",
        "CFD-BC-005",
        "CFD-BC-006",
        "CFD-DOM-001",
        "CFD-DOM-002",
        "CFD-MODEL-001",
        "CFD-MODEL-002",
        "CFD-MESH-001",
        "CFD-WARN-001",
        "CFD-WARN-002",
        "CFD-RAW-001",
        "CFD-RAW-002",
        "CFD-RAW-003",
        "CFD-RAW-004",
        "CFD-RAW-005",
        "CFD-RAW-006",
        "CFD-MON-001",
        "CFD-MON-002",
        "CFD-MON-003",
        "CFD-MON-004",
        "CFD-MON-005",
        "CFD-MON-006",
    ];
    for literal in emitters.split("\"CFD-").skip(1) {
        let code = format!("CFD-{}", &literal[..literal.find('"').unwrap()]);
        assert!(is_known_code(&code), "{code}");
        assert!(
            triggered.contains(&code.as_str()),
            "{code} has no triggering test"
        );
    }
}

#[test]
fn resolve_is_built_on_diagnose_and_diagnose_does_not_convert() {
    let mut broken = study(CAVITY);
    broken.fluid.kinematic_viscosity.value = -1.0;
    units::reset_conversion_count();
    let found = diagnose(&broken, &Capabilities::unrestricted());
    assert_eq!(units::conversion_count(), 0);
    match resolve(&broken) {
        Err(ResolveError::Diagnostics(d)) => {
            assert_eq!(d, found);
            assert!(
                ResolveError::Diagnostics(d)
                    .to_string()
                    .contains("CFD-VAL-001")
            );
        }
        other => panic!("{other:?}"),
    }
    units::reset_conversion_count();
    resolve(&study(CAVITY)).unwrap();
    assert_eq!(units::conversion_count(), 7);
}

/// What the ESI writer can run, spelled out because the model crate cannot see the backend.
fn writer_limits() -> Capabilities {
    Capabilities {
        boundary_kinds: ALL_KINDS.to_vec(),
        requires_single_cell_z: true,
        same_kind_boundaries_must_agree: true,
        raw_backend: None,
    }
}

fn find<'a>(found: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
    found.iter().filter(|d| d.code == code).collect()
}

#[test]
fn an_open_domain_with_no_outlet_has_nowhere_for_the_flow_to_go() {
    let mut channel = study(CHANNEL);
    channel.boundary_conditions.get_mut("outlet").unwrap().kind = BoundaryKind::NoSlipWall;
    let found = diagnose(&channel, &writer_limits());
    let d = &find(&found, "CFD-DOM-001")[0];
    assert_eq!(
        (d.pointer.as_str(), d.setting.as_str()),
        ("/domain/open", "open-domain flag")
    );
    assert!(d.message.contains("nowhere to leave"), "{}", d.message);
}

#[test]
fn a_closed_domain_with_an_outlet_names_the_offending_boundary() {
    let mut cavity = study(CAVITY);
    cavity.boundary_conditions.get_mut("walls").unwrap().faces = vec![Face::XMin, Face::YMin];
    cavity.boundary_conditions.insert(
        "out".to_owned(),
        BoundaryCondition {
            faces: vec![Face::XMax],
            kind: BoundaryKind::PressureOutlet,
            velocity: None,
        },
    );
    let found = diagnose(&cavity, &writer_limits());
    let d = &find(&found, "CFD-DOM-002")[0];
    assert_eq!(d.pointer, "/boundary_conditions/out/kind");
    for json in [CAVITY, CHANNEL] {
        let found = diagnose(&study(json), &writer_limits());
        assert!(find(&found, "CFD-DOM-001").is_empty() && find(&found, "CFD-DOM-002").is_empty());
    }
}

#[test]
fn a_model_the_backend_cannot_run_is_refused_by_capability() {
    let mut no_symmetry = Capabilities::unrestricted();
    no_symmetry
        .boundary_kinds
        .retain(|k| *k != BoundaryKind::Symmetry);
    let found = diagnose(&study(CHANNEL), &no_symmetry);
    assert_eq!(
        find(&found, "CFD-MODEL-001")[0].pointer,
        "/boundary_conditions/centreline/kind"
    );
    assert!(diagnose(&study(CHANNEL), &Capabilities::unrestricted()).is_empty());

    let mut thick = study(CAVITY);
    thick.mesh.cells_z = 8;
    let found = diagnose(&thick, &writer_limits());
    assert_eq!(find(&found, "CFD-MESH-001")[0].pointer, "/mesh/cells_z");
    assert!(
        find(
            &diagnose(&thick, &Capabilities::unrestricted()),
            "CFD-MESH-001"
        )
        .is_empty()
    );

    let mut two = study(CAVITY);
    two.boundary_conditions.get_mut("walls").unwrap().faces = vec![Face::XMin, Face::XMax];
    let mut bottom = two.boundary_conditions["lid"].clone();
    bottom.faces = vec![Face::YMin];
    bottom.velocity.as_mut().unwrap().value = 0.2;
    two.boundary_conditions.insert("bottom".to_owned(), bottom);
    let found = diagnose(&two, &writer_limits());
    let d = &find(&found, "CFD-MODEL-002")[0];
    assert!(
        d.message.contains("\"bottom\"") && d.message.contains("\"lid\""),
        "{}",
        d.message
    );
    assert!(d.pointer.ends_with("/velocity"));
    assert!(
        find(
            &diagnose(&two, &Capabilities::unrestricted()),
            "CFD-MODEL-002"
        )
        .is_empty()
    );
}

#[test]
fn warnings_are_reported_but_never_block() {
    let mut fast = study(CAVITY);
    fast.fluid.kinematic_viscosity.value = 1e-6;
    let found = diagnose(&fast, &writer_limits());
    assert_eq!(
        find(&found, "CFD-WARN-001")[0].pointer,
        "/fluid/kinematic_viscosity"
    );
    assert!(!has_errors(&found));
    assert!(find(&found, "CFD-WARN-001")[0].severity == Severity::Warning);
    assert!(resolve(&fast).is_ok());

    let mut coarse = study(CAVITY);
    coarse.mesh.cells_x = 4;
    coarse.mesh.cells_y = 7;
    let found = diagnose(&coarse, &writer_limits());
    let pointers: Vec<_> = find(&found, "CFD-WARN-002")
        .iter()
        .map(|d| d.pointer.clone())
        .collect();
    assert_eq!(pointers, ["/mesh/cells_x", "/mesh/cells_y"]);
    assert!(!has_errors(&found));
    for json in [CAVITY, CHANNEL] {
        assert!(diagnose(&study(json), &writer_limits()).is_empty());
    }
}
