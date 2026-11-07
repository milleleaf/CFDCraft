//! The recipes behind the two stage keys, built from a resolved study.

use std::collections::BTreeMap;

use cfdcraft_model::{BoundaryKind, Face, RawOverride, ResolvedStudy};
use cfdcraft_store::ContentHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{StageKey, ToolIdentity};

/// The canonical text of a recipe, written for hashing only.
fn canonical<T: Serialize>(value: &T) -> String {
    serde_jcs::to_string(value).expect("a recipe holds finite SI numbers and plain text only")
}

fn key_of<T: Serialize>(value: &T) -> StageKey {
    StageKey::of_text(&canonical(value))
}

/// The size and openness of the domain, in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeometryRecipe {
    pub length_x: f64,
    pub length_y: f64,
    pub length_z: f64,
    pub open: bool,
}

impl GeometryRecipe {
    /// The hash of the canonical text of the geometry.
    pub fn hash(&self) -> ContentHash {
        ContentHash::of(canonical(self).as_bytes())
    }
}

/// The patch layout of one boundary: what it is and where, never what the user called it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PatchRecipe {
    pub kind: BoundaryKind,
    pub faces: Vec<Face>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CellsRecipe {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

/// What made a mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshRecipe {
    pub tool: ToolIdentity,
    pub geometry: GeometryRecipe,
    /// Sorted by their JSON text, so the order never depends on boundary labels.
    pub patches: Vec<PatchRecipe>,
    pub cells: CellsRecipe,
    /// The raw overrides that touch a mesh dictionary, without their labels, sorted by file,
    /// entry, value and backend. Absent when there are none, so a study without them keeps its
    /// mesh key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<RawOverride>,
}

impl MeshRecipe {
    pub fn key(&self) -> StageKey {
        key_of(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FluidRecipe {
    pub kinematic_viscosity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoundaryRecipe {
    pub kind: BoundaryKind,
    pub faces: Vec<Face>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SolverRecipe {
    pub end_time: f64,
    pub write_interval: f64,
    pub convergence_p: f64,
    pub convergence_u: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MonitorsRecipe {
    pub centreline_u_sample_count: u64,
}

/// What made a result: the mesh by key, and everything the solver was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResultRecipe {
    pub tool: ToolIdentity,
    pub mesh_key: StageKey,
    pub fluid: FluidRecipe,
    /// By the user's label, so a rename changes this recipe and never the mesh recipe.
    pub boundaries: BTreeMap<String, BoundaryRecipe>,
    pub solver: SolverRecipe,
    pub monitors: MonitorsRecipe,
    /// Every raw override by the user's label, so a rename changes this recipe and never the
    /// mesh recipe. Absent when there are none, so a study without them keeps its result key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raw: BTreeMap<String, RawOverride>,
}

impl ResultRecipe {
    pub fn key(&self) -> StageKey {
        key_of(self)
    }
}

/// Both recipes of one study.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StageRecipes {
    pub mesh: MeshRecipe,
    pub result: ResultRecipe,
}

/// Both keys of one study.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageKeys {
    pub mesh: StageKey,
    pub result: StageKey,
}

impl StageRecipes {
    /// The recipes of a resolved study. The study's own schema number is not read: it is part
    /// of the study hash already and never differs between studies that resolve.
    pub fn from_resolved(resolved: &ResolvedStudy, tool: &ToolIdentity) -> Self {
        let mut patches: Vec<PatchRecipe> = resolved
            .boundary_conditions
            .values()
            .map(|b| PatchRecipe {
                kind: b.kind,
                faces: b.faces.clone(),
            })
            .collect();
        // A patch is plain enums, so the text cannot fail; an empty text would still be stable.
        patches.sort_by_cached_key(|p| serde_json::to_string(p).unwrap_or_default());
        let mut mesh_raw: Vec<RawOverride> = resolved
            .raw_overrides
            .values()
            .filter(|r| r.affects_mesh())
            .cloned()
            .collect();
        mesh_raw.sort_by(|a, b| {
            (&a.file, &a.entry, &a.value, a.backend).cmp(&(&b.file, &b.entry, &b.value, b.backend))
        });
        let mesh = MeshRecipe {
            tool: tool.clone(),
            geometry: GeometryRecipe {
                length_x: resolved.domain.length_x,
                length_y: resolved.domain.length_y,
                length_z: resolved.domain.length_z,
                open: resolved.domain.open,
            },
            patches,
            cells: CellsRecipe {
                x: resolved.mesh.cells_x,
                y: resolved.mesh.cells_y,
                z: resolved.mesh.cells_z,
            },
            raw: mesh_raw,
        };
        let result = ResultRecipe {
            tool: tool.clone(),
            mesh_key: mesh.key(),
            fluid: FluidRecipe {
                kinematic_viscosity: resolved.fluid.kinematic_viscosity,
            },
            boundaries: resolved
                .boundary_conditions
                .iter()
                .map(|(label, b)| {
                    let boundary = BoundaryRecipe {
                        kind: b.kind,
                        faces: b.faces.clone(),
                        velocity: b.velocity,
                    };
                    (label.clone(), boundary)
                })
                .collect(),
            solver: SolverRecipe {
                end_time: resolved.solver.end_time,
                write_interval: resolved.solver.write_interval,
                convergence_p: resolved.solver.convergence.p,
                convergence_u: resolved.solver.convergence.u,
            },
            monitors: MonitorsRecipe {
                centreline_u_sample_count: resolved.monitors.centreline_u_sample_count,
            },
            raw: resolved.raw_overrides.clone(),
        };
        Self { mesh, result }
    }

    pub fn keys(&self) -> StageKeys {
        StageKeys {
            mesh: self.mesh.key(),
            result: self.result.key(),
        }
    }
}
