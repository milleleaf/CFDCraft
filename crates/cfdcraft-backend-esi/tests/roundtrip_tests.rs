use cfdcraft_backend_esi::{
    Dialect, compile,
    reader::{EsiReader, source_files},
};
use cfdcraft_model::{
    ResolvedStudy, Study,
    diagnostics::{Diagnostic, render_text},
    resolve,
    roundtrip::{
        CaseReader, Expected, FactValue, SourceFile, Span, check_roundtrip, expected_facts,
    },
};

const CAVITY_V0: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v0.json");
const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

fn resolved(json: &str) -> ResolvedStudy {
    resolve(&Study::from_json(json).unwrap()).unwrap()
}

fn case(json: &str) -> (ResolvedStudy, Vec<SourceFile>) {
    let study = resolved(json);
    let tree = compile(&study, Dialect::EsiV2512).unwrap();
    (study, source_files(&tree))
}

fn text<'a>(files: &'a [SourceFile], path: &str) -> &'a str {
    std::str::from_utf8(&files.iter().find(|f| f.path == path).unwrap().bytes).unwrap()
}

/// A copy of `files` with `path` rewritten by `change`.
fn mutated(files: &[SourceFile], path: &str, change: impl Fn(&str) -> String) -> Vec<SourceFile> {
    files
        .iter()
        .map(|f| {
            if f.path == path {
                SourceFile {
                    path: f.path.clone(),
                    bytes: change(text(files, path)).into_bytes(),
                }
            } else {
                f.clone()
            }
        })
        .collect()
}

fn check(files: &[SourceFile], study: &ResolvedStudy) -> Vec<Diagnostic> {
    check_roundtrip(&EsiReader, files, study)
}

const TRANSPORT: &str = "constant/transportProperties";

#[test]
fn the_cavity_viscosity_is_read_back_and_matches() {
    let (study, files) = case(CAVITY_V0);
    let facts = EsiReader.read(&files).unwrap();
    let nu = &facts["/fluid/kinematic_viscosity"];
    assert_eq!(nu.value, FactValue::Number(0.0001));
    assert_eq!(nu.value_spans.len(), 1);
    assert_eq!(check(&files, &study), vec![]);
}

#[test]
fn viscosity_written_as_zero_fails_and_names_the_setting_the_file_and_the_line() {
    let (study, files) = case(CAVITY_V1);
    let bad = mutated(&files, TRANSPORT, |t| t.replace("0.0001;", "0;"));
    let found = check(&bad, &study);
    assert_eq!(found.len(), 1, "{found:?}");
    let d = &found[0];
    assert_eq!(d.code, "CFD-RT-001");
    assert_eq!(d.pointer, "/fluid/kinematic_viscosity");
    assert_eq!(d.setting, "kinematic viscosity");
    assert!(
        d.message.contains("written as 0 in the case"),
        "{}",
        d.message
    );
    assert!(d.message.contains("the study has 0.0001"), "{}", d.message);
    let line = text(&bad, TRANSPORT)
        .lines()
        .position(|l| l.starts_with("nu "))
        .unwrap() as u64
        + 1;
    let at = d.location.as_ref().unwrap();
    assert_eq!((at.file.as_str(), at.line), (TRANSPORT, line));
    assert_eq!(at.entry.as_deref(), Some("nu [0 2 -1 0 0 0 0] 0;"));
    let shown = render_text(&found);
    assert!(
        shown.contains(&format!(
            "error CFD-RT-001: The kinematic viscosity is written as 0 in the case, but the study has 0.0001. [kinematic viscosity; /fluid/kinematic_viscosity; {TRANSPORT}:{line} (nu [0 2 -1 0 0 0 0] 0;)]"
        )),
        "{shown}"
    );
}

#[test]
fn a_missing_viscosity_line_says_it_was_not_found() {
    let (study, files) = case(CAVITY_V1);
    let bad = mutated(&files, TRANSPORT, |t| {
        t.lines()
            .filter(|l| !l.starts_with("nu "))
            .map(|l| format!("{l}\n"))
            .collect()
    });
    let found = check(&bad, &study);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, "CFD-RT-001");
    assert_eq!(found[0].pointer, "/fluid/kinematic_viscosity");
    assert!(
        found[0]
            .message
            .contains("was not found in the written case")
    );
}

#[test]
fn a_file_that_cannot_be_read_back_names_the_file() {
    let (study, files) = case(CAVITY_V1);
    for garbage in [
        "nu [0 2 -1 0 0 0 0 0.0001;\n",
        "nu [0 2 -1 0 0 0 0] 0.0001\n",
        "/* never closed\nnu [0 2 -1 0 0 0 0] 0.0001;\n",
        "nu [0 2 -1 0 0 0 0] \"0.0001;\n",
        "nu [0 2 -1 0 0 0 0] 0.0001; }\n",
        "nu 0.0001;\n",
        "nu [0 2 -1 0 0 0 0] inf;\n",
    ] {
        let bad = mutated(&files, TRANSPORT, |_| garbage.to_owned());
        let found = check(&bad, &study);
        assert_eq!(found.len(), 1, "{garbage:?}: {found:?}");
        assert_eq!(found[0].code, "CFD-RT-002", "{garbage:?}");
        assert_eq!(found[0].location.as_ref().unwrap().file, TRANSPORT);
        assert!(found[0].message.contains(TRANSPORT), "{}", found[0].message);
    }
    let mut binary = files.clone();
    binary
        .iter_mut()
        .find(|f| f.path == TRANSPORT)
        .unwrap()
        .bytes = vec![0xff, 0xfe, 0x00];
    assert_eq!(check(&binary, &study)[0].code, "CFD-RT-002");
    let removed: Vec<_> = files.into_iter().filter(|f| f.path != TRANSPORT).collect();
    let found = check(&removed, &study);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].code, "CFD-RT-002");
    assert!(found[0].message.contains(TRANSPORT));
}

#[test]
fn comparison_is_exact() {
    let (study, files) = case(CAVITY_V1);
    let off_by_one_ulp = mutated(&files, TRANSPORT, |t| {
        t.replace("0.0001;", "0.00010000000000000002;")
    });
    let found = check(&off_by_one_ulp, &study);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("0.00010000000000000002"));
    // The same number in another spelling reads back to the same bits and passes.
    let respelled = mutated(&files, TRANSPORT, |t| t.replace("0.0001;", "1e-4;"));
    assert_eq!(check(&respelled, &study), vec![]);
}

#[test]
fn the_channel_viscosity_round_trips() {
    let (study, files) = case(CHANNEL_V1);
    assert_eq!(check(&files, &study), vec![]);
}

const BLOCK_MESH: &str = "system/blockMeshDict";
const VELOCITY: &str = "0/U";
const PRESSURE: &str = "0/p";
const CONTROL: &str = "system/controlDict";
const SOLUTION: &str = "system/fvSolution";
const TURBULENCE: &str = "constant/turbulenceProperties";

fn cases() -> Vec<(&'static str, &'static str)> {
    vec![
        ("cavity from the version 0 study", CAVITY_V0),
        ("cavity from the version 1 study", CAVITY_V1),
        ("channel", CHANNEL_V1),
    ]
}

fn codes(found: &[Diagnostic]) -> Vec<(&str, &str)> {
    found
        .iter()
        .map(|d| (d.code.as_str(), d.pointer.as_str()))
        .collect()
}

#[test]
fn both_golden_cases_round_trip_clean_and_every_expected_fact_is_recovered() {
    for (name, json) in cases() {
        let (study, files) = case(json);
        assert_eq!(check(&files, &study), vec![], "{name}");
        let facts = EsiReader.read(&files).unwrap();
        let wanted: Vec<String> = expected_facts(&study).into_iter().map(|e| e.key).collect();
        let got: Vec<String> = facts.keys().cloned().collect();
        assert_eq!(got, wanted, "{name}");
        for key in [
            "/domain/length_x",
            "/domain/length_y",
            "/domain/length_z",
            "/domain/origin_x",
            "/domain/origin_y",
            "/domain/origin_z",
            "/domain/open",
            "/mesh/cells_x",
            "/mesh/cells_y",
            "/mesh/cells_z",
            "/mesh/grading",
            "/fluid/kinematic_viscosity",
            "/solver/end_time",
            "/solver/write_interval",
            "/solver/application",
            "/solver/turbulence",
            "/monitors/centreline_u_sample_count/vertical",
            "/monitors/centreline_u_sample_count/horizontal",
        ] {
            assert!(facts.contains_key(key), "{name}: {key}");
        }
        for face in ["XMin", "XMax", "YMin", "YMax", "ZMin", "ZMax"] {
            assert!(
                facts.contains_key(&format!("/boundary_conditions/by_face/{face}/kind")),
                "{name}: {face}"
            );
        }
    }
}

#[test]
fn recovered_values_are_the_resolved_values() {
    let (_, files) = case(CAVITY_V1);
    let facts = EsiReader.read(&files).unwrap();
    let value = |key: &str| facts[key].value.clone();
    assert_eq!(value("/domain/length_z"), FactValue::Number(0.05));
    assert_eq!(value("/domain/open"), FactValue::Flag(false));
    assert_eq!(value("/mesh/cells_x"), FactValue::Count(128));
    assert_eq!(value("/mesh/grading"), FactValue::Text("1 1 1".into()));
    assert_eq!(value("/solver/end_time"), FactValue::Number(5000.0));
    assert_eq!(value("/solver/write_interval"), FactValue::Number(500.0));
    assert_eq!(
        value("/monitors/centreline_u_sample_count/horizontal"),
        FactValue::Count(64)
    );
    let kind = |face: &str| value(&format!("/boundary_conditions/by_face/{face}/kind"));
    assert_eq!(kind("YMax"), FactValue::Text("moving wall".into()));
    assert_eq!(kind("XMin"), FactValue::Text("no-slip wall".into()));
    assert_eq!(kind("ZMax"), FactValue::Text("empty".into()));
    assert_eq!(
        value("/boundary_conditions/by_face/YMax/velocity"),
        FactValue::Number(0.1)
    );
    let (_, files) = case(CHANNEL_V1);
    let facts = EsiReader.read(&files).unwrap();
    assert_eq!(facts["/domain/open"].value, FactValue::Flag(true));
    // An inlet on the left side carries its speed along +x; the outlet carries no velocity.
    assert_eq!(
        facts["/boundary_conditions/by_face/XMin/velocity"].value,
        FactValue::Number(0.1)
    );
    assert!(!facts.contains_key("/boundary_conditions/by_face/XMax/velocity"));
    assert_eq!(
        facts["/boundary_conditions/by_face/XMax/kind"].value,
        FactValue::Text("pressure outlet".into())
    );
    assert_eq!(
        facts["/boundary_conditions/by_face/YMax/kind"].value,
        FactValue::Text("symmetry plane".into())
    );
}

#[test]
fn faces_are_identified_from_vertex_coordinates_not_from_patch_names() {
    let (study, files) = case(CHANNEL_V1);
    // Trade the vertex tuples of the inlet and the outlet; every name and type stays put.
    let bad = mutated(&files, BLOCK_MESH, |t| {
        t.replace("(0 4 7 3)", "(TMP)")
            .replace("(1 2 6 5)", "(0 4 7 3)")
            .replace("(TMP)", "(1 2 6 5)")
    });
    let found = check(&bad, &study);
    let pointers: Vec<&str> = found.iter().map(|d| d.pointer.as_str()).collect();
    for want in [
        "/boundary_conditions/inlet/kind",
        "/boundary_conditions/outlet/kind",
        "/boundary_conditions/inlet/velocity",
    ] {
        assert!(pointers.contains(&want), "{want}: {found:?}");
    }
    let inlet = found
        .iter()
        .find(|d| d.pointer == "/boundary_conditions/inlet/kind")
        .unwrap();
    assert!(
        inlet
            .message
            .contains("on the left side of the domain (XMin)")
    );
    assert!(inlet.message.contains("written as pressure outlet"));
    assert!(inlet.message.contains("the study has velocity inlet"));
    // The inlet's speed now sits on the right side, which the study does not account for.
    assert!(
        found
            .iter()
            .any(|d| d.message.contains("does not account for"))
    );
}

#[test]
fn a_face_claimed_twice_or_off_the_box_cannot_be_read_back() {
    let (study, files) = case(CHANNEL_V1);
    for (from, to, words) in [
        ("(0 4 7 3)", "(1 2 6 5)", "more than one patch"),
        ("(0 4 7 3)", "(0 4 7 2)", "one plane of the box"),
        ("(0 4 7 3)", "(0 0 4 4)", "four different vertices"),
        ("(0 4 7 3)", "(0 4 7 8)", "vertex"),
    ] {
        let bad = mutated(&files, BLOCK_MESH, |t| t.replacen(from, to, 1));
        let found = check(&bad, &study);
        assert_eq!(found.len(), 1, "{to}: {found:?}");
        assert_eq!(found[0].code, "CFD-RT-002", "{to}");
        assert!(
            found[0].message.contains(words),
            "{to}: {}",
            found[0].message
        );
    }
}

#[test]
fn every_file_the_reader_needs_is_required() {
    let (study, files) = case(CAVITY_V1);
    for path in [
        BLOCK_MESH, VELOCITY, PRESSURE, TRANSPORT, TURBULENCE, CONTROL, SOLUTION,
    ] {
        let fewer: Vec<_> = files.iter().filter(|f| f.path != path).cloned().collect();
        let found = check(&fewer, &study);
        assert_eq!(found.len(), 1, "{path}: {found:?}");
        assert_eq!(found[0].code, "CFD-RT-002", "{path}");
        assert!(
            found[0].message.contains(path),
            "{path}: {}",
            found[0].message
        );
    }
}

#[test]
fn a_fact_the_study_does_not_account_for_fails() {
    let (mut study, files) = case(CAVITY_V1);
    // The case still writes the three wall sides; the study no longer has them.
    study.boundary_conditions.remove("walls");
    let found = check(&files, &study);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found
            .iter()
            .all(|d| d.code == "CFD-RT-001" && d.message.contains("does not account for")),
        "{found:?}"
    );
}

#[test]
fn a_velocity_with_an_off_axis_component_names_the_written_vector() {
    let (study, files) = case(CHANNEL_V1);
    let bad = mutated(&files, VELOCITY, |t| t.replace("(0.1 0 0)", "(0.1 0.2 0)"));
    let found = check(&bad, &study);
    assert_eq!(
        codes(&found),
        vec![("CFD-RT-001", "/boundary_conditions/inlet/velocity")],
        "{found:?}"
    );
    assert!(
        found[0].message.contains("(0.1 0.2 0)"),
        "{}",
        found[0].message
    );
    assert_eq!(found[0].setting, "inlet velocity");
}

#[test]
fn negative_zero_is_zero_and_nothing_else_is_forgiven() {
    let (study, files) = case(CAVITY_V1);
    let negative = mutated(&files, BLOCK_MESH, |t| t.replacen("(0 0 0)", "(-0 0 0)", 1));
    assert_eq!(check(&negative, &study), vec![]);
    let shifted = mutated(&files, BLOCK_MESH, |t| {
        t.replacen("(0 0 0)", "(0.0000001 0 0)", 1)
    });
    let found = check(&shifted, &study);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, "CFD-RT-002");
}

#[test]
fn the_open_flag_follows_the_pressure_reference_in_the_solver_settings() {
    let (study, files) = case(CAVITY_V1);
    let bad = mutated(&files, SOLUTION, |t| t.replace("    pRefCell 0;\n", ""));
    let found = check(&bad, &study);
    assert_eq!(
        codes(&found),
        vec![("CFD-RT-001", "/domain/open")],
        "{found:?}"
    );
}

#[test]
fn an_outlet_pressure_other_than_zero_is_not_a_pressure_outlet() {
    let (study, files) = case(CHANNEL_V1);
    let bad = mutated(&files, PRESSURE, |t| {
        t.replace("value uniform 0;", "value uniform 5;")
    });
    let found = check(&bad, &study);
    assert_eq!(
        codes(&found),
        vec![("CFD-RT-001", "/boundary_conditions/outlet/kind")],
        "{found:?}"
    );
    assert!(
        found[0]
            .message
            .contains("pressure value other than uniform 0"),
        "{}",
        found[0].message
    );
}

// ---- mutation harness ---------------------------------------------------------------------
//
// Every mutation is built from the spans the reader itself recorded and judged by the checker,
// so no writer function decides whether a mutation was caught.

/// A copy of `files` with the bytes of `span` replaced by `with`.
fn splice(files: &[SourceFile], span: &Span, with: &str) -> Vec<SourceFile> {
    files
        .iter()
        .map(|f| {
            let mut bytes = f.bytes.clone();
            if f.path == span.file {
                bytes.splice(span.start..span.end, with.bytes());
            }
            SourceFile {
                path: f.path.clone(),
                bytes,
            }
        })
        .collect()
}

fn token<'a>(files: &'a [SourceFile], span: &Span) -> &'a str {
    let file = files.iter().find(|f| f.path == span.file).unwrap();
    std::str::from_utf8(&file.bytes[span.start..span.end]).unwrap()
}

/// The check failed, and an error names the fact's setting or pointer, or the mutated file.
fn caught(found: &[Diagnostic], want: &Expected, file: &str) -> bool {
    !found.is_empty()
        && found
            .iter()
            .all(|d| matches!(d.code.as_str(), "CFD-RT-001" | "CFD-RT-002"))
        && found.iter().any(|d| {
            want.source_pointers.contains(&d.pointer)
                || d.setting == want.setting
                || d.location.as_ref().is_some_and(|l| l.file == file)
                || d.message.contains(file)
        })
}

/// Zero every value token and drop every entry the reader recovered. Returns the counts and the
/// mutations that were not caught.
fn run_harness(json: &str) -> (usize, usize, Vec<String>) {
    let (study, files) = case(json);
    assert_eq!(check(&files, &study), vec![], "negative control");
    let facts = EsiReader.read(&files).unwrap();
    let wanted = expected_facts(&study);
    let (mut zeroed, mut dropped) = (0, 0);
    let mut survivors = Vec::new();
    for (key, fact) in &facts {
        let want = wanted.iter().find(|w| &w.key == key).unwrap();
        for span in &fact.value_spans {
            // A token that already reads as zero cannot be zeroed.
            if token(&files, span).parse::<f64>().is_ok_and(|v| v == 0.0) {
                continue;
            }
            zeroed += 1;
            let found = check(&splice(&files, span, "0"), &study);
            if !caught(&found, want, &span.file) {
                survivors.push(format!(
                    "zero {key} at {}:{}: {found:?}",
                    span.file, span.line
                ));
            }
        }
        if let Some(entry) = &fact.entry_span {
            dropped += 1;
            let found = check(&splice(&files, entry, ""), &study);
            if !caught(&found, want, &entry.file) {
                survivors.push(format!(
                    "drop {key} at {}:{}: {found:?}",
                    entry.file, entry.line
                ));
            }
        }
    }
    (zeroed, dropped, survivors)
}

#[test]
fn zeroing_or_dropping_any_recovered_value_fails_the_check_in_both_golden_cases() {
    for (name, json) in [("cavity", CAVITY_V1), ("channel", CHANNEL_V1)] {
        let (zeroed, dropped, survivors) = run_harness(json);
        assert!(survivors.is_empty(), "{name}: {survivors:#?}");
        assert!(zeroed >= 25, "{name}: only {zeroed} zero mutations");
        assert!(dropped >= 25, "{name}: only {dropped} drop mutations");
    }
}

#[test]
fn an_inlet_written_as_a_wall_in_the_mesh_fails_and_names_the_boundary() {
    let (study, files) = case(CHANNEL_V1);
    let bad = mutated(&files, BLOCK_MESH, |t| {
        t.replace(
            "inlet\n    {\n        type patch;",
            "inlet\n    {\n        type wall;",
        )
    });
    assert_ne!(text(&bad, BLOCK_MESH), text(&files, BLOCK_MESH));
    let found = check(&bad, &study);
    let kind = found
        .iter()
        .find(|d| d.pointer == "/boundary_conditions/inlet/kind")
        .unwrap_or_else(|| panic!("{found:?}"));
    assert_eq!(kind.code, "CFD-RT-001");
    assert_eq!(kind.setting, "inlet boundary kind");
    assert!(
        kind.message
            .contains("is written as moving wall in the case, but the study has velocity inlet"),
        "{}",
        kind.message
    );
    assert!(
        kind.message
            .contains("on the left side of the domain (XMin)")
    );
    assert!(found.iter().all(|d| d.code == "CFD-RT-001"), "{found:?}");
}

#[test]
fn an_inlet_written_as_a_wall_in_the_velocity_field_fails_and_spells_out_what_was_found() {
    let (study, files) = case(CHANNEL_V1);
    let bad = mutated(&files, VELOCITY, |t| {
        t.replace("inlet { type fixedValue;", "inlet { type noSlip;")
    });
    assert_ne!(text(&bad, VELOCITY), text(&files, VELOCITY));
    let found = check(&bad, &study);
    let kind = found
        .iter()
        .find(|d| d.pointer == "/boundary_conditions/inlet/kind")
        .unwrap_or_else(|| panic!("{found:?}"));
    assert_eq!(kind.code, "CFD-RT-001");
    assert!(
        kind.message
            .contains("written as unrecognised combination (patch, noSlip, zeroGradient)"),
        "{}",
        kind.message
    );
    assert!(kind.message.contains("the study has velocity inlet"));
    assert!(
        found
            .iter()
            .any(|d| d.pointer == "/boundary_conditions/inlet/velocity"),
        "the inlet's speed is no longer applied: {found:?}"
    );
}

#[test]
fn vertices_that_are_not_the_boxs_corners_in_the_blocks_order_are_refused() {
    let (study, files) = case(CAVITY_V1);
    let bad = mutated(&files, BLOCK_MESH, |t| {
        t.replace("(0.1 0 0.05)", "(TMP)")
            .replace("(0 0.1 0.05)", "(0.1 0 0.05)")
            .replace("(TMP)", "(0 0.1 0.05)")
    });
    assert_ne!(text(&bad, BLOCK_MESH), text(&files, BLOCK_MESH));
    let found = check(&bad, &study);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, "CFD-RT-002");
    assert!(
        found[0].message.contains("axis-aligned box"),
        "{}",
        found[0].message
    );
}
