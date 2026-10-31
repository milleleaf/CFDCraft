use crate::{dictionary, scalar};
use cfdcraft_model::ResolvedStudy;

pub fn block_mesh(s: &ResolvedStudy) -> String {
    let x = scalar(s.domain.length_x);
    let y = scalar(s.domain.length_y);
    let z = scalar(s.domain.length_z);
    dictionary(
        "blockMeshDict",
        "dictionary",
        &format!(
            "convertToMeters 1;\nvertices\n(\n    (0 0 0)\n    ({x} 0 0)\n    ({x} {y} 0)\n    (0 {y} 0)\n    (0 0 {z})\n    ({x} 0 {z})\n    ({x} {y} {z})\n    (0 {y} {z})\n);\nblocks\n(\n    hex (0 1 2 3 4 5 6 7) ({} {} {}) simpleGrading (1 1 1)\n);\nedges ();\nboundary\n(\n    movingWall\n    {{\n        type wall;\n        faces ((3 7 6 2));\n    }}\n    fixedWalls\n    {{\n        type wall;\n        faces ((0 4 7 3) (1 2 6 5) (0 1 5 4));\n    }}\n    frontAndBack\n    {{\n        type empty;\n        faces ((0 3 2 1) (4 5 6 7));\n    }}\n);\nmergePatchPairs ();\n",
            s.mesh.cells_x, s.mesh.cells_y, s.mesh.cells_z
        ),
    )
}
