//! The mesh dictionary and the patch table are written from the shape layout. These tests build
//! studies for the box, the backward step and the cylinder channel, compile them and read the
//! written text back.

use cfdcraft_backend_esi::{
    CaseTree, CompileError, Dialect, compile,
    guard::scan_tree,
    patches::{layout_inputs, layout_of, patch_of_label},
};
use cfdcraft_model::{Face, ResolvedStudy, Study, resolve};
use cfdcraft_shapes::{Side, problems};
use std::path::Path;

/// A study as JSON text. `shape` is either empty or starts with a comma and holds the `shape`
/// member of the domain; `mesh_extra` is empty or starts with a comma and holds more mesh members.
struct Text<'a> {
    domain: [f64; 3],
    open: bool,
    shape: &'a str,
    boundaries: &'a str,
    cells: [u64; 2],
    mesh_extra: &'a str,
    nu: f64,
}

impl Text<'_> {
    fn json(&self) -> String {
        let [x, y, z] = self.domain;
        let [cx, cy] = self.cells;
        let (open, shape, boundaries, mesh_extra, nu) = (
            self.open,
            self.shape,
            self.boundaries,
            self.mesh_extra,
            self.nu,
        );
        format!(
            r#"{{"schema_version":1,"domain":{{"length_x":{{"value":{x},"unit":"m"}},"length_y":{{"value":{y},"unit":"m"}},"length_z":{{"value":{z},"unit":"m"}},"open":{open}{shape}}},"fluid":{{"kinematic_viscosity":{{"value":{nu},"unit":"m2/s"}}}},"boundary_conditions":{{{boundaries}}},"mesh":{{"cells_x":{cx},"cells_y":{cy},"cells_z":1{mesh_extra}}},"solver":{{"solver":"SimpleLaminar","end_time":{{"value":100,"unit":"s"}},"write_interval":{{"value":100,"unit":"s"}},"convergence":{{"p":1e-06,"U":1e-06}}}},"monitors":{{"centreline_u_sample_count":64}}}}"#
        )
    }
}

/// The lid-driven box with a given cell count along both axes.
fn box_json(cells: u64, mesh_extra: &str) -> String {
    Text {
        domain: [0.1, 0.1, 0.05],
        open: false,
        shape: "",
        boundaries: r#""lid":{"faces":["YMax"],"kind":"MovingWall","velocity":{"value":0.1,"unit":"m/s"}},"walls":{"faces":["XMin","XMax","YMin"],"kind":"NoSlipWall"}"#,
        cells: [cells, cells],
        mesh_extra,
        nu: 0.0001,
    }
    .json()
}

fn graded_json(level: u32) -> String {
    box_json(
        40,
        &format!(r#","level":{level},"grading":{{"x":1.05,"y":1.05}}"#),
    )
}

/// The backward step, with or without the start-up section, whose floor boundary has this label.
fn step_json(startup: bool, level: u32, floor_label: &str) -> String {
    let (startup_length, cells_x) = if startup { (0.254, 220) } else { (0.0, 218) };
    let shape = format!(
        r#","shape":{{"kind":"backward_step","step_height":{{"value":12.7,"unit":"mm"}},"upstream_length":{{"value":1.651,"unit":"m"}},"startup_length":{{"value":{startup_length},"unit":"m"}}}}"#
    );
    let boundaries = format!(
        r#""inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{{"value":44.2,"unit":"m/s"}}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"{floor_label}":{{"faces":["YMin"],"kind":"NoSlipWall"}},"top_wall":{{"faces":["YMax"],"kind":"NoSlipWall"}}"#
    );
    Text {
        domain: [2.286, 0.1143, 0.0127],
        open: true,
        shape: &shape,
        boundaries: &boundaries,
        cells: [cells_x, 114],
        mesh_extra: &format!(r#","level":{level}"#),
        nu: 0.0000156,
    }
    .json()
}

/// The cylinder channel, whose cylinder boundary has this label.
fn cylinder_json(level: u32, obstacle_label: &str) -> String {
    let shape = r#","shape":{"kind":"cylinder_channel","diameter":{"value":0.1,"unit":"m"},"centre_x":{"value":0.2,"unit":"m"},"centre_y":{"value":0.2,"unit":"m"}}"#;
    let boundaries = format!(
        r#""inlet":{{"faces":["XMin"],"kind":"VelocityInlet","velocity":{{"value":1.0,"unit":"m/s"}}}},"outlet":{{"faces":["XMax"],"kind":"PressureOutlet"}},"channel_walls":{{"faces":["YMin","YMax"],"kind":"NoSlipWall"}},"{obstacle_label}":{{"faces":["Obstacle"],"kind":"NoSlipWall"}}"#
    );
    Text {
        domain: [2.2, 0.41, 0.1],
        open: true,
        shape,
        boundaries: &boundaries,
        cells: [114, 44],
        mesh_extra: &format!(r#","level":{level}"#),
        nu: 0.001,
    }
    .json()
}

fn resolved(json: &str) -> ResolvedStudy {
    resolve(&Study::from_json(json).unwrap()).unwrap()
}

fn tree(s: &ResolvedStudy) -> CaseTree {
    compile(s, Dialect::EsiV2512).unwrap()
}

fn file(tree: &CaseTree, name: &str) -> String {
    let f = tree
        .files
        .iter()
        .find(|f| f.rel_path == Path::new(name))
        .unwrap();
    String::from_utf8(f.bytes.clone()).unwrap()
}

fn mesh_dict(s: &ResolvedStudy) -> String {
    file(&tree(s), "system/blockMeshDict")
}

/// The `hex` lines of a dictionary.
fn hex_lines(dict: &str) -> Vec<&str> {
    dict.lines()
        .filter(|l| l.trim_start().starts_with("hex "))
        .collect()
}

/// The cell counts of each block.
fn block_cells(dict: &str) -> Vec<[u64; 3]> {
    hex_lines(dict)
        .into_iter()
        .map(|l| {
            let counts = l.split('(').nth(2).unwrap().split(')').next().unwrap();
            let n: Vec<u64> = counts
                .split_whitespace()
                .map(|t| t.parse().unwrap())
                .collect();
            [n[0], n[1], n[2]]
        })
        .collect()
}

/// (name, mesh type, face list) of each patch in the boundary section, in written order.
fn patches_of(dict: &str) -> Vec<(String, String, String)> {
    let section = dict.split("\nboundary\n").nth(1).unwrap();
    let mut out: Vec<(String, String, String)> = Vec::new();
    for line in section.lines() {
        let text = line.trim();
        if line.starts_with("    ") && line.chars().nth(4).is_some_and(char::is_alphabetic) {
            out.push((text.to_owned(), String::new(), String::new()));
        } else if let Some(t) = text.strip_prefix("type ")
            && let Some(last) = out.last_mut()
        {
            last.1 = t.trim_end_matches(';').to_owned();
        } else if let Some(f) = text.strip_prefix("faces ")
            && let Some(last) = out.last_mut()
        {
            last.2 = f.trim_end_matches(';').to_owned();
        }
    }
    out
}

/// The numbers of a parenthesised text.
fn numbers(text: &str) -> Vec<f64> {
    text.replace(['(', ')'], " ")
        .split_whitespace()
        .map(|t| t.parse().unwrap())
        .collect()
}

#[test]
fn patch_names_come_from_the_fixed_table() {
    let s = resolved(&box_json(128, ""));
    let patches = patches_of(&mesh_dict(&s));
    let names: Vec<&str> = patches.iter().map(|p| p.0.as_str()).collect();
    assert_eq!(names, ["movingWall", "fixedWalls", "frontAndBack"]);
    assert_eq!(patches[0].2, "((3 7 6 2))");
    assert_eq!(patches[1].2, "((0 4 7 3) (1 2 6 5) (0 1 5 4))");
    assert_eq!(patches[2].2, "((0 3 2 1) (4 5 6 7))");
    assert_eq!(patch_of_label(&s, "lid"), Some("movingWall"));
    assert_eq!(patch_of_label(&s, "walls"), Some("fixedWalls"));
    assert_eq!(patch_of_label(&s, "no_such_label"), None);
}

#[test]
fn a_graded_box_writes_sections() {
    let s = resolved(&graded_json(0));
    let dict = mesh_dict(&s);
    let blocks = hex_lines(&dict);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].contains(") (40 40 1) simpleGrading (((0.5 0.5 "));
    assert!(blocks[0].ends_with(")) 1)"));
    assert!(dict.contains("\nedges ();\n"));
    let grading = blocks[0].split("simpleGrading").nth(1).unwrap();
    let e = 1.05f64.powf(19.0);
    let section = [0.5, 0.5, e, 0.5, 0.5, 1.0 / e];
    let want: Vec<f64> = section
        .iter()
        .chain(&section)
        .chain(&[1.0])
        .copied()
        .collect();
    let got = numbers(grading);
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert!((g - w).abs() <= 1e-12 * w.abs(), "{g} against {w}");
    }
}

#[test]
fn the_layout_is_the_one_the_dictionary_lists() {
    let s = resolved(&graded_json(1));
    let layout = layout_of(&s).unwrap();
    let (_, _, mesh) = layout_inputs(&s);
    assert_eq!(mesh.level, 1);
    assert_eq!(layout.blocks.len(), hex_lines(&mesh_dict(&s)).len());
}

/// The line a field file holds for a patch.
fn field_entry(tree: &CaseTree, name: &str, patch: &str) -> String {
    let start = format!("    {patch} {{");
    file(tree, name)
        .lines()
        .find(|l| l.starts_with(&start))
        .unwrap_or_else(|| panic!("{name} has no entry for {patch}"))
        .to_owned()
}

fn names(dict: &str) -> Vec<String> {
    patches_of(dict).into_iter().map(|p| p.0).collect()
}

/// The number of block faces in a face list text such as `((0 1 2 3) (4 5 6 7))`.
fn face_count(faces: &str) -> usize {
    faces.matches('(').count() - 1
}

#[test]
fn the_step_mesh_has_four_blocks_and_a_startup_patch() {
    let s = resolved(&step_json(true, 0, "floor_walls"));
    let t = tree(&s);
    let dict = file(&t, "system/blockMeshDict");
    assert_eq!(hex_lines(&dict).len(), 4);
    assert!(!dict.contains("arc "));
    assert!(dict.contains("\nedges ();\n"));
    let layout = layout_of(&s).unwrap();
    let vertex_lines = dict
        .lines()
        .filter(|l| l.starts_with("    (") && !l.contains("hex"))
        .count();
    assert_eq!(vertex_lines, layout.vertices.len());
    assert_eq!(
        names(&dict),
        ["inlet", "outlet", "fixedWalls", "startup", "frontAndBack"]
    );
    let patches = patches_of(&dict);
    // The start-up section lies on the floor and the roof of the channel, so it is not one plane:
    // the mesh builder on the compute host refused `symmetryPlane` for it ("Symmetry plane
    // 'startup' is not planar") and accepts the general `symmetry` type.
    assert_eq!(patches[3].1, "symmetry");
    for (name, _, _) in &patches {
        field_entry(&t, "0/U", name);
        field_entry(&t, "0/p", name);
    }
    assert!(field_entry(&t, "0/U", "startup").contains("type symmetry;"));
    assert!(field_entry(&t, "0/p", "startup").contains("type symmetry;"));
    // Without the start-up section there is no such patch.
    let plain = mesh_dict(&resolved(&step_json(false, 0, "floor_walls")));
    assert_eq!(
        names(&plain),
        ["inlet", "outlet", "fixedWalls", "frontAndBack"]
    );
}

#[test]
fn the_cylinder_mesh_has_arcs_and_an_obstacle_patch() {
    let s = resolved(&cylinder_json(0, "cylinder"));
    let t = tree(&s);
    let dict = file(&t, "system/blockMeshDict");
    assert_eq!(hex_lines(&dict).len(), 12);
    assert_eq!(
        dict.lines()
            .filter(|l| l.trim_start().starts_with("arc "))
            .count(),
        8
    );
    assert_eq!(
        names(&dict),
        ["inlet", "outlet", "fixedWalls", "obstacle", "frontAndBack"]
    );
    let patches = patches_of(&dict);
    assert_eq!(patches[3].1, "wall");
    // A patch face is a block face, not a cell: one inner face per ring block, at every level.
    let layout = layout_of(&s).unwrap();
    let wanted = layout
        .quads
        .iter()
        .filter(|q| q.side == Side::Obstacle)
        .count();
    assert_eq!(wanted, 4);
    for level in 0..=2 {
        let dict = mesh_dict(&resolved(&cylinder_json(level, "cylinder")));
        assert_eq!(face_count(&patches_of(&dict)[3].2), wanted, "level {level}");
    }
    assert!(field_entry(&t, "0/U", "obstacle").contains("type noSlip;"));
    assert_eq!(patch_of_label(&s, "cylinder"), Some("obstacle"));
    assert_eq!(patch_of_label(&s, "channel_walls"), Some("fixedWalls"));
}

#[test]
fn every_emitted_layout_is_sound() {
    for level in 0..=2 {
        let studies = [
            ("step with start-up", step_json(true, level, "floor_walls")),
            ("step", step_json(false, level, "floor_walls")),
            ("cylinder", cylinder_json(level, "cylinder")),
            ("graded box", graded_json(level)),
        ];
        for (what, json) in studies {
            let s = resolved(&json);
            let (domain, shape, _) = layout_inputs(&s);
            let layout = layout_of(&s).unwrap();
            let found = problems(&layout, &domain, &shape);
            assert!(found.is_empty(), "{what} at level {level}: {found:?}");
            let total: u64 = block_cells(&mesh_dict(&s))
                .iter()
                .map(|c| c.iter().product::<u64>())
                .sum();
            assert_eq!(total, layout.cell_count, "{what} at level {level}");
        }
        let s = resolved(&graded_json(level));
        let cells = s.mesh.cells_x * s.mesh.cells_y * s.mesh.cells_z;
        assert_eq!(layout_of(&s).unwrap().cell_count, cells);
        assert_eq!(s.mesh.cells_x, 40 << level);
    }
}

#[test]
fn refinement_is_visible_in_the_dictionary() {
    let coarse = block_cells(&mesh_dict(&resolved(&step_json(true, 0, "floor_walls"))));
    let fine = block_cells(&mesh_dict(&resolved(&step_json(true, 1, "floor_walls"))));
    assert_eq!(coarse.len(), fine.len());
    for (c, f) in coarse.iter().zip(&fine) {
        assert_eq!([c[0] * 2, c[1] * 2, c[2]], *f);
    }
}

#[test]
fn the_guard_passes_every_mesh_case() {
    for json in [
        step_json(true, 0, "floor_walls"),
        step_json(false, 1, "floor_walls"),
        cylinder_json(0, "cylinder"),
        cylinder_json(2, "cylinder"),
        graded_json(0),
    ] {
        assert!(scan_tree(&tree(&resolved(&json))).is_empty());
    }
}

#[test]
fn no_author_text_reaches_the_dictionary() {
    let hostile = "secret_label_xyz";
    for json in [
        step_json(true, 0, hostile),
        step_json(false, 1, hostile),
        cylinder_json(0, hostile),
    ] {
        let s = resolved(&json);
        assert!(s.boundary_conditions.contains_key(hostile));
        for f in &tree(&s).files {
            let text = String::from_utf8_lossy(&f.bytes);
            assert!(!text.contains(hostile), "{}", f.rel_path.display());
        }
    }
}

#[test]
fn the_obstacle_without_a_cylinder_is_refused() {
    let invalid =
        |s: &ResolvedStudy| matches!(compile(s, Dialect::EsiV2512), Err(CompileError::Invalid(_)));
    let mut boxed = resolved(&box_json(128, ""));
    boxed
        .boundary_conditions
        .get_mut("walls")
        .unwrap()
        .faces
        .push(Face::Obstacle);
    assert!(invalid(&boxed));
    let mut stepped = resolved(&step_json(true, 0, "floor_walls"));
    stepped
        .boundary_conditions
        .get_mut("floor_walls")
        .unwrap()
        .faces
        .push(Face::Obstacle);
    assert!(invalid(&stepped));
    let mut cylinder = resolved(&cylinder_json(0, "cylinder"));
    assert!(!invalid(&cylinder));
    cylinder.boundary_conditions.remove("cylinder").unwrap();
    assert!(invalid(&cylinder));
}

/// Compares written text with a golden file; a difference gives the whole actual text so it can
/// be pasted into the golden file as it is.
fn golden_difference(name: &str, actual: &str) -> Option<String> {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
        .join("system/blockMeshDict");
    let want = std::fs::read_to_string(&golden).unwrap();
    (actual != want)
        .then(|| format!("{name} differs from its golden file; the actual text is:\n{actual}"))
}

#[test]
fn the_three_mesh_dictionaries_equal_their_golden_files() {
    let differences: Vec<String> = [
        golden_difference(
            "step-mesh-v2512",
            &mesh_dict(&resolved(&step_json(true, 0, "floor_walls"))),
        ),
        golden_difference(
            "cylinder-mesh-v2512",
            &mesh_dict(&resolved(&cylinder_json(0, "cylinder"))),
        ),
        golden_difference("graded-box-v2512", &mesh_dict(&resolved(&graded_json(0)))),
    ]
    .into_iter()
    .flatten()
    .collect();
    assert!(differences.is_empty(), "{}", differences.join("\n\n"));
}
