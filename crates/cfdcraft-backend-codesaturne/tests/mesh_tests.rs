use cfdcraft_backend_codesaturne::mesh::gmsh_text;
use cfdcraft_model::{ResolvedStudy, Study, resolve};
use std::collections::BTreeMap;

const CAVITY_V1: &str = include_str!("../../cfdcraft-model/tests/fixtures/cavity_v1.json");

fn cavity(cells: u64) -> ResolvedStudy {
    let mut study = resolve(&Study::from_json(CAVITY_V1).unwrap()).unwrap();
    study.mesh.cells_x = cells;
    study.mesh.cells_y = cells;
    study
}

struct Msh {
    names: BTreeMap<u32, (u32, String)>,
    nodes: Vec<[f64; 3]>,
    /// (type, physical group, node numbers from 1)
    elements: Vec<(u32, u32, Vec<usize>)>,
}

/// A reader of the Gmsh 2.2 text the writer produces, written for this test only.
fn read(text: &str) -> Msh {
    let mut lines = text.lines();
    let mut msh = Msh {
        names: BTreeMap::new(),
        nodes: Vec::new(),
        elements: Vec::new(),
    };
    while let Some(line) = lines.next() {
        match line {
            "$PhysicalNames" => {
                let count: usize = lines.next().unwrap().parse().unwrap();
                for _ in 0..count {
                    let mut parts = lines.next().unwrap().splitn(3, ' ');
                    let dim: u32 = parts.next().unwrap().parse().unwrap();
                    let id: u32 = parts.next().unwrap().parse().unwrap();
                    let name = parts.next().unwrap().trim_matches('"').to_owned();
                    msh.names.insert(id, (dim, name));
                }
            }
            "$Nodes" => {
                let count: usize = lines.next().unwrap().parse().unwrap();
                for _ in 0..count {
                    let parts: Vec<f64> = lines
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .map(|p| p.parse().unwrap())
                        .collect();
                    msh.nodes.push([parts[1], parts[2], parts[3]]);
                }
            }
            "$Elements" => {
                let count: usize = lines.next().unwrap().parse().unwrap();
                for _ in 0..count {
                    let parts: Vec<usize> = lines
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .map(|p| p.parse().unwrap())
                        .collect();
                    let tags = parts[2];
                    msh.elements.push((
                        u32::try_from(parts[1]).unwrap(),
                        u32::try_from(parts[3]).unwrap(),
                        parts[3 + tags..].to_vec(),
                    ));
                }
            }
            _ => {}
        }
    }
    msh
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[test]
fn a_2x2_cavity_extrudes_to_a_gmsh_mesh_one_cell_thick() {
    let study = cavity(2);
    let text = gmsh_text(&study).unwrap();
    assert!(
        text.starts_with("$MeshFormat\n2.2 0 8\n$EndMeshFormat\n"),
        "{text}"
    );
    let msh = read(&text);

    let names: Vec<(&str, u32)> = msh
        .names
        .values()
        .map(|(dim, name)| (name.as_str(), *dim))
        .collect();
    for wanted in ["x_min", "x_max", "y_min", "y_max", "z_symmetry"] {
        assert!(
            names.contains(&(wanted, 2)),
            "{wanted} is missing: {names:?}"
        );
    }
    assert!(names.contains(&("fluid", 3)), "{names:?}");

    assert_eq!(msh.nodes.len(), 18, "nine nodes per layer");
    let layer_z: Vec<f64> = msh.nodes.iter().map(|n| n[2]).collect();
    let thickness = study.domain.length_z;
    assert!(layer_z[..9].iter().all(|z| z.abs() <= 1e-15));
    assert!(layer_z[9..].iter().all(|z| (z - thickness).abs() <= 1e-12));

    let hexes: Vec<_> = msh.elements.iter().filter(|e| e.0 == 5).collect();
    assert_eq!(hexes.len(), 4);
    for (_, _, nodes) in &hexes {
        assert_eq!(nodes.len(), 8);
        assert!(nodes.iter().all(|n| (1..=18).contains(n)), "{nodes:?}");
        let p = |i: usize| msh.nodes[nodes[i] - 1];
        let volume = dot(sub(p(1), p(0)), cross(sub(p(3), p(0)), sub(p(4), p(0))));
        assert!(volume > 0.0, "{nodes:?} has volume {volume}");
    }

    let quads: Vec<_> = msh.elements.iter().filter(|e| e.0 == 3).collect();
    let in_group = |name: &str| {
        let id = msh.names.iter().find(|(_, (_, n))| n == name).unwrap().0;
        quads.iter().filter(|e| e.1 == *id).count()
    };
    for side in ["x_min", "x_max", "y_min", "y_max"] {
        assert_eq!(in_group(side), 2, "{side}");
    }
    assert_eq!(in_group("z_symmetry"), 8);
    let groups: Vec<u32> = quads.iter().map(|e| e.1).collect();
    assert!(
        groups.windows(2).all(|w| w[0] <= w[1]),
        "grouped: {groups:?}"
    );

    // Every boundary quad has its outward normal pointing away from the middle of the box.
    let middle = [
        study.domain.length_x / 2.0,
        study.domain.length_y / 2.0,
        study.domain.length_z / 2.0,
    ];
    for (_, group, nodes) in &quads {
        let p = |i: usize| msh.nodes[nodes[i] - 1];
        let normal = cross(sub(p(1), p(0)), sub(p(3), p(0)));
        let centre = [0, 1, 2].map(|axis| (0..4).map(|i| p(i)[axis]).sum::<f64>() / 4.0);
        assert!(
            dot(normal, sub(centre, middle)) > 0.0,
            "group {group}: {nodes:?} faces inward"
        );
    }
}

#[test]
fn a_mesh_with_more_than_one_cell_through_z_is_refused() {
    let mut study = cavity(2);
    study.mesh.cells_z = 2;
    assert!(gmsh_text(&study).is_err());
}
