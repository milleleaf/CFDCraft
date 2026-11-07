//! The recipes behind the two stage keys, built from a resolved study.

use std::collections::BTreeMap;

use cfdcraft_model::{
    BoundaryKind, Face, InitMode, InletProfile, MonitorField, RawOverride, ResolvedPhysics,
    ResolvedShape, ResolvedStudy, ResolvedThermal, ResolvedTime, SurfaceQuantity, TurbulenceModel,
};
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
    /// For a wall held at a fixed temperature: the place of that temperature among the distinct
    /// fixed temperatures of the study, counted from 1 in ascending order. The mesh gives such
    /// walls their own patches, so the place matters and the temperature itself does not. Absent
    /// for a wall without a fixed temperature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CellsRecipe {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

/// Wall grading ratios, an axis present only when it is graded.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GradingRecipe {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

/// A domain shape other than the plain box, in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShapeRecipe {
    BackwardStep {
        step_height: f64,
        upstream_length: f64,
        startup_length: f64,
    },
    CylinderChannel {
        diameter: f64,
        centre_x: f64,
        centre_y: f64,
    },
}

impl From<ResolvedShape> for ShapeRecipe {
    fn from(shape: ResolvedShape) -> Self {
        match shape {
            ResolvedShape::BackwardStep {
                step_height,
                upstream_length,
                startup_length,
            } => Self::BackwardStep {
                step_height,
                upstream_length,
                startup_length,
            },
            ResolvedShape::CylinderChannel {
                diameter,
                centre_x,
                centre_y,
            } => Self::CylinderChannel {
                diameter,
                centre_x,
                centre_y,
            },
        }
    }
}

fn is_zero(level: &u32) -> bool {
    *level == 0
}

/// What made a mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeshRecipe {
    pub tool: ToolIdentity,
    pub geometry: GeometryRecipe,
    /// Sorted by their JSON text, so the order never depends on boundary labels.
    pub patches: Vec<PatchRecipe>,
    /// The effective cell counts, the refinement level already applied.
    pub cells: CellsRecipe,
    /// The refinement level the counts were refined by. Absent at level 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub level: u32,
    /// Absent when no axis is graded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grading: Option<GradingRecipe>,
    /// Absent for a box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<ShapeRecipe>,
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
    /// The inlet profile, when the study states one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<InletProfile>,
    /// The thermal condition of a wall, in kelvin, when the study states one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thermal: Option<ThermalRecipe>,
}

/// The temperature condition of a wall.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThermalRecipe {
    FixedTemperature { value: f64 },
    Adiabatic,
}

impl From<ResolvedThermal> for ThermalRecipe {
    fn from(thermal: ResolvedThermal) -> Self {
        match thermal {
            ResolvedThermal::FixedTemperature { value } => Self::FixedTemperature { value },
            ResolvedThermal::Adiabatic => Self::Adiabatic,
        }
    }
}

/// Steady, or transient with the time step of mesh level 0 in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TimeRecipe {
    Steady,
    Transient { delta_t: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TurbulenceRecipe {
    pub model: TurbulenceModel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viscosity_ratio: Option<f64>,
    /// In m/s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_velocity: Option<f64>,
}

/// Boussinesq buoyancy in SI.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BuoyancyRecipe {
    pub gravity: f64,
    pub beta: f64,
    pub reference_temperature: f64,
    pub prandtl: f64,
    pub turbulent_prandtl: f64,
}

/// The physics the solver was given. Absent when the study is steady, laminar, without buoyancy
/// and started from a uniform field, so such a study keeps its result key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PhysicsRecipe {
    pub time: TimeRecipe,
    pub turbulence: TurbulenceRecipe,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buoyancy: Option<BuoyancyRecipe>,
    pub init: InitMode,
}

impl Default for PhysicsRecipe {
    /// The plain steady laminar run, which a study without physics stands for.
    fn default() -> Self {
        Self {
            time: TimeRecipe::Steady,
            turbulence: TurbulenceRecipe {
                model: TurbulenceModel::Laminar,
                intensity: None,
                viscosity_ratio: None,
                reference_velocity: None,
            },
            buoyancy: None,
            init: InitMode::Uniform,
        }
    }
}

impl From<&ResolvedPhysics> for PhysicsRecipe {
    fn from(p: &ResolvedPhysics) -> Self {
        Self {
            time: match p.time {
                ResolvedTime::Steady => TimeRecipe::Steady,
                ResolvedTime::Transient { delta_t } => TimeRecipe::Transient { delta_t },
            },
            turbulence: TurbulenceRecipe {
                model: p.turbulence.model,
                intensity: p.turbulence.intensity,
                viscosity_ratio: p.turbulence.viscosity_ratio,
                reference_velocity: p.turbulence.reference_velocity,
            },
            buoyancy: p.buoyancy.map(|b| BuoyancyRecipe {
                gravity: b.gravity,
                beta: b.beta,
                reference_temperature: b.reference_temperature,
                prandtl: b.prandtl,
                turbulent_prandtl: b.turbulent_prandtl,
            }),
            init: p.init,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SolverRecipe {
    pub end_time: f64,
    pub write_interval: f64,
    pub convergence_p: f64,
    pub convergence_u: f64,
}

/// A probe the case writes: where it reads and what, never what the user called it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProbeRecipe {
    pub x: f64,
    pub y: f64,
    pub field: MonitorField,
    /// The label of the boundary the point lies on, for a reading at a wall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
}

/// A sampled line the case writes, ends in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineRecipe {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub samples: u64,
    pub field: MonitorField,
}

/// A surface integral the case writes. It names the kind of boundary, as the mesh does, so two
/// boundaries of one kind with different names give the same entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceRecipe {
    pub kind: BoundaryKind,
    pub quantity: SurfaceQuantity,
    /// The label of the boundary, set only for a heat flux, which depends on which wall it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<String>,
}

/// Force coefficients on one wall, named by the boundary's label and never by the monitor's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ForceRecipe {
    pub boundary: String,
    pub reference_velocity: f64,
    pub reference_length: f64,
}

/// What the case watches: the centreline sample count and the planned monitors in plan order,
/// the default probes included, with no label and no settle tolerance. A list that is empty is
/// left out of the text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MonitorsRecipe {
    pub centreline_u_sample_count: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub probes: Vec<ProbeRecipe>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<LineRecipe>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<SurfaceRecipe>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forces: Vec<ForceRecipe>,
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
    /// Absent for a steady laminar study without buoyancy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physics: Option<PhysicsRecipe>,
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
        // The distinct fixed wall temperatures, ascending: a wall's place in this list is what
        // the mesh knows of its temperature.
        let mut temperatures: Vec<f64> = resolved
            .boundary_conditions
            .values()
            .filter_map(|b| match b.thermal {
                Some(ResolvedThermal::FixedTemperature { value }) => Some(value),
                _ => None,
            })
            .collect();
        temperatures.sort_by(f64::total_cmp);
        temperatures.dedup();
        let place = |value: f64| {
            temperatures
                .iter()
                .position(|t| *t == value)
                .and_then(|i| u32::try_from(i + 1).ok())
        };
        let mut patches: Vec<PatchRecipe> = resolved
            .boundary_conditions
            .values()
            .map(|b| PatchRecipe {
                kind: b.kind,
                faces: b.faces.clone(),
                thermal: match b.thermal {
                    Some(ResolvedThermal::FixedTemperature { value }) => place(value),
                    _ => None,
                },
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
            level: resolved.mesh.level,
            grading: resolved
                .mesh
                .grading
                .map(|g| GradingRecipe { x: g.x, y: g.y }),
            shape: resolved.domain.shape.map(ShapeRecipe::from),
            raw: mesh_raw,
        };
        let plan = resolved.monitor_plan();
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
                        profile: b.profile,
                        thermal: b.thermal.map(ThermalRecipe::from),
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
                probes: plan
                    .probes
                    .iter()
                    .map(|p| ProbeRecipe {
                        x: p.x,
                        y: p.y,
                        field: p.field,
                        on: p.boundary.clone(),
                    })
                    .collect(),
                lines: plan
                    .lines
                    .iter()
                    .map(|l| LineRecipe {
                        x0: l.x0,
                        y0: l.y0,
                        x1: l.x1,
                        y1: l.y1,
                        samples: l.samples,
                        field: l.field,
                    })
                    .collect(),
                surfaces: plan
                    .surfaces
                    .iter()
                    .map(|s| SurfaceRecipe {
                        kind: s.kind,
                        quantity: s.quantity,
                        boundary: (s.quantity == SurfaceQuantity::HeatFlux)
                            .then(|| s.boundary.clone()),
                    })
                    .collect(),
                forces: plan
                    .forces
                    .iter()
                    .map(|f| ForceRecipe {
                        boundary: f.boundary.clone(),
                        reference_velocity: f.reference_velocity,
                        reference_length: f.reference_length,
                    })
                    .collect(),
            },
            physics: resolved.physics.as_ref().map(PhysicsRecipe::from),
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
