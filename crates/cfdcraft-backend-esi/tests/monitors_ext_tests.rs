//! The force coefficient, boundary probe and heat-flux function objects of the ESI writer, the
//! patch lookup by label, and the two golden function blocks.

use cfdcraft_backend_esi::{
    CaseTree, Dialect, compile,
    guard::scan_tree,
    monitors::{balance_patches, monitor_fo_names},
    patches::patch_of_label,
};
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use std::path::Path;

/// The cylinder channel (0.41 m high, 0.01 m deep, a 0.1 m cylinder at 0.2 m, 0.2 m) as a transient
/// laminar study. `obstacle` is the label of the cylinder boundary, `monitors` is empty or starts
/// with a comma and holds more members of the `monitors` object.
fn cylinder_json(obstacle: &str, monitors: &str) -> String {
    format!(
        r#"{{"schema_version":1,"domain":{{"length_x":{{"value":2.2,"unit":"m"}},"length_y":{{"value":0.41,"unit":"m"}},"length_z":{{"value":0.01,"unit":"m"}},"open":true,"shape":{{"kind":"cylinder_channel","diameter":{{"value":0.1,"unit":"m"}},"centre_x":{{"value":0.2,"unit":"m"}},"centre_y":{{"value":0.2,"unit":"m"}}}}}},"fluid":{{"kinematic_viscosity":{{"value":0.001,"unit":"m2/s"}}}},"boundary_conditions":{{"inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{{"value":1.0,"unit":"m/s"}}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"channel_walls":{{"faces":["YMin","YMax"],"kind":"NoSlipWall"}},"{obstacle}":{{"faces":["Obstacle"],"kind":"NoSlipWall"}}}},"mesh":{{"cells_x":114,"cells_y":44,"cells_z":1}},"solver":{{"solver":"FromPhysics","end_time":{{"value":1,"unit":"s"}},"write_interval":{{"value":1,"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"physics":{{"time":{{"mode":"transient","delta_t":{{"value":0.001,"unit":"s"}}}}}},"monitors":{{"centreline_u_sample_count":64{monitors}}}}}"#
    )
}

/// A force monitor on the cylinder boundary, with a reference length in metres.
fn force_member(label: &str, length: &str) -> String {
    format!(
        r#""{label}":{{"boundary":"cylinder","reference_velocity":{{"value":1,"unit":"m/s"}},"reference_length":{{"value":{length},"unit":"m"}}}}"#
    )
}

/// Two pressure probes on the cylinder boundary, at its front and rear points.
fn probe_members(front: &str, rear: &str) -> String {
    format!(
        r#""{front}":{{"x":{{"value":0.15,"unit":"m"}},"y":{{"value":0.2,"unit":"m"}},"field":"pressure","boundary":"cylinder"}},"{rear}":{{"x":{{"value":0.25,"unit":"m"}},"y":{{"value":0.2,"unit":"m"}},"field":"pressure","boundary":"cylinder"}}"#
    )
}

/// The cylinder with a force and two boundary probes, named by these labels.
fn cylinder_monitors(force: &str, front: &str, rear: &str) -> String {
    format!(
        r#","forces":{{{}}},"probes":{{{}}}"#,
        force_member(force, "0.1"),
        probe_members(front, rear)
    )
}

/// The closed cavity heated from the left and cooled from the right with buoyancy on; `monitors`
/// is empty or starts with a comma.
fn heated_json(monitors: &str) -> String {
    format!(
        r#"{{"schema_version":1,"domain":{{"length_x":{{"value":0.1,"unit":"m"}},"length_y":{{"value":0.1,"unit":"m"}},"length_z":{{"value":0.05,"unit":"m"}},"open":false}},"fluid":{{"kinematic_viscosity":{{"value":0.0001,"unit":"m2/s"}}}},"boundary_conditions":{{"hot":{{"faces":["XMin"],"kind":"NoSlipWall","thermal":{{"kind":"fixed_temperature","value":{{"value":300.5,"unit":"K"}}}}}},"cold":{{"faces":["XMax"],"kind":"NoSlipWall","thermal":{{"kind":"fixed_temperature","value":{{"value":299.5,"unit":"K"}}}}}},"adiabatic":{{"faces":["YMin","YMax"],"kind":"NoSlipWall","thermal":{{"kind":"adiabatic"}}}}}},"mesh":{{"cells_x":128,"cells_y":128,"cells_z":1}},"solver":{{"solver":"FromPhysics","end_time":{{"value":5000,"unit":"s"}},"write_interval":{{"value":500,"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"physics":{{"buoyancy":{{"gravity":9.81,"beta":0.003,"reference_temperature":{{"value":300,"unit":"K"}},"prandtl":0.71,"turbulent_prandtl":0.85}}}},"monitors":{{"centreline_u_sample_count":64{monitors}}}}}"#
    )
}

/// Heat-flux surfaces on the cold and the hot wall and two mid-plane velocity lines, named by
/// these labels (the order of the labels decides the numbering, so keep cold before hot and
/// horizontal before vertical).
fn heated_monitors(cold: &str, hot: &str, horizontal: &str, vertical: &str) -> String {
    format!(
        r#","surfaces":{{"{cold}":{{"boundary":"cold","quantity":"heat_flux"}},"{hot}":{{"boundary":"hot","quantity":"heat_flux"}}}},"lines":{{"{horizontal}":{{"x0":{{"value":0,"unit":"m"}},"y0":{{"value":0.05,"unit":"m"}},"x1":{{"value":0.1,"unit":"m"}},"y1":{{"value":0.05,"unit":"m"}},"samples":16,"field":"velocity"}},"{vertical}":{{"x0":{{"value":0.05,"unit":"m"}},"y0":{{"value":0,"unit":"m"}},"x1":{{"value":0.05,"unit":"m"}},"y1":{{"value":0.1,"unit":"m"}},"samples":16,"field":"velocity"}}}}"#
    )
}

fn resolved(json: &str) -> ResolvedStudy {
    resolve(&Study::from_json(json).unwrap()).unwrap_or_else(|e| panic!("{e}"))
}

fn tree(s: &ResolvedStudy) -> CaseTree {
    compile(s, Dialect::EsiV2512).unwrap()
}

fn control(s: &ResolvedStudy) -> String {
    let tree = tree(s);
    let file = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new("system/controlDict"))
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

/// The lines of an object with their indentation removed.
fn trimmed(object: &str) -> Vec<&str> {
    object.lines().map(str::trim).collect()
}

/// The position of an object in the control dictionary.
fn at(text: &str, name: &str) -> usize {
    text.find(&format!("\n    {name}\n"))
        .unwrap_or_else(|| panic!("no object {name}"))
}

/// No file of the tree holds the word, once the fixed patch name `frontAndBack` is set aside.
fn tree_lacks(s: &ResolvedStudy, word: &str) -> bool {
    tree(s).files.iter().all(|f| {
        !String::from_utf8_lossy(&f.bytes)
            .replace("frontAndBack", "")
            .contains(word)
    })
}

#[test]
fn a_force_object_sets_the_lift_axis_to_y() {
    let s = resolved(&cylinder_json(
        "cylinder",
        &format!(
            r#","forces":{{{}}}"#,
            force_member("cylinder_forces", "0.1")
        ),
    ));
    let text = control(&s);
    assert_eq!(
        trimmed(object(&text, "force_1")),
        [
            "force_1",
            "{",
            "type forceCoeffs;",
            "libs (forces);",
            "patches (obstacle);",
            "rho rhoInf;",
            "rhoInf 1;",
            "magUInf 1;",
            "lRef 0.1;",
            "Aref 0.001;",
            "origin (0 0 0);",
            "e1 (1 0 0);",
            "e3 (0 1 0);",
            "writeControl timeStep;",
            "writeInterval 1;",
            "}"
        ]
    );
}

#[test]
fn the_force_object_passes_the_guard_and_names_no_author_text() {
    let s = resolved(&cylinder_json(
        "cylinder",
        &format!(
            r#","forces":{{{}}}"#,
            force_member("cylinder_forces", "0.1")
        ),
    ));
    assert!(scan_tree(&tree(&s)).is_empty());
    assert!(tree_lacks(&s, "cylinder_forces"));
    assert!(tree_lacks(&s, "cylinder"));
    let text = control(&s);
    assert!(!text.contains('#'));
    assert!(!text.contains("code"));
}

#[test]
fn monitor_names_include_the_forces() {
    let members = format!(
        r#","forces":{{{}}},"probes":{{{}}},"lines":{{"mid":{{"x0":{{"value":0.5,"unit":"m"}},"y0":{{"value":0.1,"unit":"m"}},"x1":{{"value":1.5,"unit":"m"}},"y1":{{"value":0.1,"unit":"m"}},"samples":16,"field":"velocity"}}}},"surfaces":{{"s":{{"boundary":"outlet","quantity":"flux"}}}}"#,
        force_member("cylinder_forces", "0.1"),
        probe_members("front", "rear")
    );
    let s = resolved(&cylinder_json("cylinder", &members));
    let names = monitor_fo_names(&s);
    assert_eq!(
        names[..5],
        ["probe_1", "probe_2", "line_1", "force_1", "surface_1"]
    );
    let balances: Vec<String> = balance_patches(&s).into_iter().map(|b| b.fo).collect();
    assert_eq!(names[5..], balances[..]);
    assert_eq!(balances.len(), 4, "inlet, outlet, fixedWalls, obstacle");
    let text = control(&s);
    assert!(at(&text, "line_1") < at(&text, "force_1"));
    assert!(at(&text, "force_1") < at(&text, "surface_1"));
}

#[test]
fn surfaces_find_their_patch_by_label() {
    // By kind, the cylinder and the channel walls (both no-slip walls) would be one patch.
    let members = r#","surfaces":{"a_cylinder":{"boundary":"cylinder","quantity":"flux"},"b_outlet":{"boundary":"outlet","quantity":"flux"},"c_walls":{"boundary":"channel_walls","quantity":"pressure_average"}}"#;
    let s = resolved(&cylinder_json("cylinder", members));
    let text = control(&s);
    for (name, label, patch) in [
        ("surface_1", "cylinder", "obstacle"),
        ("surface_2", "outlet", "outlet"),
        ("surface_3", "channel_walls", "fixedWalls"),
    ] {
        assert_eq!(patch_of_label(&s, label), Some(patch));
        assert!(
            object(&text, name).contains(&format!("        name {patch};\n")),
            "{name}"
        );
    }
}

#[test]
fn two_forces_are_numbered_in_label_order() {
    let members = format!(
        r#","forces":{{{},{}}}"#,
        force_member("b_drag", "0.2"),
        force_member("a_drag", "0.1")
    );
    let s = resolved(&cylinder_json("cylinder", &members));
    let text = control(&s);
    assert!(object(&text, "force_1").contains("        lRef 0.1;\n"));
    assert!(object(&text, "force_2").contains("        lRef 0.2;\n"));
    assert!(tree_lacks(&s, "a_drag") && tree_lacks(&s, "b_drag"));
    assert_eq!(monitor_fo_names(&s)[..2], ["force_1", "force_2"]);
}

#[test]
fn a_boundary_probe_is_a_patch_probe() {
    let s = resolved(&cylinder_json(
        "cylinder",
        &format!(r#","probes":{{{}}}"#, probe_members("front", "rear")),
    ));
    let text = control(&s);
    for (name, point) in [
        ("probe_1", "(0.15 0.2 0.005)"),
        ("probe_2", "(0.25 0.2 0.005)"),
    ] {
        assert_eq!(
            trimmed(object(&text, name)),
            [
                name,
                "{",
                "type patchProbes;",
                "libs (sampling);",
                "writeControl timeStep;",
                "writeInterval 1;",
                "patches (obstacle);",
                "probeLocations",
                "(",
                point,
                ");",
                "fields (p);",
                "}"
            ]
        );
    }
    // A probe in the flow is written exactly as before.
    let flow = r#","probes":{"mid":{"x":{"value":1,"unit":"m"},"y":{"value":0.2,"unit":"m"},"field":"velocity"}}"#;
    let text = control(&resolved(&cylinder_json("cylinder", flow)));
    assert_eq!(
        object(&text, "probe_1"),
        "    probe_1\n    {\n        type probes;\n        libs (sampling);\n        writeControl timeStep;\n        writeInterval 1;\n        probeLocations\n        (\n            (1.0 0.2 0.005)\n        );\n        fields (U);\n    }\n"
    );
}

#[test]
fn a_heat_flux_surface_needs_a_gradient_object_first() {
    let s = resolved(&heated_json(&heated_monitors(
        "cold_flux",
        "hot_flux",
        "mid_horizontal",
        "mid_vertical",
    )));
    let text = control(&s);
    assert_eq!(
        trimmed(object(&text, "heat_grad")),
        [
            "heat_grad",
            "{",
            "type grad;",
            "libs (fieldFunctionObjects);",
            "field T;",
            "result gradT;",
            "writeControl none;",
            "}"
        ]
    );
    assert_eq!(text.matches("\n    heat_grad\n").count(), 1);
    assert!(at(&text, "heat_grad") < at(&text, "surface_1"));
    // Surfaces are numbered in label order: the cold wall first.
    for (name, label) in [("surface_1", "cold"), ("surface_2", "hot")] {
        let patch = patch_of_label(&s, label).unwrap();
        assert_eq!(
            trimmed(object(&text, name)),
            [
                name,
                "{",
                "type surfaceFieldValue;",
                "libs (fieldFunctionObjects);",
                "writeControl timeStep;",
                "writeInterval 1;",
                "log no;",
                "writeFields no;",
                "regionType patch;",
                format!("name {patch};").as_str(),
                "operation areaIntegrate;",
                "fields (gradT);",
                "}"
            ]
        );
    }
    // No heat-flux surface, no gradient object.
    let plain = control(&resolved(&heated_json("")));
    assert!(!plain.contains("heat_grad") && !plain.contains("gradT"));
}

#[test]
fn the_guard_passes_and_no_author_text_is_written() {
    let cylinder = resolved(&cylinder_json(
        "cylinder",
        &cylinder_monitors("cylinder_forces", "front", "rear"),
    ));
    let heated = resolved(&heated_json(&heated_monitors(
        "cold_flux",
        "hot_flux",
        "mid_horizontal",
        "mid_vertical",
    )));
    assert!(scan_tree(&tree(&cylinder)).is_empty());
    assert!(scan_tree(&tree(&heated)).is_empty());
    // `front` is also the start of the fixed patch name `frontAndBack`, which `tree_lacks` sets
    // aside.
    for word in ["front", "rear", "cylinder_forces"] {
        assert!(tree_lacks(&cylinder, word), "{word}");
    }
    for word in ["hot_flux", "cold_flux", "mid_horizontal", "mid_vertical"] {
        assert!(tree_lacks(&heated, word), "{word}");
    }
}

#[test]
fn pressure_and_flux_surfaces_are_unchanged() {
    let members = r#","surfaces":{"a":{"boundary":"outlet","quantity":"flux"},"b":{"boundary":"channel_walls","quantity":"pressure_average"},"c":{"boundary":"channel_walls","quantity":"pressure_integral"}}"#;
    let text = control(&resolved(&cylinder_json("cylinder", members)));
    let surface = |name: &str, patch: &str, op: &str, field: &str| {
        format!(
            "    {name}\n    {{\n        type surfaceFieldValue;\n        libs (fieldFunctionObjects);\n        writeControl timeStep;\n        writeInterval 1;\n        log no;\n        writeFields no;\n        regionType patch;\n        name {patch};\n        operation {op};\n        fields ({field});\n    }}\n"
        )
    };
    assert_eq!(
        object(&text, "surface_1"),
        surface("surface_1", "outlet", "sum", "phi")
    );
    assert_eq!(
        object(&text, "surface_2"),
        surface("surface_2", "fixedWalls", "areaAverage", "p")
    );
    assert_eq!(
        object(&text, "surface_3"),
        surface("surface_3", "fixedWalls", "areaIntegrate", "p")
    );
    assert!(!text.contains("heat_grad"));
}

#[test]
fn monitor_names_include_the_gradient_object() {
    let s = resolved(&heated_json(&heated_monitors(
        "cold_flux",
        "hot_flux",
        "mid_horizontal",
        "mid_vertical",
    )));
    let names = monitor_fo_names(&s);
    assert_eq!(
        names[..5],
        ["line_1", "line_2", "heat_grad", "surface_1", "surface_2"]
    );
    let plain = monitor_fo_names(&resolved(&heated_json("")));
    assert!(!plain.iter().any(|n| n == "heat_grad"));
}

/// Compares the control dictionary with its golden file; a difference gives the whole actual text
/// so it can be pasted into the golden file as it is.
fn golden_difference(name: &str, actual: &str) -> Option<String> {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
        .join("system/controlDict");
    let want = std::fs::read_to_string(&golden).unwrap();
    (actual != want)
        .then(|| format!("{name} differs from its golden file; the actual text is:\n{actual}"))
}

fn golden_cylinder() -> ResolvedStudy {
    resolved(&cylinder_json(
        "cylinder",
        &cylinder_monitors("cylinder_forces", "front", "rear"),
    ))
}

fn golden_heated() -> ResolvedStudy {
    resolved(&heated_json(&heated_monitors(
        "cold_flux",
        "hot_flux",
        "mid_horizontal",
        "mid_vertical",
    )))
}

#[test]
fn the_cylinder_control_dict_equals_its_golden() {
    let difference = golden_difference("cylinder-monitors-v2512", &control(&golden_cylinder()));
    assert!(difference.is_none(), "{}", difference.unwrap_or_default());
}

#[test]
fn the_heated_control_dict_equals_its_golden() {
    let difference = golden_difference("heated-monitors-v2512", &control(&golden_heated()));
    assert!(difference.is_none(), "{}", difference.unwrap_or_default());
}

#[test]
fn the_order_of_objects_is_fixed() {
    let (cylinder, heated) = (golden_cylinder(), golden_heated());
    let (first_cylinder_balance, first_heated_balance) = (
        balance_patches(&cylinder)[0].fo.clone(),
        balance_patches(&heated)[0].fo.clone(),
    );
    let (cylinder, heated) = (control(&cylinder), control(&heated));
    let order = |text: &str, names: &[&str]| {
        let places: Vec<usize> = names.iter().map(|n| at(text, n)).collect();
        assert!(places.windows(2).all(|w| w[0] < w[1]), "{names:?}");
    };
    order(
        &cylinder,
        &[
            "centreline",
            "probe_1",
            "probe_2",
            "force_1",
            &first_cylinder_balance,
        ],
    );
    order(
        &heated,
        &[
            "centreline",
            "line_1",
            "line_2",
            "heat_grad",
            "surface_1",
            "surface_2",
            &first_heated_balance,
        ],
    );
}

#[test]
fn the_goldens_do_not_change_with_a_label_rename() {
    let renamed_cylinder = resolved(&cylinder_json(
        "cylinder",
        &cylinder_monitors("zz_force", "aa_point", "bb_point"),
    ));
    assert_eq!(control(&renamed_cylinder), control(&golden_cylinder()));
    let renamed_heated = resolved(&heated_json(&heated_monitors(
        "x_cold", "y_hot", "p_across", "q_up",
    )));
    assert_eq!(control(&renamed_heated), control(&golden_heated()));
}
