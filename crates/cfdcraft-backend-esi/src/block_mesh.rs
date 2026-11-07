use crate::{case::Patch, dictionary, scalar, transport::component};
use cfdcraft_shapes::{Block, Layout, Segment};

/// One axis grading: `1` for the single even section, the multi-grading sections otherwise.
fn axis(segments: &[Segment]) -> String {
    if let [s] = segments
        && s.length_fraction == 1.0
        && s.cell_fraction == 1.0
        && s.expansion == 1.0
    {
        return "1".into();
    }
    let sections: Vec<String> = segments
        .iter()
        .map(|s| {
            format!(
                "({} {} {})",
                scalar(s.length_fraction),
                scalar(s.cell_fraction),
                scalar(s.expansion)
            )
        })
        .collect();
    format!("({})", sections.join(" "))
}

fn block(b: &Block) -> String {
    let vertices: Vec<String> = b.vertices.iter().map(|v| v.to_string()).collect();
    format!(
        "    hex ({}) ({} {} {}) simpleGrading ({} {} {})\n",
        vertices.join(" "),
        b.cells[0],
        b.cells[1],
        b.cells[2],
        axis(&b.grading[0]),
        axis(&b.grading[1]),
        axis(&b.grading[2])
    )
}

pub fn block_mesh(layout: &Layout, patches: &[Patch]) -> String {
    let vertices: String = layout
        .vertices
        .iter()
        .map(|v| {
            format!(
                "    ({} {} {})\n",
                component(v[0]),
                component(v[1]),
                component(v[2])
            )
        })
        .collect();
    let blocks: String = layout.blocks.iter().map(block).collect();
    let edges = if layout.arcs.is_empty() {
        "edges ();\n".to_owned()
    } else {
        let arcs: String = layout
            .arcs
            .iter()
            .map(|a| {
                format!(
                    "    arc {} {} ({} {} {})\n",
                    a.from,
                    a.to,
                    component(a.through[0]),
                    component(a.through[1]),
                    component(a.through[2])
                )
            })
            .collect();
        format!("edges\n(\n{arcs});\n")
    };
    let boundary: String = patches
        .iter()
        .map(|p| {
            format!(
                "    {}\n    {{\n        type {};\n        faces ({});\n    }}\n",
                p.name,
                p.mesh_type(),
                p.faces_text()
            )
        })
        .collect();
    dictionary(
        "blockMeshDict",
        "dictionary",
        &format!(
            "convertToMeters 1;\nvertices\n(\n{vertices});\nblocks\n(\n{blocks});\n{edges}boundary\n(\n{boundary});\nmergePatchPairs ();\n"
        ),
    )
}
