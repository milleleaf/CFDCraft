//! The surface of an obstacle: closed, oriented, of the volume of its outline, and written as the
//! same bytes every time.

mod support;

use cfdcraft_backend_lbm::{
    obstacles::circle_outline,
    threemf::{self, MEMBERS, Mesh, ThreeMfError, UNIT},
};
use std::collections::BTreeMap;
use support::{entries, model_text};

fn circle(radius: f64, spacing: f64) -> Vec<[f64; 2]> {
    circle_outline([0.2, 0.2], radius, spacing).unwrap()
}

/// Twice the area of a counter-clockwise polygon.
fn twice_area(outline: &[[f64; 2]]) -> f64 {
    (0..outline.len())
        .map(|i| {
            let (p, q) = (outline[i], outline[(i + 1) % outline.len()]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn minus(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn corners(mesh: &Mesh, triangle: [u32; 3]) -> [[f64; 3]; 3] {
    triangle.map(|i| mesh.vertices[i as usize])
}

/// Every directed edge of the surface, with how many triangles hold it.
fn directed_edges(mesh: &Mesh) -> BTreeMap<(u32, u32), usize> {
    let mut edges = BTreeMap::new();
    for [a, b, c] in &mesh.triangles {
        for edge in [(*a, *b), (*b, *c), (*c, *a)] {
            *edges.entry(edge).or_insert(0) += 1;
        }
    }
    edges
}

#[test]
fn a_circle_prism_is_a_closed_oriented_surface() {
    let outline = circle(0.05, 0.01);
    let thickness = 0.03;
    let mesh = threemf::prism(&outline, thickness).unwrap();

    // Closed and consistently oriented: every directed edge is held once, and so is its reverse.
    let edges = directed_edges(&mesh);
    for ((a, b), count) in &edges {
        assert_eq!(*count, 1, "the edge {a} to {b}");
        assert_eq!(edges.get(&(*b, *a)), Some(&1), "the edge {b} to {a}");
    }

    // Outward normals: the body is convex, so every normal points away from its centre.
    let centre = [0.2, 0.2, thickness / 2.0];
    let mut volume = 0.0;
    for triangle in &mesh.triangles {
        let [p, q, r] = corners(&mesh, *triangle);
        let normal = cross(minus(q, p), minus(r, p));
        let middle = [
            (p[0] + q[0] + r[0]) / 3.0,
            (p[1] + q[1] + r[1]) / 3.0,
            (p[2] + q[2] + r[2]) / 3.0,
        ];
        assert!(dot(normal, minus(middle, centre)) > 0.0, "{triangle:?}");
        volume += dot(p, cross(q, r)) / 6.0;
    }
    let expected = twice_area(&outline) / 2.0 * thickness;
    assert!(
        (volume - expected).abs() <= 1e-9 * expected,
        "{volume} against {expected}"
    );
}

#[test]
fn an_outline_that_runs_clockwise_gives_the_same_oriented_surface() {
    let mut outline = circle(0.05, 0.01);
    let counter = threemf::prism(&outline, 0.03).unwrap();
    outline.reverse();
    let clockwise = threemf::prism(&outline, 0.03).unwrap();
    let volume = |mesh: &Mesh| -> f64 {
        mesh.triangles
            .iter()
            .map(|t| {
                let [p, q, r] = corners(mesh, *t);
                dot(p, cross(q, r)) / 6.0
            })
            .sum()
    };
    assert!(volume(&clockwise) > 0.0);
    assert!((volume(&clockwise) - volume(&counter)).abs() <= 1e-9 * volume(&counter));
}

#[test]
fn an_outline_that_is_not_convex_or_not_finite_is_refused() {
    let dent = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [1.0, 0.5], [0.0, 2.0]];
    assert_eq!(
        threemf::prism(&dent, 1.0),
        Err(ThreeMfError::NotStrictlyConvex)
    );
    let line = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]];
    assert_eq!(
        threemf::prism(&line, 1.0),
        Err(ThreeMfError::NotStrictlyConvex)
    );
    assert_eq!(
        threemf::prism(&[[0.0, 0.0], [1.0, 0.0]], 1.0),
        Err(ThreeMfError::TooFewCorners)
    );
    let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    for thickness in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            threemf::prism(&square, thickness),
            Err(ThreeMfError::NotFinite)
        );
    }
    let mesh = threemf::prism(&square, 1.0).unwrap();
    assert_eq!(
        threemf::write("a b", &mesh),
        Err(ThreeMfError::BadName),
        "a name is a plain word"
    );
}

#[test]
fn the_polygon_follows_the_circle_within_a_quarter_cell() {
    let centre = [0.2, 0.2];
    for (radius, spacing) in [
        (0.05, 0.01),
        (0.05, 0.005),
        (0.05, 0.0025),
        (0.05, 0.02),
        (1.0, 0.01),
    ] {
        let outline = circle_outline(centre, radius, spacing).unwrap();
        let n = outline.len();
        assert!(n >= 4 && n.is_multiple_of(2), "{n} corners");
        let sagitta =
            |segments: usize| radius * (1.0 - (std::f64::consts::PI / segments as f64).cos());
        assert!(
            sagitta(n) <= spacing / 4.0,
            "{n} corners for {radius} and {spacing}"
        );
        if n > 4 {
            assert!(
                sagitta(n - 2) > spacing / 4.0,
                "{} corners would have done for {radius} and {spacing}",
                n - 2
            );
        }
        for corner in &outline {
            let distance =
                ((corner[0] - centre[0]).powi(2) + (corner[1] - centre[1]).powi(2)).sqrt();
            assert!((distance - radius).abs() <= 1e-12 * radius, "{corner:?}");
        }
        // The chord between neighbours stays within a quarter cell of the circle.
        for i in 0..n {
            let (p, q) = (outline[i], outline[(i + 1) % n]);
            let mid = [
                (p[0] + q[0]) / 2.0 - centre[0],
                (p[1] + q[1]) / 2.0 - centre[1],
            ];
            let gap = radius - (mid[0] * mid[0] + mid[1] * mid[1]).sqrt();
            assert!(gap <= spacing / 4.0 + 1e-12, "{gap}");
        }
    }
}

#[test]
fn the_3mf_bytes_are_deterministic() {
    let outline = circle(0.05, 0.01);
    let mesh = threemf::prism(&outline, 0.03).unwrap();
    let first = threemf::write("Cylinder", &mesh).unwrap();
    assert_eq!(first, threemf::write("Cylinder", &mesh).unwrap());

    let members = entries(&first);
    let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, MEMBERS);
    for member in &members {
        assert_eq!(member.method, 0, "{} is stored", member.name);
        // The earliest time an archive can hold: midnight on the first of January 1980.
        assert_eq!((member.time, member.date), (0, 0x21), "{}", member.name);
    }
    let model = model_text(&first);
    assert!(model.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(model.contains(&format!("unit=\"{UNIT}\"")));
    assert!(model.contains("name=\"Cylinder\""));
    assert_eq!(model.matches("<vertex ").count(), 2 * outline.len());
    assert_eq!(model.matches("<triangle ").count(), 4 * outline.len() - 4);
}

/// The value of an attribute in a tag of the model text.
fn attribute(tag: &str, name: &str) -> String {
    let key = format!(" {name}=\"");
    let from = tag.find(&key).unwrap() + key.len();
    let length = tag[from..].find('"').unwrap();
    tag[from..from + length].to_owned()
}

#[test]
fn the_model_reads_back() {
    let outline = circle(0.05, 0.01);
    let mesh = threemf::prism(&outline, 0.03).unwrap();
    let model = model_text(&threemf::write("Cylinder", &mesh).unwrap());
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for tag in model.lines().map(str::trim) {
        if tag.starts_with("<vertex ") {
            let number = |name: &str| attribute(tag, name).parse::<f64>().unwrap();
            vertices.push([number("x"), number("y"), number("z")]);
        } else if tag.starts_with("<triangle ") {
            let index = |name: &str| attribute(tag, name).parse::<u32>().unwrap();
            triangles.push([index("v1"), index("v2"), index("v3")]);
        }
    }
    assert_eq!(vertices, mesh.vertices);
    assert_eq!(triangles, mesh.triangles);
    assert!(
        triangles
            .iter()
            .flatten()
            .all(|i| (*i as usize) < vertices.len())
    );
}
