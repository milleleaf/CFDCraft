use cfdcraft_backend_esi::{
    CaseFile, CaseTree, Dialect, compile,
    guard::scan_tree,
    monitors::{BalancePatch, balance_patches, functions_block, monitor_fo_names},
};
use cfdcraft_model::{ResolvedStudy, Study, resolve};

const CAVITY: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");
const CHANNEL: &str = include_str!("../../cfdcraft-model/tests/fixtures/channel_v1.json");

/// The `centreline` object as the writer has always produced it for the cavity.
const CAVITY_CENTRELINE: &str = "    centreline\n    {\n        type sets;\n        libs (sampling);\n        writeControl writeTime;\n        setFormat raw;\n        interpolationScheme cellPoint;\n        fields (U);\n        sets\n        (\n            vertical\n            {\n                type uniform;\n                axis y;\n                start (0.05 0 0.025);\n                end (0.05 0.1 0.025);\n                nPoints 64;\n            }\n            horizontal\n            {\n                type uniform;\n                axis x;\n                start (0 0.05 0.025);\n                end (0.1 0.05 0.025);\n                nPoints 64;\n            }\n        );\n    }\n";

/// The study text with these members added at the front of its `monitors` object.
fn with_monitors(json: &str, members: &str) -> String {
    json.replacen(
        "\"monitors\": {",
        &format!("\"monitors\": {{ {members},"),
        1,
    )
}

fn resolved(json: &str) -> ResolvedStudy {
    resolve(&Study::from_json(json).unwrap()).unwrap()
}

fn tree(study: &ResolvedStudy) -> CaseTree {
    compile(study, Dialect::EsiV2512).unwrap()
}

fn control(study: &ResolvedStudy) -> String {
    let tree = tree(study);
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path.as_os_str() == "system/controlDict")
        .unwrap();
    String::from_utf8(file.bytes.clone()).unwrap()
}

/// The text of one function object, from its name to its closing brace.
fn object<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text
        .find(&format!("\n    {name}\n    {{\n"))
        .unwrap_or_else(|| panic!("no object {name}"))
        + 1;
    let end = text[start..].find("\n    }\n").unwrap() + start + "\n    }\n".len();
    &text[start..end]
}

const LISTED: &str = r#""probes": { "lidprobe": { "x": {"value": 0.05, "unit": "m"}, "y": {"value": 0.05, "unit": "m"}, "field": "velocity" } },
        "lines": { "midline": { "x0": {"value": 0.0, "unit": "m"}, "y0": {"value": 0.05, "unit": "m"}, "x1": {"value": 0.1, "unit": "m"}, "y1": {"value": 0.05, "unit": "m"}, "samples": 16, "field": "pressure" } },
        "surfaces": {
            "a_flux": { "boundary": "lid", "quantity": "flux" },
            "b_avg": { "boundary": "walls", "quantity": "pressure_average" },
            "c_int": { "boundary": "walls", "quantity": "pressure_integral" }
        }"#;

#[test]
fn the_cavity_control_dict_has_the_default_probes_and_the_flux_objects() {
    let text = control(&resolved(CAVITY));
    for name in [
        "probe_1",
        "probe_2",
        "probe_3",
        "probe_4",
        "probe_5",
        "balance_movingWall",
        "balance_fixedWalls",
    ] {
        object(&text, name);
    }
    assert!(!text.contains("probe_6"));
    assert!(!text.contains("balance_frontAndBack"));
    assert!(text.contains("runTimeModifiable true;\n"));
    assert!(text.contains("purgeWrite 2;\n"));
    assert!(!text.contains("purgeWrite 0;"));
    assert!(!text.contains("runTimeModifiable false;"));
    assert_eq!(object(&text, "centreline"), CAVITY_CENTRELINE);
    assert_eq!(
        object(&text, "probe_1"),
        "    probe_1\n    {\n        type probes;\n        libs (sampling);\n        writeControl timeStep;\n        writeInterval 1;\n        probeLocations\n        (\n            (0.05 0.05 0.025)\n        );\n        fields (U);\n    }\n"
    );
    assert_eq!(
        object(&text, "balance_movingWall"),
        "    balance_movingWall\n    {\n        type surfaceFieldValue;\n        libs (fieldFunctionObjects);\n        writeControl timeStep;\n        writeInterval 1;\n        log no;\n        writeFields no;\n        regionType patch;\n        name movingWall;\n        operation sum;\n        fields (phi);\n    }\n"
    );
    assert_eq!(
        monitor_fo_names(&resolved(CAVITY)),
        [
            "probe_1",
            "probe_2",
            "probe_3",
            "probe_4",
            "probe_5",
            "balance_movingWall",
            "balance_fixedWalls"
        ]
    );
}

#[test]
fn names_never_come_from_labels() {
    let study = resolved(&with_monitors(CAVITY, LISTED));
    for file in &tree(&study).files {
        let text = String::from_utf8_lossy(&file.bytes);
        for label in ["lidprobe", "midline", "a_flux", "b_avg", "c_int"] {
            assert!(
                !text.contains(label),
                "{} holds {label}",
                file.rel_path.display()
            );
        }
    }
    let text = control(&study);
    for name in ["probe_1", "line_1", "surface_1", "surface_2", "surface_3"] {
        object(&text, name);
    }
}

#[test]
fn a_listed_monitor_replaces_the_default_probes_and_the_flux_objects_stay() {
    let members = r#""probes": { "one": { "x": {"value": 0.02, "unit": "m"}, "y": {"value": 0.03, "unit": "m"}, "field": "pressure" } }"#;
    let study = resolved(&with_monitors(CAVITY, members));
    let text = control(&study);
    assert_eq!(
        object(&text, "probe_1"),
        "    probe_1\n    {\n        type probes;\n        libs (sampling);\n        writeControl timeStep;\n        writeInterval 1;\n        probeLocations\n        (\n            (0.02 0.03 0.025)\n        );\n        fields (p);\n    }\n"
    );
    assert!(!text.contains("probe_2"));
    assert_eq!(
        monitor_fo_names(&study),
        ["probe_1", "balance_movingWall", "balance_fixedWalls"]
    );
    assert_eq!(
        balance_patches(&study),
        balance_patches(&resolved(CAVITY)),
        "the balances do not depend on the listed monitors"
    );
}

#[test]
fn an_inlet_case_has_inlet_and_outlet_balances_and_marks_the_reference() {
    let study = resolved(CHANNEL);
    let patch = |patch: &'static str, inlet: bool| BalancePatch {
        fo: format!("balance_{patch}"),
        patch,
        inlet,
    };
    assert_eq!(
        balance_patches(&study),
        [
            patch("inlet", true),
            patch("outlet", false),
            patch("fixedWalls", false),
            patch("symmetry", false)
        ]
    );
    let text = control(&study);
    for name in [
        "balance_inlet",
        "balance_outlet",
        "balance_fixedWalls",
        "balance_symmetry",
    ] {
        object(&text, name);
    }
    assert!(!text.contains("balance_frontAndBack"));
    assert!(object(&text, "balance_inlet").contains("name inlet;\n"));
    assert!(object(&text, "probe_1").contains("(0.5 0.05 0.005)"));
}

#[test]
fn a_line_and_a_surface_render_their_blocks() {
    let text = control(&resolved(&with_monitors(CAVITY, LISTED)));
    assert_eq!(
        object(&text, "line_1"),
        "    line_1\n    {\n        type sets;\n        libs (sampling);\n        writeControl writeTime;\n        setFormat raw;\n        interpolationScheme cellPoint;\n        fields (p);\n        sets\n        (\n            line\n            {\n                type uniform;\n                axis xyz;\n                start (0.0 0.05 0.025);\n                end (0.1 0.05 0.025);\n                nPoints 16;\n            }\n        );\n    }\n"
    );
    let surface = |name: &str, patch: &str, op: &str, field: &str| {
        format!(
            "    {name}\n    {{\n        type surfaceFieldValue;\n        libs (fieldFunctionObjects);\n        writeControl timeStep;\n        writeInterval 1;\n        log no;\n        writeFields no;\n        regionType patch;\n        name {patch};\n        operation {op};\n        fields ({field});\n    }}\n"
        )
    };
    assert_eq!(
        object(&text, "surface_1"),
        surface("surface_1", "movingWall", "sum", "phi")
    );
    assert_eq!(
        object(&text, "surface_2"),
        surface("surface_2", "fixedWalls", "areaAverage", "p")
    );
    assert_eq!(
        object(&text, "surface_3"),
        surface("surface_3", "fixedWalls", "areaIntegrate", "p")
    );
    // Listed probes and lines replace the default probes; the order is probes, lines, surfaces,
    // then the balances.
    let at = |name: &str| text.find(&format!("\n    {name}\n")).unwrap();
    assert!(at("probe_1") < at("line_1"));
    assert!(at("line_1") < at("surface_1"));
    assert!(at("surface_3") < at("balance_movingWall"));
}

#[test]
fn the_output_is_deterministic() {
    let study = resolved(&with_monitors(CAVITY, LISTED));
    assert_eq!(tree(&study), tree(&study));
    assert_eq!(functions_block(&study), functions_block(&study));
    assert!(functions_block(&study).starts_with("    probe_1\n"));
}

#[test]
fn the_whole_compiled_case_passes_the_scan_with_every_monitor_kind() {
    for json in [
        CAVITY.to_owned(),
        with_monitors(CAVITY, LISTED),
        CHANNEL.to_owned(),
    ] {
        assert_eq!(scan_tree(&tree(&resolved(&json))), vec![]);
    }
}

#[test]
fn a_coded_type_or_a_system_call_among_monitors_is_still_refused() {
    let study = resolved(CAVITY);
    for (word, code) in [("coded", "CFD-GUARD-002"), ("systemCall", "CFD-GUARD-003")] {
        let mut edited = tree(&study);
        let control: &mut CaseFile = edited
            .files
            .iter_mut()
            .find(|f| f.rel_path.as_os_str() == "system/controlDict")
            .unwrap();
        let text = String::from_utf8(control.bytes.clone()).unwrap();
        let text = text.replacen("type probes;", &format!("type {word};"), 1);
        control.bytes = text.into_bytes();
        let refused = scan_tree(&edited);
        assert!(
            refused
                .iter()
                .any(|r| r.rule.code() == code && r.entry == word),
            "{word}: {refused:?}"
        );
    }
}
