use crate::{case::Patch, dictionary, scalar};
use cfdcraft_model::ResolvedStudy;

pub(crate) fn block_mesh(s: &ResolvedStudy, patches: &[Patch]) -> String {
    let x = scalar(s.domain.length_x);
    let y = scalar(s.domain.length_y);
    let z = scalar(s.domain.length_z);
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
            "convertToMeters 1;\nvertices\n(\n    (0 0 0)\n    ({x} 0 0)\n    ({x} {y} 0)\n    (0 {y} 0)\n    (0 0 {z})\n    ({x} 0 {z})\n    ({x} {y} {z})\n    (0 {y} {z})\n);\nblocks\n(\n    hex (0 1 2 3 4 5 6 7) ({} {} {}) simpleGrading (1 1 1)\n);\nedges ();\nboundary\n(\n{boundary});\nmergePatchPairs ();\n",
            s.mesh.cells_x, s.mesh.cells_y, s.mesh.cells_z
        ),
    )
}
