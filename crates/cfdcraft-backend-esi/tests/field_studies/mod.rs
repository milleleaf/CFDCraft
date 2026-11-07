//! Studies and helpers for the tests of the turbulence, buoyancy and transient files. Every study is
//! JSON text that goes through `resolve`, so a combination the model refuses fails the test that
//! builds it.
#![allow(dead_code)]

use cfdcraft_backend_esi::{CaseTree, Dialect, compile, guard::scan_tree};
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use std::path::Path;

pub fn resolved(json: &str) -> ResolvedStudy {
    resolve(&Study::from_json(json).unwrap()).unwrap_or_else(|e| panic!("{e}"))
}

pub fn tree(s: &ResolvedStudy) -> CaseTree {
    compile(s, Dialect::EsiV2512).unwrap()
}

pub fn has_file(tree: &CaseTree, name: &str) -> bool {
    tree.files.iter().any(|f| f.rel_path == Path::new(name))
}

pub fn file(tree: &CaseTree, name: &str) -> String {
    let f = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap_or_else(|| panic!("the case has no file {name}"));
    String::from_utf8(f.bytes.clone()).unwrap()
}

/// The trimmed lines of a text.
pub fn lines(text: &str) -> Vec<&str> {
    text.lines().map(str::trim).collect()
}

/// The trimmed lines of the block that starts at the line `name`, up to and including its closing
/// brace.
pub fn block(text: &str, name: &str) -> Vec<String> {
    let all = lines(text);
    let from = all
        .iter()
        .position(|l| *l == name)
        .unwrap_or_else(|| panic!("no block {name} in\n{text}"));
    let len = all[from..].iter().position(|l| *l == "}").unwrap();
    all[from..=from + len]
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

/// The trimmed line a field file holds for a patch.
pub fn entry(tree: &CaseTree, name: &str, patch: &str) -> String {
    let start = format!("{patch} {{");
    let text = file(tree, name);
    lines(&text)
        .into_iter()
        .find(|l| l.starts_with(&start))
        .unwrap_or_else(|| panic!("{name} has no entry for {patch}"))
        .to_owned()
}

/// A number as the writer prints it.
pub fn num(value: f64) -> String {
    format!("{value:?}")
}

/// The tree holds no `#` and no word that starts with `code`, and the safety scan finds nothing.
pub fn assert_clean(tree: &CaseTree) {
    assert!(
        scan_tree(tree).is_empty(),
        "the safety scan refused the case"
    );
    for f in &tree.files {
        let text = String::from_utf8_lossy(&f.bytes).to_lowercase();
        assert!(!text.contains('#'), "{}", f.rel_path.display());
        assert!(
            !text
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| w.starts_with("code")),
            "{}",
            f.rel_path.display()
        );
    }
}

/// Compares the listed files with their golden files. An empty answer means they are equal; any
/// other answer holds the whole actual text of every file that differs, ready to paste.
pub fn golden_differences(name: &str, tree: &CaseTree, files: &[&str]) -> String {
    let mut out = String::new();
    for f in files {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name)
            .join(f);
        let want = std::fs::read_to_string(path).unwrap_or_default();
        let actual = file(tree, f);
        if actual != want {
            out.push_str(&format!(
                "{name}/{f} differs from its golden file; the actual text is:\n{actual}\n"
            ));
        }
    }
    out
}

/// The backward step with a start-up section. `physics` is a JSON object, `inlet_extra` is empty
/// or starts with a comma and adds members to the inlet boundary.
pub fn step_json(physics: &str, end: &str, interval: &str, inlet_extra: &str) -> String {
    format!(
        r#"{{"schema_version":1,"physics":{physics},"domain":{{"length_x":{{"value":2.286,"unit":"m"}},"length_y":{{"value":0.1143,"unit":"m"}},"length_z":{{"value":0.0127,"unit":"m"}},"open":true,"shape":{{"kind":"backward_step","step_height":{{"value":12.7,"unit":"mm"}},"upstream_length":{{"value":1.651,"unit":"m"}},"startup_length":{{"value":0.254,"unit":"m"}}}}}},"fluid":{{"kinematic_viscosity":{{"value":0.0000156,"unit":"m2/s"}}}},"boundary_conditions":{{"inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{{"value":44.2,"unit":"m/s"}}{inlet_extra}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"floor_walls":{{"faces":["YMin"],"kind":"NoSlipWall"}},"top_wall":{{"faces":["YMax"],"kind":"NoSlipWall"}}}},"mesh":{{"cells_x":220,"cells_y":114,"cells_z":1}},"solver":{{"solver":"FromPhysics","end_time":{{"value":{end},"unit":"s"}},"write_interval":{{"value":{interval},"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"monitors":{{"centreline_u_sample_count":64}}}}"#
    )
}

/// The cylinder channel. `physics` is a JSON object, `inlet_extra` is empty or starts with a comma.
pub fn cylinder_json(physics: &str, end: &str, interval: &str, inlet_extra: &str) -> String {
    format!(
        r#"{{"schema_version":1,"physics":{physics},"domain":{{"length_x":{{"value":2.2,"unit":"m"}},"length_y":{{"value":0.41,"unit":"m"}},"length_z":{{"value":0.1,"unit":"m"}},"open":true,"shape":{{"kind":"cylinder_channel","diameter":{{"value":0.1,"unit":"m"}},"centre_x":{{"value":0.2,"unit":"m"}},"centre_y":{{"value":0.2,"unit":"m"}}}}}},"fluid":{{"kinematic_viscosity":{{"value":0.001,"unit":"m2/s"}}}},"boundary_conditions":{{"inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{{"value":1.0,"unit":"m/s"}}{inlet_extra}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"channel_walls":{{"faces":["YMin","YMax"],"kind":"NoSlipWall"}},"cylinder":{{"faces":["Obstacle"],"kind":"NoSlipWall"}}}},"mesh":{{"cells_x":114,"cells_y":44,"cells_z":1}},"solver":{{"solver":"FromPhysics","end_time":{{"value":{end},"unit":"s"}},"write_interval":{{"value":{interval},"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"monitors":{{"centreline_u_sample_count":64}}}}"#
    )
}

/// A closed square cavity with a hot wall on the left, a cold wall on the right and two
/// insulated walls, with Boussinesq buoyancy. `head` is empty or holds physics members that end
/// with a comma; the three wall temperatures are given in kelvin.
pub fn heated_json(head: &str, end: &str, interval: &str) -> String {
    heated_walls_json(head, end, interval, "300.5", "299.5")
}

pub fn heated_walls_json(head: &str, end: &str, interval: &str, left: &str, right: &str) -> String {
    format!(
        r#"{{"schema_version":1,"physics":{{{head}"buoyancy":{{"gravity":9.81,"beta":0.003,"reference_temperature":{{"value":300.0,"unit":"K"}},"prandtl":0.71,"turbulent_prandtl":0.85}}}},"domain":{{"length_x":{{"value":0.1,"unit":"m"}},"length_y":{{"value":0.1,"unit":"m"}},"length_z":{{"value":0.05,"unit":"m"}},"open":false}},"fluid":{{"kinematic_viscosity":{{"value":0.0001,"unit":"m2/s"}}}},"boundary_conditions":{{"hot":{{"faces":["XMin"],"kind":"NoSlipWall","thermal":{{"kind":"fixed_temperature","value":{{"value":{left},"unit":"K"}}}}}},"cold":{{"faces":["XMax"],"kind":"NoSlipWall","thermal":{{"kind":"fixed_temperature","value":{{"value":{right},"unit":"K"}}}}}},"adiabatic":{{"faces":["YMin","YMax"],"kind":"NoSlipWall","thermal":{{"kind":"adiabatic"}}}}}},"mesh":{{"cells_x":32,"cells_y":32,"cells_z":1}},"solver":{{"solver":"FromPhysics","end_time":{{"value":{end},"unit":"s"}},"write_interval":{{"value":{interval},"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"monitors":{{"centreline_u_sample_count":64}}}}"#
    )
}
