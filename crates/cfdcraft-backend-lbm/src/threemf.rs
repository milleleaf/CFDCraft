//! A closed, oriented prism surface as a 3MF package, the form in which the lattice solver takes
//! an obstacle.
//!
//! The package is the plain 3MF Core form with no face tags. The solver's format page
//! (`docs/formats/tagged-mesh.md` of its documentation) says untagged 3MF goes through the same
//! reader, one face per object, and that lengths are converted from the model's `unit`. The
//! bytes are deterministic: a fixed order of entries, stored without compression, one fixed time
//! and permission on every entry, and every number written by this module, never taken from text
//! of a study.

use std::{
    fmt::Write as _,
    io::{Cursor, Write},
};
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

/// The length unit of the model: metres, so a coordinate is the study's own number.
pub const UNIT: &str = "meter";

/// The members of the package, in the order they are written.
pub const MEMBERS: [&str; 3] = ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"];

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>\n";

const RELATIONSHIPS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>\n";

/// A triangle mesh. Every triangle is counter-clockwise seen from outside.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// Why a surface or a package could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreeMfError {
    /// The outline has fewer than three corners.
    TooFewCorners,
    /// A coordinate or the thickness is not a finite number, or the thickness is not above zero.
    NotFinite,
    /// The outline has no area, or a corner turns the wrong way or not at all, so it is not
    /// strictly convex and a fan of triangles would not cover it.
    NotStrictlyConvex,
    /// The object name is not a plain ASCII word.
    BadName,
    /// The archive could not be written.
    Archive(String),
}

/// The surface of the prism that stands on `outline` (corners in the x-y plane) and rises from
/// z = 0 to z = `thickness`. The outline may run either way round; the surface is oriented
/// outward whichever it is. The first `n` vertices are the bottom ring and the next `n` the top.
pub fn prism(outline: &[[f64; 2]], thickness: f64) -> Result<Mesh, ThreeMfError> {
    let n = outline.len();
    if n < 3 {
        return Err(ThreeMfError::TooFewCorners);
    }
    if !(thickness.is_finite() && thickness > 0.0)
        || outline.iter().flatten().any(|v| !v.is_finite())
    {
        return Err(ThreeMfError::NotFinite);
    }
    let twice_area: f64 = (0..n)
        .map(|i| {
            let (p, q) = (outline[i], outline[(i + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum();
    if twice_area == 0.0 {
        return Err(ThreeMfError::NotStrictlyConvex);
    }
    let mut ring = outline.to_vec();
    if twice_area < 0.0 {
        ring.reverse();
    }
    for i in 0..n {
        let (a, b, c) = (ring[i], ring[(i + 1) % n], ring[(i + 2) % n]);
        let turn = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if turn <= 0.0 {
            return Err(ThreeMfError::NotStrictlyConvex);
        }
    }
    let mut vertices: Vec<[f64; 3]> = ring.iter().map(|p| [p[0], p[1], 0.0]).collect();
    vertices.extend(ring.iter().map(|p| [p[0], p[1], thickness]));
    let n32 = u32::try_from(n).map_err(|_| ThreeMfError::NotFinite)?;
    let mut triangles = Vec::with_capacity(4 * n - 4);
    for k in 1..n32 - 1 {
        // Bottom cap, seen from below; top cap, seen from above.
        triangles.push([0, k + 1, k]);
        triangles.push([n32, n32 + k, n32 + k + 1]);
    }
    for k in 0..n32 {
        let next = (k + 1) % n32;
        triangles.push([k, next, n32 + next]);
        triangles.push([k, n32 + next, n32 + k]);
    }
    Ok(Mesh {
        vertices,
        triangles,
    })
}

fn model_xml(name: &str, mesh: &Mesh) -> Result<String, ThreeMfError> {
    let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !plain {
        return Err(ThreeMfError::BadName);
    }
    if mesh.vertices.iter().flatten().any(|v| !v.is_finite()) {
        return Err(ThreeMfError::NotFinite);
    }
    let mut xml = String::new();
    let _ = write!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"{UNIT}\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n <resources>\n  <object id=\"1\" type=\"model\" name=\"{name}\">\n   <mesh>\n    <vertices>\n"
    );
    for [x, y, z] in &mesh.vertices {
        // The shortest text that reads back as the same number.
        let _ = writeln!(xml, "     <vertex x=\"{x}\" y=\"{y}\" z=\"{z}\"/>");
    }
    xml.push_str("    </vertices>\n    <triangles>\n");
    for [a, b, c] in &mesh.triangles {
        let _ = writeln!(xml, "     <triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c}\"/>");
    }
    xml.push_str(
        "    </triangles>\n   </mesh>\n  </object>\n </resources>\n <build>\n  <item objectid=\"1\"/>\n </build>\n</model>\n",
    );
    Ok(xml)
}

/// The bytes of a 3MF package that holds `mesh` as one object called `name`.
pub fn write(name: &str, mesh: &Mesh) -> Result<Vec<u8>, ThreeMfError> {
    let model = model_xml(name, mesh)?;
    let fixed = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    let archive = |e: &dyn std::fmt::Display| ThreeMfError::Archive(e.to_string());
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (member, text) in MEMBERS
        .iter()
        .zip([CONTENT_TYPES, RELATIONSHIPS, model.as_str()])
    {
        zip.start_file(*member, fixed).map_err(|e| archive(&e))?;
        zip.write_all(text.as_bytes()).map_err(|e| archive(&e))?;
    }
    Ok(zip.finish().map_err(|e| archive(&e))?.into_inner())
}
