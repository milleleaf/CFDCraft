//! The mesh text: the native format, one marker per boundary group, nothing dropped.

mod common;

use common::{cavity4, compiled, parse_mesh, text};

#[test]
fn a_4x4_cavity_mesh_is_written_in_the_native_format() {
    let tree = compiled(&cavity4());
    let source = text(&tree, "mesh.su2");
    let lines: Vec<&str> = source.lines().collect();
    assert_eq!(lines[0], "NDIME= 2");
    assert_eq!(lines[1], "NELEM= 16");
    for (i, line) in lines[2..18].iter().enumerate() {
        let words: Vec<&str> = line.split(' ').collect();
        assert_eq!(words.len(), 6, "{line}");
        assert_eq!(words[0], "9", "{line}");
        assert_eq!(words[5], i.to_string(), "{line}");
        for node in &words[1..5] {
            assert!(node.parse::<u32>().unwrap() < 25, "{line}");
        }
    }
    assert_eq!(lines[18], "NPOIN= 25");
    for (i, line) in lines[19..44].iter().enumerate() {
        let words: Vec<&str> = line.split(' ').collect();
        assert_eq!(words.len(), 3, "{line}");
        assert_eq!(words[2], i.to_string(), "{line}");
        for coordinate in &words[..2] {
            let (whole, fraction) = coordinate.split_once('.').unwrap();
            assert_eq!(fraction.len(), 12, "{line}");
            assert!(!whole.starts_with('-'), "{line}");
        }
    }
    assert_eq!(lines[44], "NMARK= 2");
    // The lid has four edges, the three walls twelve, each as a line element.
    assert_eq!(lines[45], "MARKER_TAG= movingWall");
    assert_eq!(lines[46], "MARKER_ELEMS= 4");
    assert_eq!(lines[51], "MARKER_TAG= fixedWalls");
    assert_eq!(lines[52], "MARKER_ELEMS= 12");
    for line in lines[47..51].iter().chain(&lines[53..65]) {
        let words: Vec<&str> = line.split(' ').collect();
        assert_eq!(words.len(), 3, "{line}");
        assert_eq!(words[0], "3", "{line}");
    }
    assert_eq!(lines.len(), 65);
}

#[test]
fn the_cells_cover_the_domain_and_the_lid_lies_on_the_top() {
    let tree = compiled(&cavity4());
    let mesh = parse_mesh(&text(&tree, "mesh.su2"));
    assert_eq!(mesh.cells.len(), 16);
    assert_eq!(mesh.nodes.len(), 25);
    let mut covered = 0.0;
    for i in 0..mesh.cells.len() {
        let area = mesh.area(i);
        assert!(area > 0.0, "cell {i} has area {area}");
        covered += area;
    }
    assert!((covered - 0.01).abs() < 1e-12, "{covered}");
    let (tag, edges) = &mesh.markers[0];
    assert_eq!(tag, "movingWall");
    for edge in edges {
        for node in edge {
            assert!((mesh.nodes[*node][1] - 0.1).abs() < 1e-12);
        }
    }
    // Every node belongs to a cell.
    let mut used = vec![false; mesh.nodes.len()];
    for cell in &mesh.cells {
        for node in cell {
            used[*node] = true;
        }
    }
    assert!(used.iter().all(|u| *u));
}

#[test]
fn the_mesh_text_does_not_depend_on_the_study_labels() {
    let tree = compiled(&cavity4());
    let mut relabelled = cavity4();
    let lid = relabelled.boundary_conditions.remove("lid").unwrap();
    relabelled
        .boundary_conditions
        .insert("top-plate".into(), lid);
    assert_eq!(compiled(&relabelled), tree);
}
